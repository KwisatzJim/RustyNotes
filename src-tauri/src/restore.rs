//! Replace local rows transactionally, preserving a verified pre-restore backup.
use crate::backup_preview::{prepare, validate, PreparedBackup, PreviewState};
use rusqlite::{types::Value, Connection, OpenFlags, TransactionBehavior};
use std::path::Path;

const TABLES: &[&str] = &[
    "notes",
    "trashed_notes",
    "refresh_conflicts",
    "conflict_resolutions",
    "imported_notes",
    "note_creation_attempts",
    "creation_recoveries",
    "settings",
    "sqlite_sequence",
];

pub(crate) fn restore_prepared(live: &Path, reviewed: PreparedBackup) -> Result<String, String> {
    // Re-read using the same bounded, read-only validation as preview. Compare
    // bytes, not counts or timestamps: even a same-length edit invalidates consent.
    let fresh = prepare(Path::new(&reviewed.summary.path))?;
    if std::fs::read(fresh.file.path()).map_err(|_| "Could not read backup.")?
        != std::fs::read(reviewed.file.path()).map_err(|_| "Could not read reviewed backup.")?
    {
        return Err("The backup changed since preview. Preview it again before restoring.".into());
    }
    let source = Connection::open_with_flags(fresh.file.path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "Could not open the verified backup.")?;
    source
        .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
        .map_err(|_| "Could not prepare restore.")?;
    validate(&source).map_err(|_| "Backup validation failed. Nothing was restored.")?;
    replace(live, &source)
}

fn replace(live: &Path, source: &Connection) -> Result<String, String> {
    replace_with_backup(live, source, crate::backup::write_backup)
}

fn replace_with_backup(
    live: &Path,
    source: &Connection,
    save: impl FnOnce(&Path, &Path) -> Result<(), String>,
) -> Result<String, String> {
    // No file replacement: the existing SQLite file and schema remain in place,
    // so SQLite coordinates other connections and rolls back interrupted writes.
    let mut db = Connection::open_with_flags(live, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_| "Could not open existing local data. Nothing was restored.")?;
    db.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "Could not prepare restore.")?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "Local data is busy. Close other RustyNotes copies and try again.")?;
    validate(&tx)
        .map_err(|_| "Current local database needs review before restore. Nothing was restored.")?;
    // The write reservation prevents local data changing between this backup
    // (through a second read-only connection) and the replacement transaction.
    let parent = live
        .parent()
        .ok_or("Could not locate the local data folder.")?;
    let folder = tempfile::Builder::new()
        .prefix("before-restore-")
        .tempdir_in(parent)
        .map_err(|_| "Could not create a safety backup folder. Nothing was restored.")?;
    let safety = folder.path().join("RustyNotes-safety.sqlite3");
    if let Err(error) = save(live, &safety) {
        // Publication may succeed before the folder flush fails. Retain that
        // file at the reported path, while still refusing to restore.
        if safety.exists() {
            let _retained = folder.keep();
        }
        return Err(error);
    }
    let _retained = folder.keep();
    #[cfg(unix)]
    for directory in [safety.parent().unwrap(), parent] {
        std::fs::File::open(directory).and_then(|f| f.sync_all())
            .map_err(|_| format!("Could not confirm safety backup durability. Nothing was restored. Backup location: {}", safety.display()))?;
    }
    let result = copy_rows(&tx, source).and_then(|_| tx.commit());
    result.map_err(|_| format!("Restore could not be confirmed. Reload local notes before continuing. Safety backup: {}", safety.display()))?;
    Ok(safety.to_string_lossy().into_owned())
}

fn copy_rows(target: &Connection, source: &Connection) -> rusqlite::Result<()> {
    target.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    for table in TABLES {
        target.execute(&format!("DELETE FROM \"{table}\""), [])?;
        let present: bool = source.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
            [table],
            |r| r.get(0),
        )?;
        if !present && *table == "settings" {
            continue;
        }
        let mut statement = source.prepare(&format!("SELECT * FROM \"{table}\""))?;
        let count = statement.column_count();
        let placeholders = vec!["?"; count].join(",");
        let mut insert =
            target.prepare(&format!("INSERT INTO \"{table}\" VALUES ({placeholders})"))?;
        let rows = statement.query_map([], |r| {
            (0..count)
                .map(|i| r.get::<_, Value>(i))
                .collect::<rusqlite::Result<Vec<_>>>()
        })?;
        for row in rows {
            insert.execute(rusqlite::params_from_iter(row?))?;
        }
    }
    // Check the restored core rows can actually be consumed by the app.
    for table in ["notes", "trashed_notes"] {
        let mut statement = target.prepare(&format!(
            "SELECT id,title,content,category,favorite,modified_at FROM {table}"
        ))?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        for row in rows {
            row?;
        }
    }
    validate(target)
}

#[tauri::command]
pub async fn restore_local_backup(
    token: String,
    confirmed: bool,
    previews: tauri::State<'_, PreviewState>,
    login: tauri::State<'_, crate::auth::LoginState>,
) -> Result<String, String> {
    let reviewed = take_preview(&previews, &token, confirmed)?;
    login.restore_backup(reviewed).await
}

