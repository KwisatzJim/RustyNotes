//! Single-file, insert-only local Markdown import. No network or source writes.
use crate::db::Note;
use rusqlite::Connection;
use std::{fs::File, io::Read, path::Path};
use tauri_plugin_dialog::DialogExt;

const MAX_BYTES: u64 = 4 * 1024 * 1024;

fn read_markdown(path: &Path) -> Result<(String, String), String> {
    if !path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("md"))
    {
        return Err("Choose a Markdown file ending in .md. Nothing was imported.".into());
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "Could not read the selected file. Nothing was imported.".to_string())?;
    if !metadata.file_type().is_file() {
        return Err("Choose a regular Markdown file, not a folder or symbolic link.".into());
    }
    if metadata.len() > MAX_BYTES {
        return Err("The file exceeds the 4 MiB import limit. Nothing was imported.".into());
    }
    let file = File::open(path)
        .map_err(|_| "Could not open the selected file. Nothing was imported.".to_string())?;
    if !file
        .metadata()
        .map_err(|_| "Could not check the selected file.".to_string())?
        .is_file()
    {
        return Err("Choose a regular Markdown file.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not finish reading the file. Nothing was imported.".to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The file exceeds the 4 MiB import limit. Nothing was imported.".into());
    }
    let content = String::from_utf8(bytes).map_err(|_| "This file is not UTF-8 text. Save it as UTF-8 Markdown and try again. Nothing was imported.".to_string())?;
    if content.contains('\0') {
        return Err("The file contains binary data. Nothing was imported.".into());
    }
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Untitled")
        .to_owned();
    Ok((title, content))
}

fn store(db: &mut Connection, title: String, content: String) -> Result<Note, String> {
    let tx = db
        .transaction()
        .map_err(|_| "Could not start local import.".to_string())?;
    let modified_at = chrono::Utc::now().timestamp();
    tx.execute("INSERT INTO notes(title,content,category,favorite,modified_at) VALUES (?1,?2,'Personal',0,?3)", (&title,&content,modified_at))
        .map_err(|_| "Could not save the imported note. Existing notes were not changed.".to_string())?;
    let note = Note {
        id: tx.last_insert_rowid(),
        title,
        content,
        category: "Personal".into(),
        favorite: false,
        modified_at,
    };
    tx.commit().map_err(|_| {
        "Import could not be confirmed. Check your local notes before importing again.".to_string()
    })?;
    Ok(note)
}

#[tauri::command]
pub async fn import_markdown(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Option<Note>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let chosen = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Import one Markdown file")
            .add_filter("Markdown", &["md"])
            .blocking_pick_file();
        let Some(chosen) = chosen else {
            return Ok(None);
        };
        let path = chosen
            .into_path()
            .map_err(|_| "Choose a local Markdown file.".to_string())?;
        let (title, content) = read_markdown(&path)?;
        let mut db = crate::db::open_database()
            .map_err(|_| "Could not open local storage. Nothing was imported.".to_string())?;
        store(&mut db, title, content).map(Some)
    })
    .await
    .map_err(|_| {
        "Import could not be confirmed. Check your local notes before importing again.".to_string()
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_markdown_unicode_line_endings_and_source_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("旅行.MD");
        let content = "\u{feff}# Heading\r\n\r\n**bold** 🌍 [link](https://example.com)\n";
        std::fs::write(&path, content).unwrap();
        let (title, text) = read_markdown(&path).unwrap();
        assert_eq!(title, "旅行");
        assert_eq!(text, content);
        assert_eq!(std::fs::read(&path).unwrap(), content.as_bytes());
    }
    #[test]
    fn accepts_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.md");
        std::fs::write(&path, "").unwrap();
        assert_eq!(read_markdown(&path).unwrap(), ("empty".into(), "".into()));
    }
    #[test]
    fn rejects_invalid_utf8_binary_and_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        for bytes in [
            vec![255, 254],
            b"binary\0data".to_vec(),
            vec![b'x'; MAX_BYTES as usize + 1],
        ] {
            std::fs::write(&path, &bytes).unwrap();
            assert!(read_markdown(&path).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }
    #[test]
    fn rejects_wrong_extension_folder_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "text").unwrap();
        assert!(read_markdown(&path).is_err());
        let folder = dir.path().join("folder.md");
        std::fs::create_dir(&folder).unwrap();
        assert!(read_markdown(&folder).is_err());
        assert!(read_markdown(&dir.path().join("missing.md")).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original.md");
        let link = dir.path().join("link.md");
        std::fs::write(&original, "text").unwrap();
        std::os::unix::fs::symlink(&original, &link).unwrap();
        assert!(read_markdown(&link).is_err());
    }
    #[test]
    fn repeated_import_creates_separate_local_only_notes() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::db::initialize_database(&db).unwrap();
        let first = store(&mut db, "same".into(), "original".into()).unwrap();
        let second = store(&mut db, "same".into(), "new copy".into()).unwrap();
        assert_ne!(first.id, second.id);
        let original: String = db
            .query_row("SELECT content FROM notes WHERE id=?1", [first.id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(original, "original");
        assert_eq!(second.category, "Personal");
        assert!(!second.favorite);
        let associations: i64 = db
            .query_row("SELECT COUNT(*) FROM imported_notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(associations, 0);
    }
    #[test]
    fn database_failure_does_not_change_existing_notes() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::db::initialize_database(&db).unwrap();
        store(&mut db, "one".into(), "existing".into()).unwrap();
        db.execute_batch("CREATE TRIGGER fail_import BEFORE INSERT ON notes BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        assert!(store(&mut db, "two".into(), "new".into()).is_err());
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
