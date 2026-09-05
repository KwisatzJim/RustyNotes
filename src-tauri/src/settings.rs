use rusqlite::{Connection, OptionalExtension};
use url::Url;

pub(crate) fn normalize_server_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Enter your Nextcloud server address.".into());
    }
    if input.chars().any(char::is_whitespace) || input.contains('\\') {
        return Err("The server address cannot contain spaces or backslashes.".into());
    }
    let mut url =
        Url::parse(input).map_err(|_| "Enter a complete HTTPS server address.".to_string())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !input.to_lowercase().starts_with("https://")
    {
        return Err("Use an HTTPS address, such as https://cloud.example.com.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Do not include a username or password in the server address.".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("Use the server's base address, without a query or # fragment.".into());
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url.into())
}

fn initialize(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
}

fn read_url(connection: &Connection) -> rusqlite::Result<Option<String>> {
    initialize(connection)?;
    connection
        .query_row(
            "SELECT value FROM settings WHERE key = 'server_url'",
            [],
            |row| row.get(0),
        )
        .optional()
}

fn write_url(connection: &Connection, input: &str) -> Result<String, String> {
    let normalized = normalize_server_url(input)?;
    initialize(connection).map_err(|_| "Could not initialize settings storage.".to_string())?;
    connection.execute(
        "INSERT INTO settings (key, value) VALUES ('server_url', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [&normalized],
    ).map_err(|_| "Could not save the server address.".to_string())?;
    Ok(normalized)
}

#[tauri::command]
pub fn get_server_url() -> Result<Option<String>, String> {
    let connection =
        crate::db::open_database().map_err(|_| "Could not open settings storage.".to_string())?;
    read_url(&connection).map_err(|_| "Could not read the server address.".to_string())
}

#[tauri::command]
pub async fn save_server_url(
    server_url: String,
    state: tauri::State<'_, crate::auth::LoginState>,
) -> Result<String, String> {
    state.save_address(&server_url).await
}

pub(crate) fn persist_server_url(server_url: &str) -> Result<String, String> {
    let connection =
        crate::db::open_database().map_err(|_| "Could not open settings storage.".to_string())?;
    write_url(&connection, server_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_root_subdirectory_and_custom_port() {
        assert_eq!(
            normalize_server_url(" https://Cloud.Example.com ").unwrap(),
            "https://cloud.example.com/"
        );
        assert_eq!(
            normalize_server_url("https://cloud.example.com:8443/nextcloud").unwrap(),
            "https://cloud.example.com:8443/nextcloud/"
        );
        assert_eq!(
            normalize_server_url("https://cloud.example.com/nextcloud/").unwrap(),
            "https://cloud.example.com/nextcloud/"
        );
    }

    #[test]
    fn rejects_unsafe_or_incomplete_addresses() {
        for input in [
            "",
            "cloud.example.com",
            "http://cloud.example.com",
            "file:///tmp",
            "https:cloud.example.com",
            "https://user:secret@cloud.example.com",
            "https://cloud.example.com/?token=secret",
            "https://cloud.example.com/#notes",
            "https://cloud.example.com/a b",
            "https://cloud.example.com\\notes",
        ] {
            assert!(normalize_server_url(input).is_err(), "accepted {input}");
        }
    }

    #[test]
    fn settings_round_trip_and_rejected_update_preserves_previous_value() {
        let connection = Connection::open_in_memory().unwrap();
        assert_eq!(read_url(&connection).unwrap(), None);
        write_url(&connection, "https://cloud.example.com/nextcloud").unwrap();
        assert!(write_url(&connection, "http://other.example.com").is_err());
        assert_eq!(
            read_url(&connection).unwrap().unwrap(),
            "https://cloud.example.com/nextcloud/"
        );
        write_url(&connection, "https://other.example.com").unwrap();
        assert_eq!(
            read_url(&connection).unwrap().unwrap(),
            "https://other.example.com/"
        );
    }
}