fn take_preview(
    previews: &PreviewState,
    token: &str,
    confirmed: bool,
) -> Result<PreparedBackup, String> {
    if !confirmed {
        return Err("Explicit confirmation is required. Nothing was restored.".into());
    }
    let reviewed = {
        let mut stored = previews
            .0
            .lock()
            .map_err(|_| "Could not read backup preview.")?;
        if stored.as_ref().map(|p| p.summary.token.as_str()) != Some(token) {
            return Err("That preview has expired. Preview the backup again.".into());
        }
        stored.take().ok_or("Preview the backup again.")?
    };
    Ok(reviewed)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(path: &Path, text: &str) {
        let db = Connection::open(path).unwrap();
        crate::db::initialize_database(&db).unwrap();
        db.execute(
            "INSERT INTO notes(title,content,modified_at) VALUES(?1,?1,0)",
            [text],
        )
        .unwrap();
    }
    fn title(path: &Path) -> String {
        Connection::open(path)
            .unwrap()
            .query_row("SELECT title FROM notes", [], |r| r.get(0))
            .unwrap()
    }
    #[test]
    fn confirmation_is_required_and_preview_can_only_be_consumed_once() {
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join("backup.sqlite3");
        fixture(&backup, "old");
        let prepared = prepare(&backup).unwrap();
        let token = prepared.summary.token.clone();
        let state = PreviewState::default();
        *state.0.lock().unwrap() = Some(prepared);
        assert!(take_preview(&state, &token, false).is_err());
        assert!(take_preview(&state, "wrong-token", true).is_err());
        assert!(take_preview(&state, &token, true).is_ok());
        assert!(take_preview(&state, &token, true).is_err());
    }
    #[test]
    fn safety_backup_failure_prevents_any_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live.db");
        let backup = dir.path().join("backup.sqlite3");
        fixture(&live, "current");
        fixture(&backup, "old");
        let source = Connection::open(&backup).unwrap();
        assert!(replace_with_backup(&live, &source, |_, _| Err("disk full".into())).is_err());
        assert_eq!(title(&live), "current");
    }
    #[test]
    fn published_safety_backup_is_retained_when_durability_check_fails() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live.db");
        let backup = dir.path().join("backup.sqlite3");
        fixture(&live, "current");
        fixture(&backup, "old");
        let source = Connection::open(&backup).unwrap();
        let mut saved_path = None;
        assert!(replace_with_backup(&live, &source, |from, to| {
            crate::backup::write_backup(from, to)?;
            saved_path = Some(to.to_path_buf());
            Err("simulated durability failure".into())
        }).is_err());
        assert_eq!(title(&live), "current");
        assert_eq!(title(&saved_path.unwrap()), "current");
    }

    #[test]
    fn wal_restore_preserves_metadata_and_safety_copy_can_undo_it() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live.db");
        let backup = dir.path().join("backup.sqlite3");
        fixture(&live, "current");
        fixture(&backup, "old");
        let current = Connection::open(&live).unwrap();
        current.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        let source = Connection::open(&backup).unwrap();
        source.execute_batch("INSERT INTO trashed_notes VALUES(9,'trash','text','Work',1,12);
            INSERT INTO imported_notes VALUES('https://example.com/','user',88,1,'snapshot');
            INSERT INTO refresh_conflicts VALUES(1,1,'base','local','remote','lk','rk');
            INSERT INTO conflict_resolutions VALUES(1,'keep_local',NULL);
            INSERT INTO note_creation_attempts VALUES(9,'https://example.com/','user','local',NULL,0);
            INSERT INTO creation_recoveries VALUES(9,1,'review');
            CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            INSERT INTO settings VALUES('server_url','https://example.com/');").unwrap();
        drop(source);
        let safety = restore_prepared(&live, prepare(&backup).unwrap()).unwrap();
        let source = Connection::open(&backup).unwrap();
        for table in TABLES {
            let rows = |db: &Connection| {
                let mut statement = db
                    .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                    .unwrap();
                let count = statement.column_count();
                statement
                    .query_map([], |r| {
                        (0..count)
                            .map(|i| r.get::<_, Value>(i))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap()
            };
            assert_eq!(rows(&current), rows(&source), "{table}");
        }
        restore_prepared(&live, prepare(Path::new(&safety)).unwrap()).unwrap();
        assert_eq!(title(&live), "current");
        assert_eq!(
            current
                .query_row("SELECT count(*) FROM settings", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn restores_and_preserves_previous_data_in_safety_copy() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live.db");
        let backup = dir.path().join("backup.sqlite3");
        fixture(&live, "current");
        fixture(&backup, "old");
        let before = std::fs::read(&backup).unwrap();
        let safety = restore_prepared(&live, prepare(&backup).unwrap()).unwrap();
        assert_eq!(title(&live), "old");
        assert_eq!(title(Path::new(&safety)), "current");
        assert_eq!(before, std::fs::read(backup).unwrap());
    }
    #[test]
    fn same_count_changed_backup_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live.db");
        let backup = dir.path().join("backup.sqlite3");
        fixture(&live, "current");
        fixture(&backup, "old");
        let preview = prepare(&backup).unwrap();
        Connection::open(&backup)
            .unwrap()
            .execute("UPDATE notes SET title='new'", [])
            .unwrap();
        assert!(restore_prepared(&live, preview)
            .unwrap_err()
            .contains("changed"));
        assert_eq!(title(&live), "current");
    }
    #[test]
    fn failed_row_copy_rolls_back_and_keeps_safety_backup() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live.db");
        let backup = dir.path().join("backup.sqlite3");
        fixture(&live, "current");
        fixture(&backup, "old");
        Connection::open(&backup)
            .unwrap()
            .execute("UPDATE notes SET title=x'00'", [])
            .unwrap();
        assert!(restore_prepared(&live, prepare(&backup).unwrap())
            .unwrap_err()
            .contains("Safety backup"));
        assert_eq!(title(&live), "current");
        assert!(std::fs::read_dir(dir.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("before-restore-")));
    }
}
