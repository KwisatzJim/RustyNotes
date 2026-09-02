//! Explicit, single-note conditional updates. Never creates or deletes remotely.
use crate::{db::Note, notes_api::RemoteNote};
use reqwest::{Client, Response, StatusCode};
use rusqlite::{Connection, TransactionBehavior};
use url::Url;

const UNCERTAIN: &str = "The upload outcome is uncertain. Your local note is preserved. Check Nextcloud and refresh before trying again.";
pub(crate) struct Pending {
    local: Note,
    baseline: RemoteNote,
    baseline_json: String,
}
fn fields(title: &str, content: &str, category: &str, favorite: bool) -> serde_json::Value {
    serde_json::json!([title, content, category, favorite])
}
fn remote_fields(note: &RemoteNote) -> serde_json::Value {
    fields(&note.title, &note.content, &note.category, note.favorite)
}
fn local_fields(note: &Note) -> serde_json::Value {
    fields(&note.title, &note.content, &note.category, note.favorite)
}
pub(crate) fn prepare(
    db: &Connection,
    server: &str,
    account: &str,
    id: i64,
) -> Result<Pending, String> {
    let (local, baseline_json): (Note, String) = db.query_row(
        "SELECT n.id,n.title,n.content,n.category,n.favorite,n.modified_at,i.original_snapshot FROM notes n JOIN imported_notes i ON i.local_id=n.id WHERE n.id=?1 AND i.server=?2 AND i.account=?3",
        (id, server, account), |r| Ok((Note { id:r.get(0)?,title:r.get(1)?,content:r.get(2)?,category:r.get(3)?,favorite:r.get(4)?,modified_at:r.get(5)? },r.get(6)?)))
        .map_err(|_| "Select an existing note imported from the currently authorized account. Local-only notes cannot be uploaded yet.".to_string())?;
    let baseline: RemoteNote = serde_json::from_str(&baseline_json)
        .map_err(|_| "Invalid saved server snapshot.".to_string())?;
    if baseline.id <= 0 {
        return Err("Invalid saved server note ID.".into());
    }
    let unresolved: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM refresh_conflicts c LEFT JOIN conflict_resolutions r ON r.conflict_id=c.id WHERE c.local_id=?1 AND r.conflict_id IS NULL)", [id], |r|r.get(0)).map_err(|_| "Could not check saved conflicts.".to_string())?;
    if unresolved {
        return Err("Resolve this note's saved conflict before uploading.".into());
    }
    Ok(Pending {
        local,
        baseline,
        baseline_json,
    })
}
fn supports_conditional_updates(value: &str) -> bool {
    value.split(',').any(|version| {
        version
            .trim()
            .split_once('.')
            .is_some_and(|(major, minor)| {
                major == "1" && minor.parse::<u32>().is_ok_and(|minor| minor >= 2)
            })
    })
}
fn etag_header(note: &RemoteNote) -> Result<String, String> {
    let tag = note.etag.as_deref().unwrap_or("");
    // A single strong entity tag, never a wildcard or list of alternatives.
    if tag.is_empty()
        || tag.len() > 1024
        || !tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("No usable server version identifier. Upload was not sent.".into());
    }
    Ok(format!("\"{tag}\""))
}
async fn read_note(mut response: Response, id: i64) -> Result<RemoteNote, String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Interrupted note response.".to_string())?
    {
        if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
            return Err("Note response exceeded the size limit.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let note: RemoteNote =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid note response.".to_string())?;
    if note.id != id || note.modified < 0 {
        return Err("Unexpected note response.".into());
    }
    Ok(note)
}
fn validate_preflight(pending: &Pending, remote: &RemoteNote) -> Result<String, String> {
    if remote.id != pending.baseline.id || remote_fields(remote) != remote_fields(&pending.baseline)
    {
        return Err("The server note has changed. Nothing was uploaded. Refresh and review the versions first.".into());
    }
    if remote.readonly != Some(false) {
        return Err(
            "The server did not confirm this note is writable. Nothing was uploaded.".into(),
        );
    }
    etag_header(remote)
}
fn put_request(
    client: &Client,
    endpoint: Url,
    username: &str,
    password: &str,
    pending: &Pending,
    etag: &str,
) -> reqwest::RequestBuilder {
    client.put(endpoint).basic_auth(username, Some(password)).header("Accept", "application/json").header("If-Match", etag)
        .json(&serde_json::json!({"title":pending.local.title,"content":pending.local.content,"category":pending.local.category,"favorite":pending.local.favorite}))
}
pub(crate) async fn send(
    client: Client,
    server: &str,
    username: &str,
    password: &str,
    pending: &Pending,
) -> Result<RemoteNote, String> {
    exchange(&client, server, username, password, pending, |request| {
        let client = client.clone();
        async move { client.execute(request).await.map_err(|_| ()) }
    })
    .await
}

