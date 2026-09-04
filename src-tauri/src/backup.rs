//! Consistent local SQLite backup. No network or credential-store access.
use rusqlite::{
    backup::{Backup, StepResult},
    Connection, OpenFlags,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

fn checked_data_folder(database: &Path) -> Result<std::path::PathBuf, String> {
    let folder = database
        .parent()
        .ok_or("Could not locate the RustyNotes data folder.")?;
    if !folder.is_dir() {
        return Err("The RustyNotes data folder does not exist yet. Create a note first.".into());
    }
    Ok(folder.to_path_buf())
}

#[tauri::command]
pub async fn open_local_data_folder(app: tauri::AppHandle) -> Result<(), String> {
    // No frontend-supplied path: only the app's existing data folder is opened.
    tauri::async_runtime::spawn_blocking(move || {
        let folder = checked_data_folder(&crate::db::database_path())?;
        let path = folder
            .to_str()
            .ok_or("The data folder path cannot be opened by the file manager.")?;
        app.opener().open_path(path, None::<&str>).map_err(|_| {
            "Could not open the RustyNotes data folder in your file manager.".to_string()
        })
    })
    .await
    .map_err(|_| "Could not open the data folder.".to_string())?
}

pub(super) fn write_backup(source_path: &Path, destination: &Path) -> Result<(), String> {
    if destination.extension().and_then(|v| v.to_str()) != Some("sqlite3") {
        return Err("Choose a new filename ending in .sqlite3.".into());
    }
    // Do not replace anything, including dangling symlinks or another backup.
    match std::fs::symlink_metadata(destination) {
        Ok(_) => {
            return Err("That destination already exists. Choose a new backup filename.".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Could not check the backup destination.".into()),
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("Choose a full destination path.")?;
    let parent =
        std::fs::canonicalize(parent).map_err(|_| "Could not open the destination folder.")?;
    let source_path =
        std::fs::canonicalize(source_path).map_err(|_| "Could not locate the local database.")?;
    if source_path.parent() == Some(parent.as_path()) {
        return Err("Choose a backup folder outside RustyNotes’ live data folder.".into());
    }
    let destination = parent.join(destination.file_name().ok_or("Choose a filename.")?);
    let source = Connection::open_with_flags(&source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "Could not read the local database for backup.")?;
    source
        .busy_timeout(Duration::from_secs(5))
        .map_err(|_| "Could not prepare backup.")?;
    // Pin a consistent snapshot, including committed WAL contents.
    let transaction = source
        .unchecked_transaction()
        .map_err(|_| "Could not start backup snapshot.")?;
    transaction
        .query_row("SELECT count(*) FROM sqlite_schema", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(|_| "Could not read backup snapshot.")?;
    let temp = tempfile::NamedTempFile::new_in(&parent)
        .map_err(|_| "Could not create a backup in that folder. Check permissions and space.")?;
    {
        let mut target =
            Connection::open(temp.path()).map_err(|_| "Could not open the backup file.")?;
        {
            let backup = Backup::new(&transaction, &mut target)
                .map_err(|_| "Could not prepare the database backup.")?;
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if Instant::now() >= deadline {
                    return Err("Backup timed out. No completed backup was saved.".into());
                }
                match backup
                    .step(256)
                    .map_err(|_| "Database backup failed. Check available space.")?
                {
                    StepResult::Done => break,
                    StepResult::More => {}
                    StepResult::Busy | StepResult::Locked => {
                        std::thread::sleep(Duration::from_millis(25))
                    }
                    _ => {
                        return Err(
                            "Unexpected backup result. No completed backup was saved.".into()
                        )
                    }
                }
            }
        }
        let check: String = target
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(|_| "Could not verify the backup database.")?;
        if check != "ok" {
            return Err("Backup verification failed. No completed backup was saved.".into());
        }
        target
            .close()
            .map_err(|_| "Could not finish the backup database.")?;
    }
    temp.as_file()
        .sync_all()
        .map_err(|_| "Could not finish writing the backup.")?;
    temp.persist_noclobber(destination).map_err(|_| {
        "Could not save backup. Choose a new filename and check permissions and space."
    })?;
    Ok(())
}

#[tauri::command]
pub async fn backup_local_data(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let chosen = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Back up RustyNotes local data")
            .add_filter("SQLite backup", &["sqlite3"])
            .set_file_name(format!(
                "RustyNotes-backup-{}.sqlite3",
                chrono::Local::now().format("%Y-%m-%d-%H%M%S")
            ))
            .blocking_save_file();
        let Some(chosen) = chosen else {
            return Ok(None);
        };
        let path = chosen
            .into_path()
            .map_err(|_| "Choose a local file destination.")?;
        write_backup(&crate::db::database_path(), &path)?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|_| {
        "Backup could not be confirmed. Check the chosen folder before retrying.".to_string()
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_shortcut_uses_database_parent_without_creating_missing_folders() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            checked_data_folder(&dir.path().join("rustynotes.db")).unwrap(),
            dir.path()
        );
        let missing = dir.path().join("missing");
        assert!(checked_data_folder(&missing.join("rustynotes.db")).is_err());
        assert!(!missing.exists());
    }
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("live")).unwrap();
        let path = dir.path().join("live/rustynotes.db");
        let db = Connection::open(&path).unwrap();
        crate::db::initialize_database(&db).unwrap();
        (dir, path, db)
    }
    #[test]
    fn backup_includes_committed_wal_and_all_tables_without_changing_source() {
        let (dir, source, db) = fixture();
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
            INSERT INTO notes(title,content,category,favorite,modified_at) VALUES('日本語','**latest**','Work',1,42);
            INSERT INTO trashed_notes VALUES(9,'Trash','kept','Personal',0,1);
            INSERT INTO refresh_conflicts VALUES(1,1,'base','local','server','local-key','server-key');
            INSERT INTO conflict_resolutions VALUES(1,'keep_local',NULL);
            INSERT INTO imported_notes VALUES('https://example.com/','test-user',12,1,'snapshot');
            INSERT INTO note_creation_attempts VALUES(9,'https://example.com/','test-user','local',NULL,0);
            INSERT INTO creation_recoveries VALUES(9,1,'review');
            CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT);
            INSERT INTO settings VALUES('server_url','https://example.com/');
            CREATE TABLE future_data(value TEXT); INSERT INTO future_data VALUES('preserved');").unwrap();
        let output = dir.path().join("backup.sqlite3");
        write_backup(&source, &output).unwrap();
        let copy = Connection::open(&output).unwrap();
        let tables = |c: &Connection| -> Vec<String> {
            c.prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(tables(&db), tables(&copy));
        for table in tables(&db) {
            let rows = |connection: &Connection| {
                let mut statement = connection
                    .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid"))
                    .unwrap();
                let columns = statement.column_count();
                statement
                    .query_map([], |row| {
                        (0..columns)
                            .map(|i| row.get::<_, rusqlite::types::Value>(i))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap()
            };
            assert_eq!(rows(&db), rows(&copy), "table {table}");
        }
        assert_eq!(
            copy.query_row("SELECT content FROM notes", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "**latest**"
        );
        assert_eq!(
            copy.query_row("SELECT value FROM future_data", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "preserved"
        );
        db.execute("UPDATE notes SET content='later'", []).unwrap();
        assert_eq!(
            copy.query_row("SELECT content FROM notes", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "**latest**"
        );
        assert_eq!(
            copy.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }
    #[test]
    fn existing_files_bad_extensions_and_live_folder_are_rejected() {
        let (dir, source, _db) = fixture();
        let output = dir.path().join("existing.sqlite3");
        std::fs::write(&output, "keep").unwrap();
        assert!(write_backup(&source, &output).is_err());
        assert_eq!(std::fs::read_to_string(output).unwrap(), "keep");
        for path in [
            source.clone(),
            dir.path().join("live/new.sqlite3"),
            dir.path().join("wrong.md"),
            dir.path().join("missing/backup.sqlite3"),
        ] {
            assert!(write_backup(&source, &path).is_err());
        }
    }
    #[test]
    fn corrupt_source_leaves_no_output() {
        let (dir, source, db) = fixture();
        drop(db);
        std::fs::write(&source, "not sqlite").unwrap();
        let output = dir.path().join("backup.sqlite3");
        assert!(write_backup(&source, &output).is_err());
        assert!(!output.exists());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_destination_is_rejected() {
        let (dir, source, _db) = fixture();
        let output = dir.path().join("link.sqlite3");
        std::os::unix::fs::symlink(dir.path().join("absent"), &output).unwrap();
        assert!(write_backup(&source, &output).is_err());
        assert!(!dir.path().join("absent").exists());
    }
}
