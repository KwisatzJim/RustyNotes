//! Three-way, download-only refresh. Never deletes notes or overwrites local edits.
use crate::{db::Note, notes_api::RemoteNote};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Default, Serialize)]
pub struct RefreshSummary {
    pub added: usize,
    pub updated: usize,
    pub local_preserved: usize,
    pub conflicts: usize,
    pub unchanged: usize,
    pub locally_deleted: usize,
    pub server_missing: usize,
}

fn key(title: &str, content: &str, category: &str, favorite: bool) -> String {
    // A lossless comparison key, not a hash. Timestamps aren't edit evidence.
    serde_json::json!([title, content, category, favorite]).to_string()
}
fn remote_key(note: &RemoteNote) -> String {
    key(&note.title, &note.content, &note.category, note.favorite)
}

fn apply(
    connection: &mut Connection,
    server: &str,
    account: &str,
    notes: &[RemoteNote],
) -> Result<RefreshSummary, Box<dyn std::error::Error>> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS imported_notes (
        server TEXT NOT NULL, account TEXT NOT NULL, remote_id INTEGER NOT NULL,
        local_id INTEGER NOT NULL UNIQUE, original_snapshot TEXT NOT NULL,
        PRIMARY KEY(server, account, remote_id));",
    )?;
    let mut summary = RefreshSummary::default();
    let mut seen = HashSet::new();
    for remote in notes {
        if remote.id <= 0 || remote.modified < 0 || !seen.insert(remote.id) {
            return Err("Invalid remote batch".into());
        }
        let mapping: Option<(i64, String)> = tx.query_row(
            "SELECT local_id, original_snapshot FROM imported_notes WHERE server=?1 AND account=?2 AND remote_id=?3",
            (server, account, remote.id), |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        let remote_snapshot = serde_json::to_string(remote)?;
        let Some((local_id, baseline)) = mapping else {
            tx.execute("INSERT INTO notes (title, content, category, favorite, modified_at) VALUES (?1,?2,?3,?4,?5)",
                (&remote.title, &remote.content, &remote.category, remote.favorite, remote.modified))?;
            tx.execute("INSERT INTO imported_notes (server,account,remote_id,local_id,original_snapshot) VALUES (?1,?2,?3,?4,?5)",
                (server, account, remote.id, tx.last_insert_rowid(), remote_snapshot))?;
            summary.added += 1;
            continue;
        };
        let local: Option<Note> = tx
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
            .optional()?;
        let Some(local) = local else {
            summary.locally_deleted += 1;
            continue;
        };
        let base: RemoteNote = serde_json::from_str(&baseline)?;
        if base.id != remote.id {
            return Err("Invalid baseline identity".into());
        }
        let local_key = key(
            &local.title,
            &local.content,
            &local.category,
            local.favorite,
        );
        let server_key = remote_key(remote);
        let base_key = remote_key(&base);
        if local_key == server_key {
            summary.unchanged += 1;
        } else if local_key == base_key {
            tx.execute("UPDATE notes SET title=?1,content=?2,category=?3,favorite=?4,modified_at=?5 WHERE id=?6",
                (&remote.title,&remote.content,&remote.category,remote.favorite,remote.modified,local_id))?;
            summary.updated += 1;
        } else if server_key == base_key {
            summary.local_preserved += 1;
            continue;
        } else {
            // A previously resolved pair may recur later. Give each resolution
            // generation its own deduplication key while retaining all history.
            let generation: i64 = tx.query_row("SELECT COALESCE(MAX(r.conflict_id),0) FROM conflict_resolutions r JOIN refresh_conflicts c ON c.id=r.conflict_id WHERE c.local_id=?1", [local_id], |r|r.get(0))?;
            let local_key = if generation == 0 {
                local_key
            } else {
                serde_json::json!([local_key, generation]).to_string()
            };
            tx.execute("INSERT INTO refresh_conflicts (local_id,base_snapshot,local_snapshot,server_snapshot,local_key,server_key)
                VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(local_id,local_key,server_key) DO NOTHING",
                (local_id, &baseline, serde_json::to_string(&local)?, &remote_snapshot, local_key, server_key))?;
            summary.conflicts += 1;
            continue;
        }
        // Only advance the common baseline after adopting/converging versions.
        tx.execute(
            "UPDATE imported_notes SET original_snapshot=?1 WHERE local_id=?2",
            (&remote_snapshot, local_id),
        )?;
    }
    {
        let mut query = tx.prepare("SELECT remote_id FROM imported_notes JOIN notes ON notes.id=imported_notes.local_id WHERE server=?1 AND account=?2")?;
        for id in query.query_map((server, account), |r| r.get::<_, i64>(0))? {
            if !seen.contains(&id?) {
                summary.server_missing += 1;
            }
        }
    }
    tx.commit()?;
    Ok(summary)
}