async fn exchange<F, Fut>(
    client: &Client,
    server: &str,
    username: &str,
    password: &str,
    pending: &Pending,
    mut execute: F,
) -> Result<RemoteNote, String>
where
    F: FnMut(reqwest::Request) -> Fut,
    Fut: std::future::Future<Output = Result<Response, ()>>,
{
    let server = crate::settings::normalize_server_url(server)?;
    let endpoint = Url::parse(&server)
        .and_then(|base| {
            base.join(&format!(
                "index.php/apps/notes/api/v1/notes/{}",
                pending.baseline.id
            ))
        })
        .map_err(|_| "Invalid Notes address.".to_string())?;
    let request = client
        .get(endpoint.clone())
        .basic_auth(username, Some(password))
        .header("Accept", "application/json")
        .build()
        .map_err(|_| "Could not prepare the server check.".to_string())?;
    let response = execute(request).await.map_err(|_| {
        "Could not securely check the server note. Nothing was uploaded.".to_string()
    })?;
    if response.status() == StatusCode::LOCKED {
        return Err("Nextcloud reports this note is locked (HTTP 423). Nothing was uploaded. Close any editors using this note, wait briefly, then try again. If the lock persists, check the Nextcloud server log.".into());
    }
    if response.status() != StatusCode::OK {
        return Err(format!(
            "Server check failed (HTTP {}). Nothing was uploaded.",
            response.status().as_u16()
        ));
    }
    if !supports_conditional_updates(
        response
            .headers()
            .get("X-Notes-API-Versions")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
    ) {
        return Err(
            "Server has not advertised Notes API 1.2 or newer. Safe uploads are disabled.".into(),
        );
    }
    let remote = read_note(response, pending.baseline.id).await?;
    let etag = validate_preflight(pending, &remote)?;
    if local_fields(&pending.local) == remote_fields(&remote) {
        return Ok(remote);
    }
    let request = put_request(client, endpoint, username, password, pending, &etag)
        .build()
        .map_err(|_| "Could not prepare upload. Nothing was sent.".to_string())?;
    let response = execute(request).await.map_err(|_| UNCERTAIN.to_string())?;
    if response.status() == StatusCode::PRECONDITION_FAILED {
        return Err("Nextcloud refused the upload because the server note changed. Refresh and review the conflict before uploading again.".into());
    }
    if response.status() == StatusCode::LOCKED {
        return Err("Nextcloud reports this note is locked (HTTP 423), so the upload could not be confirmed. Your local edits are preserved. Close any editors using this note, wait briefly, then check Nextcloud and refresh before retrying. If the lock persists, check the Nextcloud server log.".into());
    }
    if response.status() != StatusCode::OK {
        return Err(format!(
            "Upload returned HTTP {}. {UNCERTAIN}",
            response.status().as_u16()
        ));
    }
    let result = read_note(response, pending.baseline.id)
        .await
        .map_err(|_| UNCERTAIN.to_string())?;
    etag_header(&result).map_err(|_| UNCERTAIN.to_string())?;
    // Title/category can be sanitized by Nextcloud. Never silently discard text.
    if result.content != pending.local.content || result.favorite != pending.local.favorite {
        return Err(UNCERTAIN.into());
    }
    Ok(result)
}
pub(crate) fn finish(
    db: &mut Connection,
    server: &str,
    account: &str,
    pending: &Pending,
    remote: &RemoteNote,
) -> Result<(), String> {
    let apply = || -> Result<(), Box<dyn std::error::Error>> {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = prepare(&tx, server, account, pending.local.id)?;
        if local_fields(&current.local) != local_fields(&pending.local)
            || current.baseline_json != pending.baseline_json
        {
            return Err("Local note changed during upload".into());
        }
        tx.execute("UPDATE notes SET title=?1,content=?2,category=?3,favorite=?4,modified_at=?5 WHERE id=?6", (&remote.title,&remote.content,&remote.category,remote.favorite,remote.modified,pending.local.id))?;
        tx.execute(
            "UPDATE imported_notes SET original_snapshot=?1 WHERE local_id=?2",
            (serde_json::to_string(remote)?, pending.local.id),
        )?;
        tx.commit()?;
        Ok(())
    };
    let mut apply = apply;
    apply().map_err(|_| "The server accepted the note, but local confirmation failed. Local edits were preserved. Refresh before uploading again.".into())
}

