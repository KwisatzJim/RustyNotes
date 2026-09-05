//! Read-only Notes API requests. Downloads may be explicitly imported locally.
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use url::Url;

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_IMPORT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
pub struct RemoteNote {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub category: String,
    pub favorite: bool,
    pub modified: i64,
    pub etag: Option<String>,
    pub readonly: Option<bool>,
}

// The final chunk also lists ID-only references to notes in earlier chunks.
// Deny other fields on references so a malformed full note cannot silently
// become a reference and cause content to be skipped.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteReference {
    id: i64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DownloadEntry {
    Full(RemoteNote),
    Reference(NoteReference),
}

fn import_request(
    client: Client,
    server: &str,
    username: &str,
    password: &str,
    cursor: Option<&str>,
) -> Result<RequestBuilder, String> {
    let server = crate::settings::normalize_server_url(server)?;
    let endpoint = Url::parse(&server)
        .and_then(|base| base.join("index.php/apps/notes/api/v1/notes"))
        .map_err(|_| "Invalid Notes API address.".to_string())?;
    let mut request = client
        .get(endpoint)
        .basic_auth(username, Some(password))
        .header("Accept", "application/json")
        .query(&[("chunkSize", "100")]);
    if let Some(cursor) = cursor {
        request = request.query(&[("chunkCursor", cursor)]);
    }
    Ok(request)
}

async fn collect_pages<F, Fut>(mut fetch: F) -> Result<Vec<RemoteNote>, String>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: std::future::Future<Output = Result<Response, String>>,
{
    let mut notes = Vec::new();
    let mut ids = HashSet::new();
    let mut cursors = HashSet::new();
    let mut cursor = None;
    let mut total_bytes = 0;
    for page_index in 0..1000 {
        let mut response = fetch(cursor).await?;
        check_status(response.status())?;
        let next = response
            .headers()
            .get("X-Notes-Chunk-Cursor")
            .map(|value| {
                let cursor = value
                    .to_str()
                    .map_err(|_| "Invalid Notes pagination cursor.".to_string())?;
                if cursor.is_empty() || cursor.len() > 8192 {
                    return Err("Invalid Notes pagination cursor.".to_string());
                }
                Ok(cursor.to_owned())
            })
            .transpose()?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Download interrupted. Nothing was imported.".to_string())?
        {
            total_bytes += chunk.len();
            if total_bytes > MAX_IMPORT_BYTES {
                return Err(
                    "Download exceeded the 64 MiB import limit. Nothing was imported.".into(),
                );
            }
            bytes.extend_from_slice(&chunk);
        }
        let page: Vec<DownloadEntry> = serde_json::from_slice(&bytes)
            .map_err(|_| format!("Nextcloud returned an invalid note entry on page {}. Expected a complete note or an ID-only reference. Nothing was imported.", page_index + 1))?;
        let mut references = Vec::new();
        for entry in page {
            match entry {
                DownloadEntry::Full(note) => {
                    if note.id <= 0 || note.modified < 0 || !ids.insert(note.id) {
                        return Err("Nextcloud returned invalid or duplicate full note IDs. Nothing was imported; please retry.".into());
                    }
                    notes.push(note);
                }
                DownloadEntry::Reference(reference) => references.push(reference.id),
            }
        }
        if !references.is_empty() && next.is_some() {
            return Err("Nextcloud returned ID-only references before the final page. Nothing was imported.".into());
        }
        if references.iter().any(|id| *id <= 0 || !ids.contains(id)) {
            return Err("Nextcloud referenced a note whose content was not downloaded. The server may have changed during download; retry. Nothing was imported.".into());
        }
        match next {
            None => return Ok(notes),
            Some(next) => {
                if !cursors.insert(next.clone()) {
                    return Err(
                        "Nextcloud repeated a pagination cursor. Nothing was imported.".into(),
                    );
                }
                cursor = Some(next);
            }
        }
    }
    Err("Download exceeded the pagination limit. Nothing was imported.".into())
}

