use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub category: String,
    pub favorite: bool,
    pub modified_at: i64,
}

pub(crate) fn database_path() -> PathBuf {
    // macOS keeps the existing ~/Library/Application Support location.
    // Linux uses XDG_DATA_HOME, falling back to ~/.local/share.
    let data_dir = dirs::data_local_dir().expect("Could not determine application data directory");
    database_path_in(&data_dir)
}

fn database_path_in(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("RustyNotes").join("rustynotes.db")
}

pub fn open_database() -> Result<Connection> {
    let path = database_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .expect("Could not create RustyNotes application directory");
    }

    let connection = Connection::open(path)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;

    initialize_database(&connection)?;

    Ok(connection)
}

pub(crate) fn initialize_database(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS notes (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            content     TEXT NOT NULL,
            category    TEXT NOT NULL DEFAULT 'Personal',
            favorite    INTEGER NOT NULL DEFAULT 0,
            modified_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS refresh_conflicts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            local_id INTEGER NOT NULL,
            base_snapshot TEXT NOT NULL,
            local_snapshot TEXT NOT NULL,
            server_snapshot TEXT NOT NULL,
            local_key TEXT NOT NULL,
            server_key TEXT NOT NULL,
            UNIQUE(local_id, local_key, server_key)
        );
        CREATE TABLE IF NOT EXISTS trashed_notes (
            id INTEGER PRIMARY KEY,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            category TEXT NOT NULL,
            favorite INTEGER NOT NULL,
            modified_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS conflict_resolutions (
            conflict_id INTEGER PRIMARY KEY,
            action TEXT NOT NULL,
            copy_id INTEGER
        );
        CREATE TABLE IF NOT EXISTS imported_notes (
            server TEXT NOT NULL,
            account TEXT NOT NULL,
            remote_id INTEGER NOT NULL,
            local_id INTEGER NOT NULL UNIQUE,
            original_snapshot TEXT NOT NULL,
            PRIMARY KEY (server, account, remote_id)
        );
        CREATE TABLE IF NOT EXISTS note_creation_attempts (
            local_id INTEGER PRIMARY KEY,
            server TEXT NOT NULL,
            account TEXT NOT NULL,
            local_snapshot TEXT NOT NULL,
            server_snapshot TEXT,
            completed INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS creation_recoveries (
            local_id INTEGER PRIMARY KEY,
            copy_id INTEGER NOT NULL,
            review_snapshot TEXT NOT NULL
        );
        "#,
    )?;

    Ok(())
}

pub fn get_notes() -> Result<Vec<Note>> {
    let connection = open_database()?;

    let mut statement = connection.prepare(
        r#"
        SELECT id, title, content, category, favorite, modified_at
        FROM notes
        ORDER BY modified_at DESC
        "#,
    )?;

    let notes = statement
        .query_map([], |row| {
            Ok(Note {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                category: row.get(3)?,
                favorite: row.get::<_, i64>(4)? != 0,
                modified_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(notes)
}

pub fn create_note(
    title: &str,
    content: &str,
    category: &str,
) -> Result<Note> {
    let connection = open_database()?;

    let modified_at = chrono::Utc::now().timestamp();

    connection.execute(
        r#"
        INSERT INTO notes
            (title, content, category, favorite, modified_at)
        VALUES
            (?1, ?2, ?3, 0, ?4)
        "#,
        (title, content, category, modified_at),
    )?;

    let id = connection.last_insert_rowid();

    Ok(Note {
        id,
        title: title.to_string(),
        content: content.to_string(),
        category: category.to_string(),
        favorite: false,
        modified_at,
    })
}

pub fn update_note(
    id: i64,
    title: &str,
    content: &str,
    category: &str,
    favorite: bool,
) -> Result<Note> {
    let connection = open_database()?;

    let modified_at = chrono::Utc::now().timestamp();

    connection.execute(
        r#"
        UPDATE notes
        SET
            title = ?1,
            content = ?2,
            category = ?3,
            favorite = ?4,
            modified_at = ?5
        WHERE id = ?6
        "#,
        (
            title,
            content,
            category,
            if favorite { 1 } else { 0 },
            modified_at,
            id,
        ),
    )?;

    Ok(Note {
        id,
        title: title.to_string(),
        content: content.to_string(),
        category: category.to_string(),
        favorite,
        modified_at,
    })
}

pub fn delete_note(id: i64) -> Result<()> {
    crate::trash::move_note(&mut open_database()?, id)
}

#[cfg(test)]
mod path_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn linux_default_data_directory_keeps_database_under_app_folder() {
        assert_eq!(
            database_path_in(Path::new("/home/test/.local/share")),
            PathBuf::from("/home/test/.local/share/RustyNotes/rustynotes.db")
        );
    }

    #[test]
    fn custom_data_directory_is_preserved_including_spaces() {
        assert_eq!(
            database_path_in(Path::new("/mnt/user data")),
            PathBuf::from("/mnt/user data/RustyNotes/rustynotes.db")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn actual_macos_path_is_identical_to_original_location() {
        let original = dirs::home_dir().unwrap()
            .join("Library").join("Application Support")
            .join("RustyNotes").join("rustynotes.db");
        assert_eq!(database_path(), original);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_uses_absolute_xdg_override_or_home_fallback() {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from).filter(|path| path.is_absolute())
            .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/share"));
        assert_eq!(database_path(), base.join("RustyNotes/rustynotes.db"));
    }
}