pub fn store_refresh(
    connection: &mut Connection,
    server: &str,
    account: &str,
    notes: &[RemoteNote],
) -> Result<RefreshSummary, String> {
    apply(connection, server, account, notes).map_err(|_| "Could not safely apply the refresh. No refresh changes were committed; please retry. Existing notes and conflicts were preserved.".into())
}

#[derive(Serialize)]
pub struct ConflictSummary {
    id: i64,
    local_id: i64,
    title: String,
    resolution: Option<String>,
}
#[derive(Serialize)]
pub struct ConflictDetail {
    local: Note,
    server: RemoteNote,
    resolution: Option<String>,
}

#[tauri::command]
pub fn list_refresh_conflicts() -> Result<Vec<ConflictSummary>, String> {
    let connection =
        crate::db::open_database().map_err(|_| "Could not open conflict storage.".to_string())?;
    let mut query = connection
        .prepare("SELECT c.id,c.local_id,c.local_snapshot,r.action FROM refresh_conflicts c LEFT JOIN conflict_resolutions r ON r.conflict_id=c.id ORDER BY c.id DESC")
        .map_err(|_| "Could not read saved conflicts.".to_string())?;
    let rows = query
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|_| "Could not list saved conflicts.".to_string())?;
    let mut result = Vec::new();
    for row in rows {
        let (id, local_id, snapshot, resolution) =
            row.map_err(|_| "Could not read a saved conflict.".to_string())?;
        let local: Note = serde_json::from_str(&snapshot)
            .map_err(|_| "Invalid conflict snapshot.".to_string())?;
        result.push(ConflictSummary {
            id,
            local_id,
            title: local.title,
            resolution,
        });
    }
    Ok(result)
}

