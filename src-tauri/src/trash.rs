//! Local-only Trash. Sync mappings remain intact while active notes are absent.
use crate::db::{self, Note};
use rusqlite::{Connection, Result, TransactionBehavior};

fn read_note(row: &rusqlite::Row<'_>) -> Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        category: row.get(3)?,
        favorite: row.get(4)?,
        modified_at: row.get(5)?,
    })
}

pub(crate) fn move_note(connection: &mut Connection, id: i64) -> Result<()> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Both changes commit together, or neither happens. Never replace a saved copy.
    let added = tx.execute("INSERT INTO trashed_notes SELECT id,title,content,category,favorite,modified_at FROM notes WHERE id=?1", [id])?;
    if added == 0 {
        // Retrying an already completed move is harmless.
        tx.query_row("SELECT id FROM trashed_notes WHERE id=?1", [id], |row| {
            row.get::<_, i64>(0)
        })?;
    } else {
        tx.execute("DELETE FROM notes WHERE id=?1", [id])?;
    }
    tx.commit()
}

fn restore(connection: &mut Connection, id: i64) -> Result<Note> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let note = tx.query_row(
        "SELECT id,title,content,category,favorite,modified_at FROM trashed_notes WHERE id=?1",
        [id],
        read_note,
    )?;
    tx.execute("INSERT INTO notes SELECT id,title,content,category,favorite,modified_at FROM trashed_notes WHERE id=?1", [id])?;
    tx.execute("DELETE FROM trashed_notes WHERE id=?1", [id])?;
    tx.commit()?;
    Ok(note)
}

#[tauri::command]
pub fn list_trashed_notes() -> std::result::Result<Vec<Note>, String> {
    let connection = db::open_database().map_err(|e| e.to_string())?;
    let mut statement = connection.prepare("SELECT id,title,content,category,favorite,modified_at FROM trashed_notes ORDER BY modified_at DESC,id DESC").map_err(|e| e.to_string())?;
    let notes = statement
        .query_map([], read_note)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    Ok(notes)
}

#[tauri::command]
pub fn restore_note(id: i64) -> std::result::Result<Note, String> {
    restore(&mut db::open_database().map_err(|e| e.to_string())?, id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        db::initialize_database(&c).unwrap();
        c.execute(
            "INSERT INTO notes VALUES(1,'Title','# Text','Work',1,123)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO imported_notes VALUES('server','account',42,1,'snapshot')",
            [],
        )
        .unwrap();
        c
    }
    fn count(c: &Connection, table: &str) -> i64 {
        c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    #[test]
    fn round_trip_preserves_fields_and_sync_mapping() {
        let mut c = setup();
        move_note(&mut c, 1).unwrap();
        move_note(&mut c, 1).unwrap();
        assert_eq!(count(&c, "notes"), 0);
        assert_eq!(count(&c, "imported_notes"), 1);
        let note = restore(&mut c, 1).unwrap();
        assert_eq!(
            (
                note.id,
                note.title.as_str(),
                note.content.as_str(),
                note.category.as_str(),
                note.favorite,
                note.modified_at
            ),
            (1, "Title", "# Text", "Work", true, 123)
        );
        assert_eq!(count(&c, "trashed_notes"), 0);
        assert!(restore(&mut c, 1).is_err());
        assert_eq!(count(&c, "notes"), 1);
    }
    #[test]
    fn failed_move_rolls_back_saved_copy() {
        let mut c = setup();
        c.execute_batch(
            "CREATE TRIGGER fail BEFORE DELETE ON notes BEGIN SELECT RAISE(ABORT,'test'); END;",
        )
        .unwrap();
        assert!(move_note(&mut c, 1).is_err());
        assert_eq!(count(&c, "notes"), 1);
        assert_eq!(count(&c, "trashed_notes"), 0);
    }
    #[test]
    fn failed_restore_keeps_trash_and_rolls_back_active_copy() {
        let mut c = setup();
        move_note(&mut c, 1).unwrap();
        c.execute_batch("CREATE TRIGGER fail BEFORE DELETE ON trashed_notes BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        assert!(restore(&mut c, 1).is_err());
        assert_eq!(count(&c, "notes"), 0);
        assert_eq!(count(&c, "trashed_notes"), 1);
    }
    #[test]
    fn new_notes_do_not_reuse_trashed_id() {
        let mut c = setup();
        move_note(&mut c, 1).unwrap();
        c.execute(
            "INSERT INTO notes(title,content,modified_at) VALUES('Other','Other',124)",
            [],
        )
        .unwrap();
        assert_ne!(c.last_insert_rowid(), 1);
        restore(&mut c, 1).unwrap();
        assert_eq!(count(&c, "notes"), 2);
    }
    #[test]
    fn trash_survives_reopen() {
        let file = tempfile::NamedTempFile::new().unwrap();
        {
            let mut c = Connection::open(file.path()).unwrap();
            db::initialize_database(&c).unwrap();
            c.execute(
                "INSERT INTO notes VALUES(1,'Title','Text','Personal',0,123)",
                [],
            )
            .unwrap();
            move_note(&mut c, 1).unwrap();
        }
        let mut c = Connection::open(file.path()).unwrap();
        db::initialize_database(&c).unwrap();
        assert_eq!(count(&c, "notes"), 0);
        assert_eq!(restore(&mut c, 1).unwrap().content, "Text");
    }
    #[test]
    fn refresh_skips_trash_then_uses_original_baseline_after_restore() {
        let mut c = Connection::open_in_memory().unwrap();
        db::initialize_database(&c).unwrap();
        let mut remote = crate::notes_api::RemoteNote {
            id: 42,
            title: "Title".into(),
            content: "base".into(),
            category: "Work".into(),
            favorite: false,
            modified: 123,
            etag: Some("tag".into()),
            readonly: Some(false),
        };
        crate::import::store_batch(&mut c, "server", "account", std::slice::from_ref(&remote))
            .unwrap();
        c.execute("UPDATE notes SET content='local edit' WHERE id=1", [])
            .unwrap();
        move_note(&mut c, 1).unwrap();
        remote.content = "server edit".into();
        assert_eq!(
            crate::import::store_batch(&mut c, "server", "account", std::slice::from_ref(&remote))
                .unwrap()
                .skipped,
            1
        );
        assert_eq!(
            crate::refresh::store_refresh(
                &mut c,
                "server",
                "account",
                std::slice::from_ref(&remote)
            )
            .unwrap()
            .locally_deleted,
            1
        );
        assert_eq!(count(&c, "notes"), 0);
        assert_eq!(restore(&mut c, 1).unwrap().content, "local edit");
        crate::refresh::store_refresh(&mut c, "server", "account", &[remote]).unwrap();
        assert_eq!(count(&c, "refresh_conflicts"), 1);
        let content: String = c
            .query_row("SELECT content FROM notes WHERE id=1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(content, "local edit");
    }
}
