//! Local-only conflict resolution. Snapshots are retained; no network calls.
use crate::{db::Note, notes_api::RemoteNote};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    KeepLocal,
    UseServer,
    KeepBoth,
}
impl Choice {
    fn label(self) -> &'static str {
        match self {
            Self::KeepLocal => "keep_local",
            Self::UseServer => "use_server",
            Self::KeepBoth => "keep_both",
        }
    }
}
#[derive(Serialize)]
pub struct Resolution {
    pub action: String,
    pub copy_id: Option<i64>,
}

fn fields(title: &str, content: &str, category: &str, favorite: bool) -> String {
    serde_json::json!([title, content, category, favorite]).to_string()
}
fn remote_fields(note: &RemoteNote) -> String {
    fields(&note.title, &note.content, &note.category, note.favorite)
}

pub fn apply(db: &mut Connection, id: i64, choice: Choice) -> Result<Resolution, String> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "Could not start resolution.".to_string())?;
    let prior: Option<Resolution> = tx
        .query_row(
            "SELECT action,copy_id FROM conflict_resolutions WHERE conflict_id=?1",
            [id],
            |r| {
                Ok(Resolution {
                    action: r.get(0)?,
                    copy_id: r.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|_| "Could not read resolution history.".to_string())?;
    if let Some(prior) = prior {
        return Ok(prior);
    }
    let (local_id,base_json,local_json,server_json):(i64,String,String,String)=tx.query_row(
        "SELECT local_id,base_snapshot,local_snapshot,server_snapshot FROM refresh_conflicts WHERE id=?1",[id],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|_| "Saved conflict not found.".to_string())?;
    let newest: i64 = tx
        .query_row(
            "SELECT MAX(id) FROM refresh_conflicts WHERE local_id=?1",
            [local_id],
            |r| r.get(0),
        )
        .map_err(|_| "Could not check newer comparisons.".to_string())?;
    if newest != id {
        return Err(
            "A newer comparison exists for this note. Select the newest comparison.".into(),
        );
    }
    let current: Note = tx
        .query_row(
            "SELECT id,title,content,category,favorite,modified_at FROM notes WHERE id=?1",
            [local_id],
            |r| {
                Ok(Note {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    content: r.get(2)?,
                    category: r.get(3)?,
                    favorite: r.get(4)?,
                    modified_at: r.get(5)?,
                })
            },
        )
        .map_err(|_| {
            "The local note no longer exists. Its saved comparison is still preserved.".to_string()
        })?;
    let saved: Note =
        serde_json::from_str(&local_json).map_err(|_| "Invalid local snapshot.".to_string())?;
    if fields(
        &current.title,
        &current.content,
        &current.category,
        current.favorite,
    ) != fields(
        &saved.title,
        &saved.content,
        &saved.category,
        saved.favorite,
    ) {
        return Err("This note has newer local edits. Refresh from Nextcloud to capture a new comparison before resolving.".into());
    }
    let remote: RemoteNote =
        serde_json::from_str(&server_json).map_err(|_| "Invalid server snapshot.".to_string())?;
    let base: RemoteNote =
        serde_json::from_str(&base_json).map_err(|_| "Invalid baseline snapshot.".to_string())?;
    let (remote_id, baseline): (i64, String) = tx
        .query_row(
            "SELECT remote_id,original_snapshot FROM imported_notes WHERE local_id=?1",
            [local_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| "This note's import association is missing.".to_string())?;
    let baseline: RemoteNote =
        serde_json::from_str(&baseline).map_err(|_| "Invalid current baseline.".to_string())?;
    if remote_id != remote.id
        || remote_id != base.id
        || remote_fields(&baseline) != remote_fields(&base)
    {
        return Err("This comparison is outdated. Refresh before resolving.".into());
    }
    let mut copy_id = None;
    match choice {
        Choice::KeepLocal => {}
        Choice::UseServer => {
            tx.execute("UPDATE notes SET title=?1,content=?2,category=?3,favorite=?4,modified_at=?5 WHERE id=?6",
                (&remote.title,&remote.content,&remote.category,remote.favorite,remote.modified,local_id))
                .map_err(|_| "Could not apply the saved server version. Nothing was changed.".to_string())?;
        }
        Choice::KeepBoth => {
            tx.execute("INSERT INTO notes(title,content,category,favorite,modified_at) VALUES (?1,?2,?3,?4,?5)",
                (format!("{} (server copy)",remote.title),&remote.content,&remote.category,remote.favorite,remote.modified))
                .map_err(|_| "Could not create the extra local copy. Nothing was changed.".to_string())?;
            copy_id = Some(tx.last_insert_rowid());
        }
    }
    tx.execute(
        "UPDATE imported_notes SET original_snapshot=?1 WHERE local_id=?2",
        (&server_json, local_id),
    )
    .map_err(|_| "Could not update the baseline. Nothing was changed.".to_string())?;
    tx.execute(
        "INSERT INTO conflict_resolutions(conflict_id,action,copy_id) VALUES (?1,?2,?3)",
        (id, choice.label(), copy_id),
    )
    .map_err(|_| "Could not save resolution history. Nothing was changed.".to_string())?;
    tx.execute("INSERT INTO conflict_resolutions(conflict_id,action) SELECT id,'superseded' FROM refresh_conflicts WHERE local_id=?1 AND id<?2 ON CONFLICT(conflict_id) DO NOTHING",(local_id,id))
        .map_err(|_| "Could not archive earlier comparisons. Nothing was changed.".to_string())?;
    tx.commit()
        .map_err(|_| "Could not commit resolution. Reload before retrying.".to_string())?;
    Ok(Resolution {
        action: choice.label().into(),
        copy_id,
    })
}

#[tauri::command]
pub fn resolve_conflict(id: i64, choice: Choice) -> Result<Resolution, String> {
    let mut db =
        crate::db::open_database().map_err(|_| "Could not open local storage.".to_string())?;
    apply(&mut db, id, choice)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn remote(content: &str) -> RemoteNote {
        RemoteNote {
            id: 1,
            title: "Note".into(),
            content: content.into(),
            category: "Work".into(),
            favorite: true,
            modified: 42,
            etag: None,
            readonly: None,
        }
    }
    fn refresh(db: &mut Connection, content: &str) -> crate::refresh::RefreshSummary {
        crate::refresh::store_refresh(db, "https://cloud/", "jim", &[remote(content)]).unwrap()
    }
    fn setup() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        crate::db::initialize_database(&db).unwrap();
        crate::import::store_batch(&mut db, "https://cloud/", "jim", &[remote("base")]).unwrap();
        db.execute("UPDATE notes SET content='local'", []).unwrap();
        assert_eq!(refresh(&mut db, "server").conflicts, 1);
        db
    }
    fn content(db: &Connection) -> String {
        db.query_row("SELECT content FROM notes WHERE id=1", [], |r| r.get(0))
            .unwrap()
    }
    fn count(db: &Connection, table: &str) -> i64 {
        db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    #[test]
    fn keep_local_acknowledges_server_without_changing_local_or_history() {
        let mut db = setup();
        apply(&mut db, 1, Choice::KeepLocal).unwrap();
        assert_eq!(content(&db), "local");
        let result = refresh(&mut db, "server");
        assert_eq!(result.local_preserved, 1);
        assert_eq!(result.conflicts, 0);
        assert_eq!(count(&db, "refresh_conflicts"), 1);
    }
    #[test]
    fn use_server_adopts_saved_version_and_retains_old_local_snapshot() {
        let mut db = setup();
        apply(&mut db, 1, Choice::UseServer).unwrap();
        assert_eq!(content(&db), "server");
        assert_eq!(refresh(&mut db, "server").unchanged, 1);
        let json: String = db
            .query_row(
                "SELECT local_snapshot FROM refresh_conflicts WHERE id=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Note>(&json).unwrap().content,
            "local"
        );
    }
    #[test]
    fn keep_both_creates_only_one_unlinked_copy_even_on_retry() {
        let mut db = setup();
        let first = apply(&mut db, 1, Choice::KeepBoth).unwrap();
        let retry = apply(&mut db, 1, Choice::UseServer).unwrap();
        assert_eq!(first.copy_id, retry.copy_id);
        assert_eq!(retry.action, "keep_both");
        assert_eq!(content(&db), "local");
        assert_eq!(count(&db, "notes"), 2);
        assert_eq!(count(&db, "imported_notes"), 1);
        let copy: (String, String) = db
            .query_row(
                "SELECT title,content FROM notes WHERE id=?1",
                [first.copy_id.unwrap()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(copy, ("Note (server copy)".into(), "server".into()));
    }
    #[test]
    fn newer_local_edits_are_never_overwritten() {
        let mut db = setup();
        db.execute("UPDATE notes SET content='new edit'", [])
            .unwrap();
        assert!(apply(&mut db, 1, Choice::UseServer).is_err());
        assert_eq!(content(&db), "new edit");
        assert_eq!(count(&db, "conflict_resolutions"), 0);
        refresh(&mut db, "server");
        apply(&mut db, 2, Choice::KeepLocal).unwrap();
        assert_eq!(count(&db, "conflict_resolutions"), 2);
    }
    #[test]
    fn deleted_note_is_not_resurrected() {
        let mut db = setup();
        db.execute("DELETE FROM notes", []).unwrap();
        assert!(apply(&mut db, 1, Choice::KeepBoth).is_err());
        assert_eq!(count(&db, "notes"), 0);
        assert_eq!(count(&db, "refresh_conflicts"), 1);
    }
    #[test]
    fn older_comparison_is_rejected_and_archived_when_latest_resolves() {
        let mut db = setup();
        refresh(&mut db, "new server");
        assert!(apply(&mut db, 1, Choice::UseServer).is_err());
        apply(&mut db, 2, Choice::UseServer).unwrap();
        assert_eq!(content(&db), "new server");
        assert_eq!(
            apply(&mut db, 1, Choice::KeepBoth).unwrap().action,
            "superseded"
        );
        assert_eq!(count(&db, "notes"), 1);
        assert_eq!(count(&db, "refresh_conflicts"), 2);
    }
    #[test]
    fn failed_history_write_rolls_back_note_copy_and_baseline() {
        for choice in [Choice::UseServer, Choice::KeepBoth] {
            let mut db = setup();
            db.execute_batch("CREATE TRIGGER fail_resolution BEFORE INSERT ON conflict_resolutions BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
            assert!(apply(&mut db, 1, choice).is_err());
            assert_eq!(content(&db), "local");
            assert_eq!(count(&db, "notes"), 1);
            assert_eq!(count(&db, "conflict_resolutions"), 0);
            assert_eq!(refresh(&mut db, "server").conflicts, 1);
        }
    }
    #[test]
    fn changed_baseline_rejects_outdated_comparison() {
        let mut db = setup();
        refresh(&mut db, "local");
        assert!(apply(&mut db, 1, Choice::UseServer).is_err());
        assert_eq!(content(&db), "local");
    }
    #[test]
    fn recurring_conflicting_pair_gets_new_unresolved_history_entry() {
        let mut db = setup();
        apply(&mut db, 1, Choice::KeepLocal).unwrap();
        refresh(&mut db, "base");
        apply(&mut db, 2, Choice::UseServer).unwrap();
        db.execute("UPDATE notes SET content='local'", []).unwrap();
        assert_eq!(refresh(&mut db, "server").conflicts, 1);
        assert_eq!(count(&db, "refresh_conflicts"), 3);
        assert_eq!(count(&db, "conflict_resolutions"), 2);
        refresh(&mut db, "server");
        assert_eq!(count(&db, "refresh_conflicts"), 3);
        apply(&mut db, 3, Choice::KeepLocal).unwrap();
    }
}
