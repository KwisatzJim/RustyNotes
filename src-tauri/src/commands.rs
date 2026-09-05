use crate::db::{self, Note};
use tauri::command;

#[command]
pub fn get_notes() -> Result<Vec<Note>, String> {
    db::get_notes().map_err(|error| error.to_string())
}

#[command]
pub fn create_note(title: String, content: String, category: String) -> Result<Note, String> {
    db::create_note(&title, &content, &category).map_err(|error| error.to_string())
}

#[command]
pub fn update_note(
    id: i64,
    title: String,
    content: String,
    category: String,
    favorite: bool,
) -> Result<Note, String> {
    db::update_note(id, &title, &content, &category, favorite).map_err(|error| error.to_string())
}

#[command]
pub fn delete_note(id: i64) -> Result<(), String> {
    db::delete_note(id).map_err(|error| error.to_string())
}
