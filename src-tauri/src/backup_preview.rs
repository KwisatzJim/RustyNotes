//! Read-only preview, never opens the application's live database.
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
use tauri_plugin_dialog::DialogExt;

const MAX_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct BackupPreview {
    pub(crate) path: String,
    pub(crate) token: String,
    notes: i64,
    trashed_notes: i64,
}

pub(crate) struct PreparedBackup {
    pub file: tempfile::NamedTempFile,
    _folder: tempfile::TempDir,
    pub summary: BackupPreview,
}

#[derive(Default)]
pub struct PreviewState(pub(crate) std::sync::Arc<std::sync::Mutex<Option<PreparedBackup>>>);

fn columns(db: &Connection, table: &str) -> rusqlite::Result<Vec<(String, String, i64, i64)>> {
    let mut statement =
        db.prepare("SELECT name,type,\"notnull\",pk FROM pragma_table_info(?1) ORDER BY cid")?;
    let rows = statement.query_map([table], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })?;
    rows.collect()
}

#[cfg(test)]
fn inspect(path: &Path) -> Result<BackupPreview, String> {
    prepare(path).map(|p| p.summary)
}

pub(crate) fn prepare(path: &Path) -> Result<PreparedBackup, String> {
    if path.extension().and_then(|s| s.to_str()) != Some("sqlite3") {
        return Err("Choose a RustyNotes backup ending in .sqlite3. Nothing was restored.".into());
    }
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "Could not read the backup file.")?;
    if !metadata.file_type().is_file() {
        return Err("Choose a regular file, not a folder or symbolic link.".into());
    }
    if metadata.len() > MAX_BYTES {
        return Err("This preview supports backups up to 128 MiB.".into());
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        match std::fs::symlink_metadata(Path::new(&sidecar)) {
            Ok(_) => return Err("This looks like a live database with companion files. Choose a standalone backup made by RustyNotes.".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err("Could not check the backup's companion files.".into()),
        }
    }
    let input = File::open(path).map_err(|_| "Could not open the backup file.")?;
    if !input
        .metadata()
        .map_err(|_| "Could not check the backup file.")?
        .is_file()
    {
        return Err("Choose a regular backup file.".into());
    }
    // Work on a bounded private copy so SQLite cannot create sidecars beside
    // the selected file. Keep this private snapshot for a single confirmation.
    let folder = tempfile::tempdir().map_err(|_| "Could not create a private preview folder.")?;
    let mut copy = tempfile::NamedTempFile::new_in(folder.path())
        .map_err(|_| "Could not create a preview copy.")?;
    let bytes = std::io::copy(&mut input.take(MAX_BYTES + 1), &mut copy)
        .map_err(|_| "Could not read the complete backup.")?;
    if bytes > MAX_BYTES {
        return Err("This preview supports backups up to 128 MiB.".into());
    }
    copy.flush()
        .map_err(|_| "Could not finish the preview copy.")?;
    let db = Connection::open_with_flags(copy.path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "This file could not be opened as a SQLite backup.")?;
    db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
        .map_err(|_| "Could not prepare a read-only preview.")?;
    validate(&db).map_err(|_| {
        "This is damaged or is not a compatible RustyNotes backup. Nothing was restored."
            .to_string()
    })?;
    let notes = db
        .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .map_err(|_| "Could not count backed-up notes.")?;
    let trashed_notes = db
        .query_row("SELECT count(*) FROM trashed_notes", [], |r| r.get(0))
        .map_err(|_| "Could not count backed-up Trash.")?;
    drop(db);
    let summary = BackupPreview {
        path: path.to_string_lossy().into_owned(),
        token: format!(
            "{}-{}",
            folder.path().file_name().unwrap().to_string_lossy(),
            copy.path().file_name().unwrap().to_string_lossy()
        ),
        notes,
        trashed_notes,
    };
    Ok(PreparedBackup {
        file: copy,
        _folder: folder,
        summary,
    })
}