pub(crate) async fn download_notes(
    client: Client,
    server: &str,
    username: &str,
    password: &str,
) -> Result<Vec<RemoteNote>, String> {
    collect_pages(|cursor| {
        let request = import_request(client.clone(), server, username, password, cursor.as_deref());
        async move {
            request?.send().await.map_err(|_| "Could not download notes securely. Check your connection and certificate. Nothing was imported.".to_string())
        }
    }).await
}

#[tauri::command]
pub async fn refresh_server_notes(
    state: tauri::State<'_, crate::auth::LoginState>,
) -> Result<crate::refresh::RefreshSummary, String> {
    state.refresh_notes().await
}

#[tauri::command]
pub async fn import_server_notes(
    state: tauri::State<'_, crate::auth::LoginState>,
) -> Result<crate::import::ImportSummary, String> {
    state.import_notes().await
}

pub(crate) fn check_request(
    client: Client,
    server: &str,
    username: &str,
    password: &str,
) -> Result<RequestBuilder, String> {
    let server = crate::settings::normalize_server_url(server)?;
    let endpoint = Url::parse(&server)
        .and_then(|base| base.join("index.php/apps/notes/api/v1/notes"))
        .map_err(|_| "Invalid Notes API address.".to_string())?;
    // No chunkSize: request the entire ID list, not a partial page. Exclude
    // content and other metadata because this check needs only the count.
    Ok(client
        .get(endpoint)
        .basic_auth(username, Some(password))
        .header("Accept", "application/json")
        .query(&[(
            "exclude",
            "content,title,category,favorite,modified,etag,readonly",
        )]))
}

fn check_status(status: StatusCode) -> Result<(), String> {
    match status {
        StatusCode::OK => Ok(()),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err("Nextcloud denied Notes API access. Please authorize again and check your account's Notes access.".into()),
        StatusCode::NOT_FOUND => Err("Notes API not found. Check that the Nextcloud Notes app is enabled and the server base address is correct.".into()),
        StatusCode::TOO_MANY_REQUESTS => Err("Nextcloud is limiting requests. Please wait before checking again.".into()),
        _ if status.is_redirection() => Err("The Notes API redirected the request. Redirects are blocked to protect your credentials; check the final HTTPS server address.".into()),
        _ => Err(format!("Notes API check failed (HTTP {}). Please try again later.", status.as_u16())),
    }
}

#[derive(Deserialize)]
struct NoteId {
    id: u64,
}

fn count_notes(bytes: &[u8]) -> Result<usize, String> {
    let notes: Vec<NoteId> = serde_json::from_slice(bytes)
        .map_err(|_| "The server did not return a valid Notes API list.".to_string())?;
    let mut ids = HashSet::with_capacity(notes.len());
    if notes
        .iter()
        .any(|note| note.id == 0 || !ids.insert(note.id))
    {
        return Err(
            "The Notes API returned invalid or duplicate note IDs; the count cannot be trusted."
                .into(),
        );
    }
    Ok(notes.len())
}

