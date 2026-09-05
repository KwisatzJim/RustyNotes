//! Insert-only import. Existing copies and deletion markers are never replaced.
use crate::notes_api::RemoteNote;
use rusqlite::{Connection, TransactionBehavior};
use serde::Serialize;

#[derive(Serialize)]
pub struct ImportSummary {
    pub added: usize,
    pub skipped: usize,
}

pub fn store_batch(
    connection: &mut Connection,
    server: &str,
    account: &str,
    notes: &[RemoteNote],
) -> Result<ImportSummary, String> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "Could not start local import. Your notes were not changed.".to_string())?;
    // No FK cascade: retaining the association after local deletion prevents
    // subsequent imports from unexpectedly resurrecting that deleted copy.
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS imported_notes (
            server TEXT NOT NULL,
            account TEXT NOT NULL,
            remote_id INTEGER NOT NULL,
            local_id INTEGER NOT NULL UNIQUE,
            original_snapshot TEXT NOT NULL,
            PRIMARY KEY (server, account, remote_id)
        );",
        )
        .map_err(|_| "Could not prepare local import storage.".to_string())?;
    let mut summary = ImportSummary {
        added: 0,
        skipped: 0,
    };
    let mut seen = std::collections::HashSet::new();
    for note in notes {
        if note.id <= 0 || note.modified < 0 || !seen.insert(note.id) {
            return Err("Invalid or duplicate server note ID. The import was rolled back.".into());
        }
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM imported_notes WHERE server = ?1 AND account = ?2 AND remote_id = ?3)",
            (server, account, note.id), |row| row.get(0),
        ).map_err(|_| "Could not check previously imported notes.".to_string())?;
        if exists {
            summary.skipped += 1;
            continue;
        }
        transaction.execute(
            "INSERT INTO notes (title, content, category, favorite, modified_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            (&note.title, &note.content, &note.category, note.favorite, note.modified),
        ).map_err(|_| "Could not store imported notes. The import was rolled back.".to_string())?;
        let local_id = transaction.last_insert_rowid();
        let snapshot = serde_json::to_string(note)
            .map_err(|_| "Could not preserve the server snapshot.".to_string())?;
        transaction.execute(
            "INSERT INTO imported_notes (server, account, remote_id, local_id, original_snapshot) VALUES (?1, ?2, ?3, ?4, ?5)",
            (server, account, note.id, local_id, snapshot),
        ).map_err(|_| "Could not track imported notes. The import was rolled back.".to_string())?;
        summary.added += 1;
    }
    transaction
        .commit()
        .map_err(|_| "Could not commit the import. Please retry.".to_string())?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        crate::db::initialize_database(&connection).unwrap();
        connection
    }
    fn note(id: i64) -> RemoteNote {
        RemoteNote {
            id,
            title: "Title".into(),
            content: "# Markdown\n\n**hello** 📝".into(),
            category: "Work/Project".into(),
            favorite: true,
            modified: 123,
            etag: Some("original-etag".into()),
            readonly: Some(true),
        }
    }
    fn count(connection: &Connection) -> i64 {
        connection
            .query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))
            .unwrap()
    }
    #[test]
    fn imports_fields_and_keeps_original_server_metadata() {
        let mut connection = database();
        let summary = store_batch(&mut connection, "https://cloud/", "jim", &[note(434)]).unwrap();
        assert_eq!(summary.added, 1);
        let fields: (String, String, String, bool, i64) = connection
            .query_row(
                "SELECT title, content, category, favorite, modified_at FROM notes",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            fields,
            (
                note(434).title,
                note(434).content,
                note(434).category,
                true,
                123
            )
        );
        let snapshot: String = connection
            .query_row("SELECT original_snapshot FROM imported_notes", [], |r| {
                r.get(0)
            })
            .unwrap();
        let original: RemoteNote = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(original.etag.as_deref(), Some("original-etag"));
        assert_eq!(original.readonly, Some(true));
    }
    #[test]
    fn repeated_import_preserves_local_edits_and_does_not_duplicate() {
        let mut connection = database();
        store_batch(&mut connection, "https://cloud/", "jim", &[note(1)]).unwrap();
        connection
            .execute("UPDATE notes SET content = 'local edit'", [])
            .unwrap();
        let summary = store_batch(&mut connection, "https://cloud/", "jim", &[note(1)]).unwrap();
        assert_eq!((summary.added, summary.skipped), (0, 1));
        let content: String = connection
            .query_row("SELECT content FROM notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(content, "local edit");
        assert_eq!(count(&connection), 1);
    }
    #[test]
    fn unrelated_local_notes_and_account_namespaces_are_preserved() {
        let mut connection = database();
        connection
            .execute(
                "INSERT INTO notes (title, content, modified_at) VALUES ('Title', 'local only', 1)",
                [],
            )
            .unwrap();
        for (server, account) in [
            ("https://one/", "jim"),
            ("https://one/", "other"),
            ("https://two/", "jim"),
        ] {
            store_batch(&mut connection, server, account, &[note(1)]).unwrap();
        }
        assert_eq!(count(&connection), 4);
        let original: String = connection
            .query_row("SELECT content FROM notes WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(original, "local only");
    }
    #[test]
    fn local_deletion_is_not_undone_by_reimport() {
        let mut connection = database();
        store_batch(&mut connection, "https://cloud/", "jim", &[note(1)]).unwrap();
        connection.execute("DELETE FROM notes", []).unwrap();
        let summary = store_batch(&mut connection, "https://cloud/", "jim", &[note(1)]).unwrap();
        assert_eq!(summary.skipped, 1);
        assert_eq!(count(&connection), 0);
    }
    #[test]
    fn failure_rolls_back_every_insert() {
        let mut connection = database();
        connection.execute_batch("CREATE TRIGGER fail_second BEFORE INSERT ON notes WHEN NEW.title = 'fail' BEGIN SELECT RAISE(ABORT, 'simulated disk failure'); END;").unwrap();
        let mut bad = note(2);
        bad.title = "fail".into();
        assert!(store_batch(&mut connection, "https://cloud/", "jim", &[note(1), bad]).is_err());
        assert_eq!(count(&connection), 0);
        assert_eq!(
            store_batch(&mut connection, "https://cloud/", "jim", &[note(1)])
                .unwrap()
                .added,
            1
        );
    }
    #[test]
    fn imports_434_notes_and_repeat_adds_none() {
        let mut connection = database();
        let notes: Vec<_> = (1..=434).map(note).collect();
        let first = store_batch(&mut connection, "https://cloud/", "jim", &notes).unwrap();
        assert_eq!((first.added, first.skipped), (434, 0));
        let second = store_batch(&mut connection, "https://cloud/", "jim", &notes).unwrap();
        assert_eq!((second.added, second.skipped), (0, 434));
        assert_eq!(count(&connection), 434);
    }
    #[test]
    fn empty_batch_is_safe_and_duplicates_rollback() {
        let mut connection = database();
        assert_eq!(
            store_batch(&mut connection, "https://cloud/", "jim", &[])
                .unwrap()
                .added,
            0
        );
        assert!(store_batch(
            &mut connection,
            "https://cloud/",
            "jim",
            &[note(1), note(1)]
        )
        .is_err());
        assert_eq!(count(&connection), 0);
    }
}