#[tauri::command]
pub async fn upload_note(
    id: i64,
    state: tauri::State<'_, crate::auth::LoginState>,
) -> Result<(), String> {
    state.upload_note(id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn remote(content: &str) -> RemoteNote {
        RemoteNote {
            id: 7,
            title: "Test".into(),
            content: content.into(),
            category: "Work".into(),
            favorite: false,
            modified: 50,
            etag: Some("abc123".into()),
            readonly: Some(false),
        }
    }
    fn setup() -> (Connection, Pending) {
        let mut db = Connection::open_in_memory().unwrap();
        crate::db::initialize_database(&db).unwrap();
        crate::import::store_batch(
            &mut db,
            "https://cloud/nextcloud/",
            "jim",
            &[remote("base")],
        )
        .unwrap();
        db.execute("UPDATE notes SET content='edit'", []).unwrap();
        let pending = prepare(&db, "https://cloud/nextcloud/", "jim", 1).unwrap();
        (db, pending)
    }
    fn response(status: u16, version: &str, note: &RemoteNote) -> Response {
        Response::from(
            http::Response::builder()
                .status(status)
                .header("X-Notes-API-Versions", version)
                .body(reqwest::Body::from(serde_json::to_string(note).unwrap()))
                .unwrap(),
        )
    }
    async fn run(
        pending: &Pending,
        responses: Vec<Result<Response, ()>>,
    ) -> (Result<RemoteNote, String>, Vec<reqwest::Request>) {
        let mut responses = responses.into_iter();
        let mut requests = Vec::new();
        let result = exchange(
            &Client::new(),
            "https://cloud/nextcloud/",
            "jim",
            "fake-test-password",
            pending,
            |request| {
                requests.push(request);
                std::future::ready(responses.next().expect("unexpected request"))
            },
        )
        .await;
        (result, requests)
    }
    #[test]
    fn rejects_other_accounts_local_only_and_unresolved_conflicts() {
        let (mut db, _) = setup();
        assert!(prepare(&db, "https://cloud/nextcloud/", "someone-else", 1).is_err());
        assert!(prepare(&db, "https://other/", "jim", 1).is_err());
        db.execute(
            "INSERT INTO notes(title,content,modified_at) VALUES ('local','text',0)",
            [],
        )
        .unwrap();
        assert!(prepare(&db, "https://cloud/nextcloud/", "jim", 2).is_err());
        crate::refresh::store_refresh(
            &mut db,
            "https://cloud/nextcloud/",
            "jim",
            &[remote("remote edit")],
        )
        .unwrap();
        assert!(prepare(&db, "https://cloud/nextcloud/", "jim", 1).is_err());
    }
    #[test]
    fn validates_api_versions_and_single_strong_tags() {
        for version in ["", "1.0", "1.1", "2.0", "1.x"] {
            assert!(!supports_conditional_updates(version));
        }
        for version in ["1.2", "0.2, 1.3", "1.20"] {
            assert!(supports_conditional_updates(version));
        }
        for tag in ["", "*", "a,b", "\"a\"", "W/abc", "abc\r\nHeader: value"] {
            let mut note = remote("base");
            note.etag = Some(tag.into());
            assert!(etag_header(&note).is_err());
        }
    }
    #[tokio::test]
    async fn successful_upload_sends_only_selected_note_with_quoted_if_match() {
        let (mut db, pending) = setup();
        let mut saved = remote("edit");
        saved.title = "Sanitized title".into();
        saved.etag = Some("new456".into());
        let (result, requests) = run(
            &pending,
            vec![
                Ok(response(200, "1.3", &remote("base"))),
                Ok(response(200, "1.3", &saved)),
            ],
        )
        .await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method(), reqwest::Method::GET);
        assert_eq!(requests[1].method(), reqwest::Method::PUT);
        assert_eq!(
            requests[1].url().as_str(),
            "https://cloud/nextcloud/index.php/apps/notes/api/v1/notes/7"
        );
        assert_eq!(requests[1].headers()["If-Match"], "\"abc123\"");
        let body: serde_json::Value =
            serde_json::from_slice(requests[1].body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"title":"Test","content":"edit","category":"Work","favorite":false})
        );
        finish(
            &mut db,
            "https://cloud/nextcloud/",
            "jim",
            &pending,
            &result.unwrap(),
        )
        .unwrap();
        let after = prepare(&db, "https://cloud/nextcloud/", "jim", 1).unwrap();
        assert_eq!(after.local.title, "Sanitized title");
        assert_eq!(after.baseline.etag.as_deref(), Some("new456"));
        assert_eq!(
            crate::refresh::store_refresh(&mut db, "https://cloud/nextcloud/", "jim", &[saved])
                .unwrap()
                .unchanged,
            1
        );
    }
    #[tokio::test]
    async fn changed_remote_or_unsupported_server_never_sends_put() {
        let (_, pending) = setup();
        let mut readonly = remote("base");
        readonly.readonly = Some(true);
        let mut no_tag = remote("base");
        no_tag.etag = None;
        for first in [
            response(200, "1.3", &remote("new remote")),
            response(200, "1.1", &remote("base")),
            response(200, "", &remote("base")),
            response(200, "1.3", &readonly),
            response(200, "1.3", &no_tag),
            response(404, "1.3", &remote("base")),
            response(302, "1.3", &remote("base")),
        ] {
            let (result, requests) = run(&pending, vec![Ok(first)]).await;
            assert!(result.is_err());
            assert_eq!(requests.len(), 1);
        }
    }
    #[tokio::test]
    async fn change_between_check_and_put_is_rejected_without_retry() {
        let (_, pending) = setup();
        let (result, requests) = run(
            &pending,
            vec![
                Ok(response(200, "1.3", &remote("base"))),
                Ok(response(412, "1.3", &remote("racing edit"))),
            ],
        )
        .await;
        assert!(result.err().unwrap().contains("refused"));
        assert_eq!(requests.len(), 2);
    }
    #[tokio::test]
    async fn locked_note_explains_recovery_without_retrying_or_changing_local_data() {
        let (db, pending) = setup();
        for during_upload in [false, true] {
            let mut responses = Vec::new();
            if during_upload {
                responses.push(Ok(response(200, "1.3", &remote("base"))));
            }
            responses.push(Ok(response(423, "1.3", &remote("base"))));
            let (result, requests) = run(&pending, responses).await;
            let message = result.err().unwrap();
            assert!(message.contains("locked (HTTP 423)"));
            assert!(message.contains("Close any editors"));
            assert_eq!(requests.len(), if during_upload { 2 } else { 1 });
            if during_upload {
                assert!(message.contains("could not be confirmed"));
                assert!(message.contains("refresh before retrying"));
                assert!(!message.contains("Nothing was uploaded"));
            }
            let after = prepare(&db, "https://cloud/nextcloud/", "jim", 1).unwrap();
            assert_eq!(after.local.content, "edit");
            assert_eq!(after.baseline.content, "base");
        }
    }
    #[tokio::test]
    async fn uncertain_failure_never_retries_or_advances_baseline() {
        let (db, pending) = setup();
        for second in [
            Err(()),
            Ok(response(500, "1.3", &remote("edit"))),
            Ok(response(200, "1.3", &remote("unexpected text"))),
        ] {
            let (result, requests) = run(
                &pending,
                vec![Ok(response(200, "1.3", &remote("base"))), second],
            )
            .await;
            assert!(result.err().unwrap().contains("uncertain"));
            assert_eq!(requests.len(), 2);
            assert_eq!(
                prepare(&db, "https://cloud/nextcloud/", "jim", 1)
                    .unwrap()
                    .baseline
                    .content,
                "base"
            );
        }
    }
    #[tokio::test]
    async fn unchanged_note_does_not_send_put() {
        let (_, mut pending) = setup();
        pending.local.content = "base".into();
        let (result, requests) =
            run(&pending, vec![Ok(response(200, "1.3", &remote("base")))]).await;
        assert!(result.is_ok());
        assert_eq!(requests.len(), 1);
    }
    #[test]
    fn newer_local_edits_and_database_failures_are_preserved() {
        let (mut db, pending) = setup();
        db.execute("UPDATE notes SET content='newer edit'", [])
            .unwrap();
        assert!(finish(
            &mut db,
            "https://cloud/nextcloud/",
            "jim",
            &pending,
            &remote("edit")
        )
        .is_err());
        assert_eq!(
            prepare(&db, "https://cloud/nextcloud/", "jim", 1)
                .unwrap()
                .local
                .content,
            "newer edit"
        );
        db.execute("UPDATE notes SET content='edit'", []).unwrap();
        db.execute_batch("CREATE TRIGGER fail_upload BEFORE UPDATE ON imported_notes BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        let mut saved = remote("edit");
        saved.title = "Sanitized".into();
        assert!(finish(&mut db, "https://cloud/nextcloud/", "jim", &pending, &saved).is_err());
        let after = prepare(&db, "https://cloud/nextcloud/", "jim", 1).unwrap();
        assert_eq!(after.local.title, "Test");
        assert_eq!(after.baseline.content, "base");
    }
    #[tokio::test]
    async fn malformed_and_wrong_id_responses_are_rejected() {
        let mut wrong = remote("base");
        wrong.id = 8;
        assert!(read_note(response(200, "1.3", &wrong), 7).await.is_err());
        let malformed = Response::from(
            http::Response::builder()
                .body(reqwest::Body::from("secret malformed response"))
                .unwrap(),
        );
        let error = read_note(malformed, 7).await.err().unwrap();
        assert!(!error.contains("secret"));
        let oversized = Response::from(
            http::Response::builder()
                .body(reqwest::Body::from(vec![b' '; 4 * 1024 * 1024 + 1]))
                .unwrap(),
        );
        assert!(read_note(oversized, 7).await.is_err());
    }
}
