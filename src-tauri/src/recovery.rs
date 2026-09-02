//! Explicit local association repair; never sends a network request or deletes a note.
use crate::{db::Note, notes_api::RemoteNote};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;

#[derive(Serialize)]
pub struct Item {
    local_id: i64,
    title: String,
    server: String,
    account: String,
}
#[derive(Serialize)]
pub struct Candidate {
    copy: Note,
    remote: RemoteNote,
    token: String,
}
#[derive(Serialize)]
pub struct Review {
    local: Note,
    sent: Note,
    server: String,
    account: String,
    candidates: Vec<Candidate>,
}

fn note(db: &Connection, id: i64) -> Result<Note, String> {
    db.query_row(
        "SELECT id,title,content,category,favorite,modified_at FROM notes WHERE id=?1",
        [id],
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
    .map_err(|_| "The local note no longer exists. Recovery will not recreate it.".into())
}
fn fields(title: &str, content: &str, category: &str, favorite: bool) -> serde_json::Value {
    serde_json::json!([title, content, category, favorite])
}
fn list(db: &Connection) -> Result<Vec<Item>, String> {
    let mut query=db.prepare("SELECT a.local_id,n.title,a.server,a.account FROM note_creation_attempts a JOIN notes n ON n.id=a.local_id WHERE a.completed=0 ORDER BY a.local_id DESC").map_err(|_| "Could not list interrupted uploads.".to_string())?;
    let rows = query
        .query_map([], |r| {
            Ok(Item {
                local_id: r.get(0)?,
                title: r.get(1)?,
                server: r.get(2)?,
                account: r.get(3)?,
            })
        })
        .map_err(|_| "Could not read interrupted uploads.".to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Could not read interrupted uploads.".into())
}
fn review(db: &Connection, id: i64) -> Result<Review, String> {
    let (server,account,sent_json,recorded):(String,String,String,Option<String>)=db.query_row("SELECT server,account,local_snapshot,server_snapshot FROM note_creation_attempts WHERE local_id=?1 AND completed=0",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))
        .map_err(|_| "This upload is no longer awaiting recovery. Reopen the recovery screen.".to_string())?;
    let local = note(db, id)?;
    let linked: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM imported_notes WHERE local_id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(|_| "Could not check the original note's association.".to_string())?;
    if linked {
        return Err(
            "This local note already has a server association. Recovery was stopped.".into(),
        );
    }
    let sent: Note = serde_json::from_str(&sent_json)
        .map_err(|_| "Invalid saved upload snapshot.".to_string())?;
    if sent.id != id {
        return Err("Invalid saved upload identity.".into());
    }
    let known: Option<RemoteNote> = recorded
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| "Invalid saved server response.".to_string())?;
    let mut query=db.prepare("SELECT i.local_id,i.remote_id,i.original_snapshot FROM imported_notes i JOIN notes n ON n.id=i.local_id WHERE i.server=?1 AND i.account=?2 AND NOT EXISTS(SELECT 1 FROM refresh_conflicts c LEFT JOIN conflict_resolutions r ON r.conflict_id=c.id WHERE c.local_id=i.local_id AND r.conflict_id IS NULL) AND NOT EXISTS(SELECT 1 FROM note_creation_attempts a WHERE a.local_id=i.local_id AND a.completed=0)").map_err(|_| "Could not find downloaded server copies.".to_string())?;
    let rows = query
        .query_map((&server, &account), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| "Could not read downloaded server copies.".to_string())?;
    let mut candidates = Vec::new();
    for row in rows {
        let (copy_id, remote_id, snapshot) =
            row.map_err(|_| "Could not read a server copy.".to_string())?;
        let remote: RemoteNote = serde_json::from_str(&snapshot)
            .map_err(|_| "Invalid downloaded server snapshot.".to_string())?;
        if remote.id != remote_id || remote.id <= 0 {
            return Err("Invalid downloaded server identity.".into());
        }
        // A recorded response provides identity. Without it, exact submitted
        // text/favorite only suggests candidates; the user must confirm identity.
        if let Some(known) = &known {
            if remote.id != known.id {
                continue;
            }
        } else if remote.content != sent.content || remote.favorite != sent.favorite {
            continue;
        }
        let copy = note(db, copy_id)?;
        if fields(&copy.title, &copy.content, &copy.category, copy.favorite)
            != fields(
                &remote.title,
                &remote.content,
                &remote.category,
                remote.favorite,
            )
        {
            continue;
        }
        let token =
            serde_json::to_string(&(&local, &sent, &server, &account, &recorded, &copy, &remote))
                .map_err(|_| "Could not prepare recovery comparison.".to_string())?;
        candidates.push(Candidate {
            copy,
            remote,
            token,
        });
    }
    Ok(Review {
        local,
        sent,
        server,
        account,
        candidates,
    })
}
fn apply(db: &mut Connection, id: i64, copy_id: i64, token: &str) -> Result<(), String> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "Could not start recovery.".to_string())?;
    let prior: Option<i64> = tx
        .query_row(
            "SELECT copy_id FROM creation_recoveries WHERE local_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| "Could not read recovery history.".to_string())?;
    if let Some(prior) = prior {
        return if prior == copy_id {
            Ok(())
        } else {
            Err("This upload was already recovered using another copy.".into())
        };
    }
    let current = review(&tx, id)?;
    let candidate=current.candidates.into_iter().find(|candidate|candidate.copy.id==copy_id && candidate.token==token)
        .ok_or("The comparison changed or the selected copy is no longer eligible. Reopen recovery and review it again.")?;
    let update = || -> Result<(), Box<dyn std::error::Error>> {
        let changed=tx.execute("UPDATE imported_notes SET local_id=?1 WHERE local_id=?2 AND server=?3 AND account=?4 AND remote_id=?5",(id,copy_id,&current.server,&current.account,candidate.remote.id))?;
        if changed != 1 {
            return Err("Association changed".into());
        }
        tx.execute(
            "UPDATE note_creation_attempts SET completed=1 WHERE local_id=?1",
            [id],
        )?;
        tx.execute(
            "INSERT INTO creation_recoveries(local_id,copy_id,review_snapshot) VALUES (?1,?2,?3)",
            (id, copy_id, token),
        )?;
        tx.commit()?;
        Ok(())
    };
    update().map_err(|_| "Could not commit recovery. No recovery changes were committed; reopen the screen before retrying.".into())
}
#[tauri::command]
pub fn list_creation_recoveries() -> Result<Vec<Item>, String> {
    let db = crate::db::open_database().map_err(|_| "Could not open local storage.".to_string())?;
    list(&db)
}
#[tauri::command]
pub fn get_creation_recovery(id: i64) -> Result<Review, String> {
    let db = crate::db::open_database().map_err(|_| "Could not open local storage.".to_string())?;
    review(&db, id)
}
#[tauri::command]
pub fn recover_creation(id: i64, copy_id: i64, token: String) -> Result<(), String> {
    let mut db =
        crate::db::open_database().map_err(|_| "Could not open local storage.".to_string())?;
    apply(&mut db, id, copy_id, &token)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SERVER: &str = "https://cloud/";
    fn remote(id: i64) -> RemoteNote {
        RemoteNote {
            id,
            title: "Disposable".into(),
            content: "original text".into(),
            category: "Personal".into(),
            favorite: false,
            modified: 2,
            etag: Some(format!("etag{id}")),
            readonly: Some(false),
        }
    }
    fn setup() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        seed(&mut db);
        db
    }
    fn seed(db: &mut Connection) {
        crate::db::initialize_database(db).unwrap();
        db.execute("INSERT INTO notes(title,content,category,favorite,modified_at) VALUES ('Disposable','original text','Personal',0,1)",[]).unwrap();
        crate::create_remote::begin(db, SERVER, "jim", 1).unwrap();
        crate::import::store_batch(db, SERVER, "jim", &[remote(7)]).unwrap();
    }
    fn recover(db: &mut Connection) {
        let preview = review(db, 1).unwrap();
        let candidate = &preview.candidates[0];
        apply(db, 1, candidate.copy.id, &candidate.token).unwrap();
    }
    #[test]
    fn transfers_only_link_preserves_both_notes_and_allows_updates() {
        let mut db = setup();
        db.execute("UPDATE notes SET content='newer local text' WHERE id=1", [])
            .unwrap();
        assert_eq!(list(&db).unwrap().len(), 1);
        let preview = review(&db, 1).unwrap();
        assert_eq!(preview.candidates.len(), 1);
        let candidate = &preview.candidates[0];
        apply(&mut db, 1, candidate.copy.id, &candidate.token).unwrap();
        apply(&mut db, 1, candidate.copy.id, &candidate.token).unwrap();
        assert!(list(&db).unwrap().is_empty());
        assert_eq!(note(&db, 1).unwrap().content, "newer local text");
        assert_eq!(note(&db, 2).unwrap().content, "original text");
        let linked_id: i64 = db
            .query_row(
                "SELECT local_id FROM imported_notes WHERE remote_id=7",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked_id, 1);
        assert!(crate::upload::prepare(&db, SERVER, "jim", 1).is_ok());
        assert!(crate::create_remote::begin(&mut db, SERVER, "jim", 1).is_err());
        assert_eq!(
            crate::refresh::store_refresh(&mut db, SERVER, "jim", &[remote(7)])
                .unwrap()
                .local_preserved,
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM creation_recoveries", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn never_suggests_another_account_or_server() {
        let mut db = setup();
        crate::import::store_batch(&mut db, SERVER, "other", &[remote(7)]).unwrap();
        crate::import::store_batch(&mut db, "https://other/", "jim", &[remote(7)]).unwrap();
        let preview = review(&db, 1).unwrap();
        assert_eq!(preview.candidates.len(), 1);
        assert_eq!(preview.candidates[0].copy.id, 2);
        assert!(apply(&mut db, 1, 3, &preview.candidates[0].token).is_err());
    }
    #[test]
    fn ambiguous_text_matches_are_separate_choices_not_automatic_links() {
        let mut db = setup();
        crate::import::store_batch(&mut db, SERVER, "jim", &[remote(8)]).unwrap();
        assert_eq!(review(&db, 1).unwrap().candidates.len(), 2);
        assert_eq!(list(&db).unwrap().len(), 1);
        assert!(apply(&mut db, 1, 2, "invented token").is_err());
    }
    #[test]
    fn recorded_server_identity_overrides_text_similarity() {
        let mut db = setup();
        db.execute(
            "UPDATE note_creation_attempts SET server_snapshot=?1 WHERE local_id=1",
            [serde_json::to_string(&remote(7)).unwrap()],
        )
        .unwrap();
        crate::import::store_batch(&mut db, SERVER, "jim", &[remote(8)]).unwrap();
        let mut changed = remote(7);
        changed.content = "edited on server".into();
        crate::refresh::store_refresh(&mut db, SERVER, "jim", &[changed]).unwrap();
        let preview = review(&db, 1).unwrap();
        assert_eq!(preview.candidates.len(), 1);
        assert_eq!(preview.candidates[0].remote.id, 7);
        assert_eq!(preview.candidates[0].remote.content, "edited on server");
    }
    #[test]
    fn unknown_identity_requires_original_text_and_does_not_unlock_on_no_match() {
        let mut db = setup();
        let mut changed = remote(7);
        changed.content = "different".into();
        crate::refresh::store_refresh(&mut db, SERVER, "jim", &[changed]).unwrap();
        assert!(review(&db, 1).unwrap().candidates.is_empty());
        assert!(crate::create_remote::begin(&mut db, SERVER, "jim", 1).is_err());
    }
    #[test]
    fn changed_original_copy_or_server_snapshot_invalidates_confirmation() {
        for sql in ["UPDATE notes SET content='newer' WHERE id=1", "UPDATE notes SET title='changed' WHERE id=2", "UPDATE imported_notes SET original_snapshot=json_set(original_snapshot,'$.etag','new')"] {
            let mut db=setup(); let preview=review(&db,1).unwrap(); let candidate=&preview.candidates[0];
            db.execute(sql,[]).unwrap();
            assert!(apply(&mut db,1,candidate.copy.id,&candidate.token).is_err());
            assert_eq!(list(&db).unwrap().len(),1);
        }
    }
    #[test]
    fn excludes_edited_or_conflicted_server_copy() {
        let mut db = setup();
        db.execute("UPDATE notes SET content='copy edit' WHERE id=2", [])
            .unwrap();
        assert!(review(&db, 1).unwrap().candidates.is_empty());
        let mut changed = remote(7);
        changed.title = "Server edit".into();
        crate::refresh::store_refresh(&mut db, SERVER, "jim", &[changed]).unwrap();
        db.execute("UPDATE notes SET content='original text' WHERE id=2", [])
            .unwrap();
        assert!(review(&db, 1).unwrap().candidates.is_empty());
    }
    #[test]
    fn deleted_notes_are_not_resurrected() {
        for id in [1, 2] {
            let mut db = setup();
            let preview = review(&db, 1).unwrap();
            let candidate = &preview.candidates[0];
            db.execute("DELETE FROM notes WHERE id=?1", [id]).unwrap();
            assert!(apply(&mut db, 1, candidate.copy.id, &candidate.token).is_err());
            assert!(note(&db, id).is_err());
        }
    }
    #[test]
    fn transaction_failure_rolls_back_link_and_completion() {
        let mut db = setup();
        db.execute_batch("CREATE TRIGGER fail_recovery BEFORE INSERT ON creation_recoveries BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        let preview = review(&db, 1).unwrap();
        let candidate = &preview.candidates[0];
        assert!(apply(&mut db, 1, candidate.copy.id, &candidate.token).is_err());
        assert_eq!(list(&db).unwrap().len(), 1);
        assert_eq!(review(&db, 1).unwrap().candidates.len(), 1);
        assert!(crate::upload::prepare(&db, SERVER, "jim", 2).is_ok());
        assert_eq!(note(&db, 1).unwrap().content, "original text");
    }
    #[test]
    fn completed_recovery_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sqlite");
        {
            let mut db = Connection::open(&path).unwrap();
            seed(&mut db);
            recover(&mut db);
        }
        let db = Connection::open(&path).unwrap();
        crate::db::initialize_database(&db).unwrap();
        assert!(list(&db).unwrap().is_empty());
        assert!(crate::upload::prepare(&db, SERVER, "jim", 1).is_ok());
        assert_eq!(note(&db, 2).unwrap().content, "original text");
    }
}