pub(crate) fn validate(db: &Connection) -> rusqlite::Result<()> {
    let expected = Connection::open_in_memory()?;
    crate::db::initialize_database(&expected)?;
    expected.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
    let required = [
        "notes",
        "trashed_notes",
        "refresh_conflicts",
        "conflict_resolutions",
        "imported_notes",
        "note_creation_attempts",
        "creation_recoveries",
    ];
    // Reject executable schema objects and unknown/virtual tables before reading
    // rows. Structural compatibility is not a promise that restore will succeed.
    let mut statement = db.prepare("SELECT type,name,coalesce(sql,'') FROM sqlite_schema")?;
    let objects = statement.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for object in objects {
        let (kind, name, sql) = object?;
        let allowed = match kind.as_str() {
            "table" => {
                (required.contains(&name.as_str())
                    || name == "settings"
                    || name == "sqlite_sequence")
                    && !sql.to_ascii_uppercase().contains("VIRTUAL")
            }
            "index" => name.starts_with("sqlite_autoindex_") && sql.is_empty(),
            _ => false,
        };
        if !allowed {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    for table in required {
        if columns(db, table)? != columns(&expected, table)? {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    let settings = columns(db, "settings")?;
    if !settings.is_empty() && settings != columns(&expected, "settings")? {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let check: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

#[tauri::command]
pub async fn preview_local_backup(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, PreviewState>,
) -> Result<Option<BackupPreview>, String> {
    let stored = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        *stored.lock().map_err(|_| "Could not reset preview.")? = None;
        let chosen = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Preview a RustyNotes backup (read only)")
            .add_filter("RustyNotes SQLite backup", &["sqlite3"])
            .blocking_pick_file();
        let Some(chosen) = chosen else {
            return Ok(None);
        };
        let path = chosen
            .into_path()
            .map_err(|_| "Choose a local backup file.")?;
        let prepared = prepare(&path)?;
        let summary = prepared.summary.clone();
        *stored.lock().map_err(|_| "Could not retain preview.")? = Some(prepared);
        Ok(Some(summary))
    })
    .await
    .map_err(|_| "Backup preview failed. Nothing was restored.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backup.sqlite3");
        let db = Connection::open(&path).unwrap();
        crate::db::initialize_database(&db).unwrap();
        db.execute_batch("INSERT INTO notes VALUES(1,'title','secret text','Personal',0,0); INSERT INTO trashed_notes VALUES(2,'trash','private','Work',1,0);").unwrap();
        (dir, path)
    }
    #[test]
    fn previews_a_backup_created_by_rustynotes_from_wal_database() {
        let (_source_dir, source) = fixture();
        let db = Connection::open(&source).unwrap();
        db.execute_batch(
            "PRAGMA journal_mode=WAL; INSERT INTO notes VALUES(3,'latest','latest','Work',1,1);",
        )
        .unwrap();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("saved.sqlite3");
        crate::backup::write_backup(&source, &output).unwrap();
        let before = std::fs::read(&output).unwrap();
        let preview = inspect(&output).unwrap();
        assert_eq!((preview.notes, preview.trashed_notes), (2, 1));
        assert_eq!(before, std::fs::read(&output).unwrap());
        assert_eq!(std::fs::read_dir(output_dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn counts_without_modifying_source_or_leaving_sidecars() {
        let (dir, path) = fixture();
        let before = std::fs::read(&path).unwrap();
        let result = inspect(&path).unwrap();
        assert_eq!((result.notes, result.trashed_notes), (1, 1));
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("secret text"));
        assert_eq!(before, std::fs::read(&path).unwrap());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn accepts_empty_database_and_optional_settings() {
        let (_dir, path) = fixture();
        let db = Connection::open(&path).unwrap();
        db.execute_batch("DELETE FROM notes; DELETE FROM trashed_notes; CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);").unwrap();
        drop(db);
        let result = inspect(&path).unwrap();
        assert_eq!((result.notes, result.trashed_notes), (0, 0));
    }
    #[test]
    fn rejects_corrupt_unrelated_and_incomplete_databases() {
        let (_dir, path) = fixture();
        let db = Connection::open(&path).unwrap();
        db.execute_batch("DROP TABLE imported_notes;").unwrap();
        drop(db);
        assert!(inspect(&path).is_err());
        std::fs::write(&path, "not sqlite").unwrap();
        assert!(inspect(&path).is_err());
    }
    #[test]
    fn rejects_executable_or_unknown_schema() {
        for sql in [
            "CREATE VIEW unexpected AS SELECT * FROM notes;",
            "CREATE TRIGGER unexpected AFTER DELETE ON notes BEGIN DELETE FROM trashed_notes; END;",
            "ALTER TABLE notes ADD COLUMN unexpected TEXT;",
            "CREATE TABLE unknown(x TEXT);",
        ] {
            let (_dir, path) = fixture();
            let db = Connection::open(&path).unwrap();
            db.execute_batch(sql).unwrap();
            drop(db);
            assert!(inspect(&path).is_err());
        }
    }
    #[test]
    fn rejects_live_companions_and_oversized_files() {
        let (_dir, path) = fixture();
        std::fs::write(format!("{}-wal", path.display()), "companion").unwrap();
        assert!(inspect(&path).is_err());
        let (dir, _) = fixture();
        let huge = dir.path().join("huge.sqlite3");
        File::create(&huge).unwrap().set_len(MAX_BYTES + 1).unwrap();
        assert!(inspect(&huge).unwrap_err().contains("128 MiB"));
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlinks() {
        let (dir, path) = fixture();
        let link = dir.path().join("link.sqlite3");
        std::os::unix::fs::symlink(path, &link).unwrap();
        assert!(inspect(&link).is_err());
    }
}