#[tauri::command]
pub fn get_refresh_conflict(id: i64) -> Result<ConflictDetail, String> {
    let connection =
        crate::db::open_database().map_err(|_| "Could not open conflict storage.".to_string())?;
    let (local, server): (String, String) = connection
        .query_row(
            "SELECT local_snapshot,server_snapshot FROM refresh_conflicts WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| "Saved conflict not found.".to_string())?;
    Ok(ConflictDetail {
        local: serde_json::from_str(&local).map_err(|_| "Invalid local snapshot.".to_string())?,
        server: serde_json::from_str(&server)
            .map_err(|_| "Invalid server snapshot.".to_string())?,
        resolution: connection
            .query_row(
                "SELECT action FROM conflict_resolutions WHERE conflict_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| "Could not read resolution history.".to_string())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn remote(content: &str) -> RemoteNote {
        RemoteNote {
            id: 1,
            title: "Title".into(),
            content: content.into(),
            category: "Work".into(),
            favorite: false,
            modified: 10,
            etag: Some("etag".into()),
            readonly: Some(false),
        }
    }
    fn setup() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        crate::db::initialize_database(&db).unwrap();
        crate::import::store_batch(&mut db, "https://cloud/", "jim", &[remote("base")]).unwrap();
        db
    }
    fn refresh(db: &mut Connection, content: &str) -> RefreshSummary {
        store_refresh(db, "https://cloud/", "jim", &[remote(content)]).unwrap()
    }
    fn content(db: &Connection) -> String {
        db.query_row("SELECT content FROM notes WHERE id=1", [], |r| r.get(0))
            .unwrap()
    }
    fn conflict_count(db: &Connection) -> i64 {
        db.query_row("SELECT COUNT(*) FROM refresh_conflicts", [], |r| r.get(0))
            .unwrap()
    }
    #[test]
    fn remote_only_change_updates_and_repeat_is_unchanged() {
        let mut db = setup();
        assert_eq!(refresh(&mut db, "server edit").updated, 1);
        assert_eq!(content(&db), "server edit");
        assert_eq!(refresh(&mut db, "server edit").unchanged, 1);
    }
    #[test]
    fn local_only_change_is_never_overwritten() {
        let mut db = setup();
        db.execute("UPDATE notes SET content='local edit'", [])
            .unwrap();
        assert_eq!(refresh(&mut db, "base").local_preserved, 1);
        assert_eq!(content(&db), "local edit");
        assert_eq!(conflict_count(&db), 0);
    }
    #[test]
    fn both_changed_preserves_both_and_repeat_does_not_duplicate_conflict() {
        let mut db = setup();
        db.execute("UPDATE notes SET content='local edit'", [])
            .unwrap();
        assert_eq!(refresh(&mut db, "server edit").conflicts, 1);
        assert_eq!(content(&db), "local edit");
        db.execute("UPDATE notes SET modified_at=999", []).unwrap();
        assert_eq!(refresh(&mut db, "server edit").conflicts, 1);
        assert_eq!(conflict_count(&db), 1);
        let (local, server, base): (String, String, String) = db
            .query_row(
                "SELECT local_snapshot,server_snapshot,base_snapshot FROM refresh_conflicts",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Note>(&local).unwrap().content,
            "local edit"
        );
        assert_eq!(
            serde_json::from_str::<RemoteNote>(&server).unwrap().content,
            "server edit"
        );
        assert_eq!(
            serde_json::from_str::<RemoteNote>(&base).unwrap().content,
            "base"
        );
        assert_eq!(refresh(&mut db, "another server version").conflicts, 1);
        assert_eq!(conflict_count(&db), 2);
    }
    #[test]
    fn converged_edits_advance_baseline_without_conflict() {
        let mut db = setup();
        db.execute("UPDATE notes SET content='same edit'", [])
            .unwrap();
        assert_eq!(refresh(&mut db, "same edit").unchanged, 1);
        assert_eq!(refresh(&mut db, "next server edit").updated, 1);
        assert_eq!(conflict_count(&db), 0);
    }
    #[test]
    fn timestamp_only_local_save_is_not_a_conflict() {
        let mut db = setup();
        db.execute("UPDATE notes SET modified_at=9999", []).unwrap();
        assert_eq!(refresh(&mut db, "remote").updated, 1);
    }
    #[test]
    fn categories_titles_and_favorites_are_part_of_comparison() {
        let mut db = setup();
        db.execute("UPDATE notes SET favorite=1,category='Personal'", [])
            .unwrap();
        assert_eq!(refresh(&mut db, "remote").conflicts, 1);
        let mut incoming = remote("base");
        incoming.title = "Renamed".into();
        assert_eq!(
            store_refresh(&mut db, "https://cloud/", "jim", &[incoming])
                .unwrap()
                .conflicts,
            1
        );
        assert_eq!(conflict_count(&db), 2);
    }
    #[test]
    fn local_and_server_deletions_are_preserved() {
        let mut db = setup();
        assert_eq!(
            store_refresh(&mut db, "https://cloud/", "jim", &[])
                .unwrap()
                .server_missing,
            1
        );
        assert_eq!(content(&db), "base");
        db.execute("DELETE FROM notes WHERE id=1", []).unwrap();
        assert_eq!(refresh(&mut db, "remote").locally_deleted, 1);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn other_accounts_and_local_only_notes_are_untouched() {
        let mut db = setup();
        assert_eq!(
            store_refresh(
                &mut db,
                "https://cloud/",
                "other",
                &[remote("other account")]
            )
            .unwrap()
            .added,
            1
        );
        assert_eq!(content(&db), "base");
        db.execute(
            "INSERT INTO notes(title,content,modified_at) VALUES ('local','mine',1)",
            [],
        )
        .unwrap();
        assert_eq!(refresh(&mut db, "base").unchanged, 1);
        assert_eq!(
            db.query_row("SELECT content FROM notes WHERE title='local'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            "mine"
        );
    }
    #[test]
    fn mid_transaction_failure_rolls_back_remote_update_and_baseline() {
        let mut db = setup();
        db.execute_batch("CREATE TRIGGER fail_insert BEFORE INSERT ON notes BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
        let mut new = remote("new");
        new.id = 2;
        assert!(store_refresh(&mut db, "https://cloud/", "jim", &[remote("remote"), new]).is_err());
        assert_eq!(content(&db), "base");
        let baseline: String = db
            .query_row(
                "SELECT original_snapshot FROM imported_notes WHERE local_id=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<RemoteNote>(&baseline)
                .unwrap()
                .content,
            "base"
        );
    }
    #[test]
    fn malformed_baseline_aborts_without_changes() {
        let mut db = setup();
        db.execute("UPDATE imported_notes SET original_snapshot='invalid'", [])
            .unwrap();
        assert!(store_refresh(&mut db, "https://cloud/", "jim", &[remote("remote")]).is_err());
        assert_eq!(content(&db), "base");
    }
}
