//! Export raw Markdown through a native save dialog; no network or database writes.
use std::{io::Write, path::Path};
use tauri_plugin_dialog::DialogExt;

fn suggested_filename(title: &str) -> String {
    let mut name = String::new();
    for ch in title.trim().chars() {
        let ch = if ch.is_control() || "/\\:*?\"<>|".contains(ch) {
            '_'
        } else {
            ch
        };
        if name.len() + ch.len_utf8() > 160 {
            break;
        }
        name.push(ch);
    }
    let name = name.trim_matches([' ', '.']);
    if name.is_empty() {
        return "Untitled.md".into();
    }
    let name = if name.to_ascii_lowercase().ends_with(".md") {
        &name[..name.len() - 3]
    } else {
        name
    };
    let name = name.trim_matches([' ', '.']);
    if name.is_empty() {
        return "Untitled.md".into();
    }
    format!("{name}.md")
}

fn read_note(db: &rusqlite::Connection, id: i64) -> Result<(String, String), String> {
    db.query_row("SELECT title,content FROM notes WHERE id=?1", [id], |row| {
        Ok((row.get(0)?, row.get(1)?))
    })
    .map_err(|_| "Could not read this note for export. Nothing was exported.".into())
}

fn write_markdown(path: &Path, content: &str) -> Result<(), String> {
    if !path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("md"))
    {
        return Err("Choose a filename ending in .md. Nothing was exported.".into());
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(
                "Choose a regular file, not a folder or symbolic link. Nothing was exported."
                    .into(),
            )
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err("Could not check the export destination. Nothing was exported.".into())
        }
        _ => {}
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("Choose a full destination path.")?;
    // Write alongside the target and atomically replace it only after all bytes
    // are ready. The native Save dialog handles overwrite confirmation.
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|_| {
        "Could not create an export file in that folder. Check its permissions.".to_string()
    })?;
    temp.write_all(content.as_bytes())
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|_| {
            "Could not finish writing the export. The destination file was not replaced."
                .to_string()
        })?;
    temp.persist(path).map_err(|_| {
        "Could not save the export at that destination. Check permissions and available space."
            .to_string()
    })?;
    Ok(())
}

#[tauri::command]
pub async fn export_note(
    id: i64,
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Option<String>, String> {
    // The blocking native dialog must not run on the application's UI thread.
    tauri::async_runtime::spawn_blocking(move || {
        let db =
            crate::db::open_database().map_err(|_| "Could not open local notes.".to_string())?;
        let (title, content) = read_note(&db, id)?;
        drop(db);
        let chosen = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Export note as Markdown")
            .add_filter("Markdown", &["md"])
            .set_file_name(suggested_filename(&title))
            .blocking_save_file();
        let Some(chosen) = chosen else {
            return Ok(None);
        };
        let path = chosen
            .into_path()
            .map_err(|_| "Choose a local file destination.".to_string())?;
        write_markdown(&path, &content)?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|_| "Export could not be confirmed. Check the selected destination.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filename_is_safe_and_preserves_unicode() {
        assert_eq!(
            suggested_filename(" ../../Travel: 日本語? "),
            "_.._Travel_ 日本語_.md"
        );
        assert_eq!(suggested_filename("..."), "Untitled.md");
        assert_eq!(suggested_filename("   "), "Untitled.md");
        assert_eq!(suggested_filename("Example.MD"), "Example.md");
        assert_eq!(suggested_filename("a\nb\\c"), "a_b_c.md");
        assert!(suggested_filename(&"🌍".repeat(200)).len() <= 163);
    }
    #[test]
    fn exports_exact_markdown_without_adding_title_or_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        let text = "# Heading\n\n**bold** [link](https://example.com)\n日本語 🌍\r\n";
        write_markdown(&path, text).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), text.as_bytes());
    }
    #[test]
    fn replacement_and_empty_note_do_not_leave_old_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "previous longer content").unwrap();
        write_markdown(&path, "short").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "short");
        write_markdown(&path, "").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    }
    #[test]
    fn invalid_destination_does_not_replace_existing_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("database.db");
        std::fs::write(&path, "original").unwrap();
        assert!(write_markdown(&path, "replacement").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        assert!(write_markdown(&dir.path().join("missing/note.md"), "text").is_err());
        let folder = dir.path().join("folder.md");
        std::fs::create_dir(&folder).unwrap();
        assert!(write_markdown(&folder, "text").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symbolic_link_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original.md");
        let link = dir.path().join("link.md");
        std::fs::write(&original, "original").unwrap();
        std::os::unix::fs::symlink(&original, &link).unwrap();
        assert!(write_markdown(&link, "replacement").is_err());
        assert_eq!(std::fs::read_to_string(&original).unwrap(), "original");
    }
    #[test]
    fn reads_only_selected_note_and_rejects_missing_note() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::initialize_database(&db).unwrap();
        db.execute("INSERT INTO notes(title,content,modified_at) VALUES ('one','first',0),('two','second',0)", []).unwrap();
        assert_eq!(read_note(&db, 2).unwrap(), ("two".into(), "second".into()));
        assert!(read_note(&db, 3).is_err());
        assert_eq!(read_note(&db, 1).unwrap().1, "first");
    }
}
