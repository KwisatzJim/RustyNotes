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
const MAX_NOTE_ROWS: i64 = 50_000;
const MAX_ROWS_PER_TABLE: i64 = 100_000;
const MAX_TEXT_VALUE_BYTES: i64 = 8 * 1024 * 1024;
const MAX_TOTAL_TEXT_BYTES: i64 = 96 * 1024 * 1024;

const TEXT_COLUMNS: &[(&str, &[&str])] = &[
    ("notes", &["title", "content", "category"]),
    ("trashed_notes", &["title", "content", "category"]),
    (
        "refresh_conflicts",
        &[
            "base_snapshot",
            "local_snapshot",
            "server_snapshot",
            "local_key",
            "server_key",
        ],
    ),
    ("conflict_resolutions", &["action"]),
    (
        "imported_notes",
        &["server", "account", "original_snapshot"],
    ),
    (
        "note_creation_attempts",
        &["server", "account", "local_snapshot", "server_snapshot"],
    ),
    ("creation_recoveries", &["review_snapshot"]),
    ("settings", &["key", "value"]),
];

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
    validate_restore_limits(&db)?;
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

fn validate_restore_limits(db: &Connection) -> Result<(), String> {
    validate_restore_limits_with(
        db,
        MAX_NOTE_ROWS,
        MAX_ROWS_PER_TABLE,
        MAX_TEXT_VALUE_BYTES,
        MAX_TOTAL_TEXT_BYTES,
    )
}

fn validate_restore_limits_with(
    db: &Connection,
    max_note_rows: i64,
    max_rows_per_table: i64,
    max_text_value_bytes: i64,
    max_total_text_bytes: i64,
) -> Result<(), String> {
    let mut total = 0_i64;
    for (table, fields) in TEXT_COLUMNS {
        let present: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .map_err(|_| "Could not check backup size limits.")?;
        if !present {
            continue;
        }
        let rows: i64 = db
            .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |row| {
                row.get(0)
            })
            .map_err(|_| "Could not check backup row limits.")?;
        let row_limit = if *table == "notes" || *table == "trashed_notes" {
            max_note_rows
        } else {
            max_rows_per_table
        };
        if rows > row_limit {
            return Err("This backup contains too many records to restore safely.".into());
        }
        for field in *fields {
            let (largest, subtotal): (i64, i64) = db
                .query_row(
                    &format!("SELECT coalesce(max(length(\"{field}\")),0), coalesce(sum(length(\"{field}\")),0) FROM \"{table}\""),
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| "Could not check backup text limits.")?;
            if largest > max_text_value_bytes {
                return Err("This backup contains an individual text value larger than the 8 MiB safety limit.".into());
            }
            total = total
                .checked_add(subtotal)
                .ok_or("This backup's text size is too large to restore safely.")?;
            if total > max_total_text_bytes {
                return Err(
                    "This backup contains more than 96 MiB of text and cannot be restored safely."
                        .into(),
                );
            }
        }
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
    #[test]
    fn rejects_oversized_text_values_before_restore() {
        let (_dir, path) = fixture();
        let db = Connection::open(&path).unwrap();
        db.execute(
            "UPDATE notes SET content=?1",
            ["x".repeat(MAX_TEXT_VALUE_BYTES as usize + 1)],
        )
        .unwrap();
        drop(db);
        assert!(inspect(&path).unwrap_err().contains("8 MiB"));
    }

    #[test]
    fn rejects_excessive_note_rows_before_restore() {
        let (_dir, path) = fixture();
        let db = Connection::open(&path).unwrap();
        db.execute_batch(&format!(
            "WITH RECURSIVE n(x) AS (VALUES(2) UNION ALL SELECT x+1 FROM n WHERE x<{}) INSERT INTO notes(id,title,content,category,favorite,modified_at) SELECT x,'','','',0,0 FROM n;",
            MAX_NOTE_ROWS + 1
        )).unwrap();
        drop(db);
        assert!(inspect(&path).unwrap_err().contains("too many records"));
    }

    #[test]
    fn rejects_excessive_aggregate_text_even_when_each_value_is_small() {
        let (_dir, path) = fixture();
        let db = Connection::open(&path).unwrap();
        db.execute("UPDATE notes SET title='123456', content='abcdef'", [])
            .unwrap();
        assert!(validate_restore_limits_with(&db, 10, 10, 8, 10)
            .unwrap_err()
            .contains("96 MiB"));
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