async fn response_count(mut response: Response) -> Result<usize, String> {
    check_status(response.status())?;
    if response.headers().contains_key("X-Notes-Chunk-Cursor") {
        return Err(
            "The server returned only part of the Notes list. No total count is reported.".into(),
        );
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "The Notes API response was interrupted. Please try again.".to_string())?
    {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err("The Notes list exceeded the connection check's size limit. No total count is reported.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    count_notes(&bytes)
}

#[tauri::command]
pub async fn check_notes_connection(
    state: tauri::State<'_, crate::auth::LoginState>,
) -> Result<usize, String> {
    let request = state.prepare_notes_check().await?;
    let response = request.send().await
        .map_err(|_| "Could not reach the Notes API securely. Check your connection and HTTPS certificate, then retry.".to_string())?;
    response_count(response).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(id: i64, cursor: Option<&str>) -> Response {
        let body = serde_json::json!([{"id":id,"title":"Test","content":"# text","category":"Work","favorite":true,"modified":100}]);
        let mut builder = http::Response::builder();
        if let Some(cursor) = cursor {
            builder = builder.header("X-Notes-Chunk-Cursor", cursor);
        }
        Response::from(builder.body(reqwest::Body::from(body.to_string())).unwrap())
    }

    #[tokio::test]
    async fn imports_434_notes_with_final_page_id_only_references() {
        let mut responses = std::collections::VecDeque::new();
        for start in [1, 101, 201, 301, 401] {
            let end = (start + 99).min(434);
            let mut entries: Vec<_> = (start..=end).map(|id| serde_json::json!({
                "id":id,"title":"Test","content":"preserved Markdown", "category":"Work","favorite":false,"modified":100
            })).collect();
            let mut builder = http::Response::builder();
            if end < 434 {
                builder = builder.header("X-Notes-Chunk-Cursor", format!("after-{end}"));
            } else {
                entries.extend((1..start).map(|id| serde_json::json!({"id":id})));
            }
            responses.push_back(Response::from(
                builder
                    .body(reqwest::Body::from(serde_json::to_vec(&entries).unwrap()))
                    .unwrap(),
            ));
        }
        let notes = collect_pages(|_| std::future::ready(Ok(responses.pop_front().unwrap())))
            .await
            .unwrap();
        assert_eq!(notes.len(), 434);
        assert_eq!(
            notes
                .iter()
                .map(|note| note.id)
                .collect::<HashSet<_>>()
                .len(),
            434
        );
        assert!(notes
            .iter()
            .all(|note| note.content == "preserved Markdown"));
    }

    #[tokio::test]
    async fn unknown_reference_cannot_silently_omit_note_content() {
        let response = Response::from(http::Response::new(reqwest::Body::from(r#"[{"id":999}]"#)));
        let mut response = Some(response);
        let error = collect_pages(|_| std::future::ready(Ok(response.take().unwrap())))
            .await
            .err()
            .unwrap();
        assert!(error.contains("content was not downloaded"));
    }

    #[tokio::test]
    async fn malformed_full_note_cannot_be_treated_as_reference() {
        let mut responses = std::collections::VecDeque::from([
            page(1, Some("next")),
            Response::from(http::Response::new(reqwest::Body::from(
                r#"[{"id":1,"title":"private-title","content":null}]"#,
            ))),
        ]);
        let error = collect_pages(|_| std::future::ready(Ok(responses.pop_front().unwrap())))
            .await
            .err()
            .unwrap();
        assert!(error.contains("page 2"));
        assert!(!error.contains("private-title"));
    }

    #[tokio::test]
    async fn id_only_references_are_only_accepted_on_final_page() {
        let middle = http::Response::builder()
            .header("X-Notes-Chunk-Cursor", "third")
            .body(reqwest::Body::from(r#"[{"id":1}]"#))
            .unwrap();
        let mut responses =
            std::collections::VecDeque::from([page(1, Some("second")), Response::from(middle)]);
        let error = collect_pages(|_| std::future::ready(Ok(responses.pop_front().unwrap())))
            .await
            .err()
            .unwrap();
        assert!(error.contains("before the final page"));
    }

    #[tokio::test]
    async fn downloads_all_pages_before_returning_notes() {
        let mut responses =
            std::collections::VecDeque::from([page(1, Some("cursor-two")), page(2, None)]);
        let mut requested = Vec::new();
        let notes = collect_pages(|cursor| {
            requested.push(cursor);
            std::future::ready(Ok(responses.pop_front().unwrap()))
        })
        .await
        .unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(requested, vec![None, Some("cursor-two".into())]);
    }

    #[tokio::test]
    async fn incomplete_download_returns_no_batch() {
        let mut responses = std::collections::VecDeque::from([
            Ok(page(1, Some("next"))),
            Err("network interrupted".to_string()),
        ]);
        assert!(
            collect_pages(|_| std::future::ready(responses.pop_front().unwrap()))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn rejects_repeated_cursors_and_duplicate_ids_across_pages() {
        for mut responses in [
            std::collections::VecDeque::from([page(1, Some("same")), page(2, Some("same"))]),
            std::collections::VecDeque::from([page(1, Some("next")), page(1, None)]),
        ] {
            assert!(
                collect_pages(|_| std::future::ready(Ok(responses.pop_front().unwrap())))
                    .await
                    .is_err()
            );
        }
    }

    #[test]
    fn import_request_downloads_content_and_treats_cursor_as_data() {
        let request = import_request(
            Client::new(),
            "https://cloud.example.com/sub/",
            "user",
            "password",
            Some("https://evil.example/?x=1&exclude=content"),
        )
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(request.url().host_str(), Some("cloud.example.com"));
        assert_eq!(
            request.url().path(),
            "/sub/index.php/apps/notes/api/v1/notes"
        );
        let params: std::collections::HashMap<_, _> = request.url().query_pairs().collect();
        assert!(!params.contains_key("exclude"));
        assert_eq!(params["chunkSize"], "100");
        assert_eq!(
            params["chunkCursor"],
            "https://evil.example/?x=1&exclude=content"
        );
        assert!(request.body().is_none());
    }

    #[test]
    fn request_is_get_with_no_body_and_keeps_deployment_subdirectory() {
        let request = check_request(
            Client::new(),
            "https://cloud.example.com/nextcloud/",
            "test-user",
            "test-password",
        )
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(
            request.url().path(),
            "/nextcloud/index.php/apps/notes/api/v1/notes"
        );
        assert_eq!(request.url().host_str(), Some("cloud.example.com"));
        assert!(request.body().is_none());
        assert!(request.headers()["authorization"].is_sensitive());
        let params: Vec<_> = request.url().query_pairs().collect();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].0, "exclude");
        assert!(params[0].1.split(',').any(|field| field == "content"));
        assert!(!request.url().as_str().contains("test-password"));
    }

    #[test]
    fn counts_empty_and_populated_lists_without_exposing_note_fields() {
        assert_eq!(count_notes(b"[]").unwrap(), 0);
        assert_eq!(
            count_notes(br#"[{"id":1},{"id":3,"content":"private text","title":"private title"}]"#)
                .unwrap(),
            2
        );
    }

    #[test]
    fn rejects_invalid_and_duplicate_ids() {
        for data in [
            r#"[{"id":1},{"id":1}]"#,
            r#"[{"id":0}]"#,
            r#"[{"id":-1}]"#,
            r#"[{"id":"1"}]"#,
            "[{}]",
            "null",
            "{\"error\":\"private server details\"}",
        ] {
            let error = count_notes(data.as_bytes()).unwrap_err();
            assert!(!error.contains("private server details"));
        }
    }

    #[test]
    fn errors_distinguish_authorization_missing_app_and_redirects() {
        assert!(check_status(StatusCode::OK).is_ok());
        assert!(check_status(StatusCode::UNAUTHORIZED)
            .unwrap_err()
            .contains("authorize"));
        assert!(check_status(StatusCode::NOT_FOUND)
            .unwrap_err()
            .contains("enabled"));
        assert!(check_status(StatusCode::FOUND)
            .unwrap_err()
            .contains("blocked"));
        assert!(check_status(StatusCode::TOO_MANY_REQUESTS)
            .unwrap_err()
            .contains("wait"));
    }

    #[tokio::test]
    async fn rejects_partial_lists_instead_of_reporting_incorrect_total() {
        let response = http::Response::builder()
            .header("X-Notes-Chunk-Cursor", "next-page")
            .body(reqwest::Body::from("[{\"id\":1}]"))
            .unwrap();
        assert!(response_count(Response::from(response))
            .await
            .unwrap_err()
            .contains("part"));
    }

    #[tokio::test]
    async fn handles_success_and_caps_response_size() {
        let response = Response::from(http::Response::new(reqwest::Body::from("[{\"id\":1}]")));
        assert_eq!(response_count(response).await.unwrap(), 1);
        let response = Response::from(http::Response::new(reqwest::Body::from(vec![
            b' ';
            MAX_RESPONSE_BYTES
                + 1
        ])));
        assert!(response_count(response)
            .await
            .unwrap_err()
            .contains("size limit"));
    }
}
