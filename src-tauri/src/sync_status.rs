//! Status from local storage only; never claims to know the live server state.
use crate::notes_api::RemoteNote;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    LocalOnly,
    LocalChanges,
    MatchesSnapshot,
    ConflictSaved,
    RecoveryNeeded,
}
#[derive(Serialize)]
pub struct Status {
    kind: Kind,
    server: Option<String>,
    account: Option<String>,
}

fn read(db: &mut Connection, id: i64) -> Result<Status, String> {
    // One consistent database snapshot for flags, association and note contents.
    let tx = db
        .transaction()
        .map_err(|_| "Could not read sync status.".to_string())?;
    let (title, content, category, favorite): (String, String, String, bool) = tx
        .query_row(
            "SELECT title,content,category,favorite FROM notes WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|_| "Note not found.".to_string())?;
    let recovery: Option<(String, String)> = tx
        .query_row(
            "SELECT server,account FROM note_creation_attempts WHERE local_id=?1 AND completed=0",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|_| "Could not check interrupted uploads.".to_string())?;
    if let Some((server, account)) = recovery {
        return Ok(Status {
            kind: Kind::RecoveryNeeded,
            server: Some(server),
            account: Some(account),
        });
    }
    let mapping:Option<(String,String,i64,String)>=tx.query_row("SELECT server,account,remote_id,original_snapshot FROM imported_notes WHERE local_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_| "Could not read the server association.".to_string())?;
    let Some((server, account, remote_id, snapshot)) = mapping else {
        return Ok(Status {
            kind: Kind::LocalOnly,
            server: None,
            account: None,
        });
    };
    let conflict:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM refresh_conflicts c LEFT JOIN conflict_resolutions r ON r.conflict_id=c.id WHERE c.local_id=?1 AND r.conflict_id IS NULL)",[id],|r|r.get(0)).map_err(|_| "Could not check saved conflicts.".to_string())?;
    let kind = if conflict {
        Kind::ConflictSaved
    } else {
        let baseline: RemoteNote = serde_json::from_str(&snapshot)
            .map_err(|_| "Invalid saved server snapshot.".to_string())?;
        if baseline.id != remote_id || remote_id <= 0 {
            return Err("Invalid saved server identity.".into());
        }
        if title == baseline.title
            && content == baseline.content
            && category == baseline.category
            && favorite == baseline.favorite
        {
            Kind::MatchesSnapshot
        } else {
            Kind::LocalChanges
        }
    };
    Ok(Status {
        kind,
        server: Some(server),
        account: Some(account),
    })
}
#[tauri::command]
pub fn get_note_sync_status(id: i64) -> Result<Status, String> {
    let mut db =
        crate::db::open_database().map_err(|_| "Could not open local storage.".to_string())?;
    read(&mut db, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn remote() -> RemoteNote {
        RemoteNote {
            id: 7,
            title: "Note".into(),
            content: "text".into(),
            category: "Personal".into(),
            favorite: false,
            modified: 1,
            etag: Some("tag".into()),
            readonly: Some(false),
        }
    }
    fn setup() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        crate::db::initialize_database(&db).unwrap();
        crate::import::store_batch(&mut db, "https://cloud/", "jim", &[remote()]).unwrap();
        db
    }
    #[test]
    fn matches_saved_snapshot_without_claiming_live_sync() {
        let mut db = setup();
        let status = read(&mut db, 1).unwrap();
        assert_eq!(status.kind, Kind::MatchesSnapshot);
        assert_eq!(status.server.as_deref(), Some("https://cloud/"));
        assert_eq!(status.account.as_deref(), Some("jim"));
        db.execute("UPDATE notes SET modified_at=999", []).unwrap();
        assert_eq!(read(&mut db, 1).unwrap().kind, Kind::MatchesSnapshot);
    }
    #[test]
    fn every_editable_field_affects_status() {
        for assignment in [
            "title='different'",
            "content='different'",
            "category='Work'",
            "favorite=1",
        ] {
            let mut db = setup();
            db.execute(&format!("UPDATE notes SET {assignment}"), [])
                .unwrap();
            assert_eq!(read(&mut db, 1).unwrap().kind, Kind::LocalChanges);
        }
    }
    #[test]
    fn local_only_and_interrupted_creation_have_distinct_statuses() {
        let mut db = setup();
        db.execute(
            "INSERT INTO notes(title,content,modified_at) VALUES ('local','text',1)",
            [],
        )
        .unwrap();
        assert_eq!(read(&mut db, 2).unwrap().kind, Kind::LocalOnly);
        crate::create_remote::begin(&mut db, "https://cloud/", "jim", 2).unwrap();
        assert_eq!(read(&mut db, 2).unwrap().kind, Kind::RecoveryNeeded);
    }
    #[test]
    fn conflict_wins_over_local_changes_but_resolved_history_does_not() {
        let mut db = setup();
        db.execute("UPDATE notes SET content='local'", []).unwrap();
        let mut server = remote();
        server.content = "server".into();
        crate::refresh::store_refresh(&mut db, "https://cloud/", "jim", &[server]).unwrap();
        assert_eq!(read(&mut db, 1).unwrap().kind, Kind::ConflictSaved);
        crate::resolution::apply(&mut db, 1, crate::resolution::Choice::KeepLocal).unwrap();
        assert_eq!(read(&mut db, 1).unwrap().kind, Kind::LocalChanges);
    }
    #[test]
    fn using_server_resolves_to_snapshot_status() {
        let mut db = setup();
        db.execute("UPDATE notes SET content='local'", []).unwrap();
        let mut server = remote();
        server.content = "server".into();
        crate::refresh::store_refresh(&mut db, "https://cloud/", "jim", &[server]).unwrap();
        crate::resolution::apply(&mut db, 1, crate::resolution::Choice::UseServer).unwrap();
        assert_eq!(read(&mut db, 1).unwrap().kind, Kind::MatchesSnapshot);
    }
    #[test]
    fn missing_or_corrupt_data_never_reports_synced() {
        let mut db = setup();
        assert!(read(&mut db, 999).is_err());
        db.execute("UPDATE imported_notes SET original_snapshot='invalid'", [])
            .unwrap();
        assert!(read(&mut db, 1).is_err());
    }
}
