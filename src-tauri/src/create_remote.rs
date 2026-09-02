//! New-note POSTs are never retried after a persisted attempt, even after restart.
use crate::{db::Note, notes_api::RemoteNote};
use reqwest::{Client, Response, StatusCode};
use rusqlite::{Connection, TransactionBehavior};
use url::Url;

const REVIEW: &str = "New-note creation could not be confirmed. Your local note is preserved. To prevent duplicates, creation is blocked for this note, even after restart. Check Nextcloud, refresh, then open Recover uploads to review a downloaded server copy. Do not recreate or re-upload this note blindly.";

fn local(db: &Connection, id: i64) -> Result<Note, String> {
    db.query_row(
        "SELECT id,title,content,category,favorite,modified_at FROM notes WHERE id=?1",
        [id],
        |r| {
            Ok(Note {
                id: r.get(0)?,
                title: r.get(1)?,
                content: r.get(2)?,
                category: r.get(3)?,
                favorite: r.get(4)?,
                modified_at: r.get(5)?,
            })
        },
    )
    .map_err(|_| "Local note not found.".into())
}
fn linked(db: &Connection, id: i64) -> Result<bool, String> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM imported_notes WHERE local_id=?1)",
        [id],
        |r| r.get(0),
    )
    .map_err(|_| "Could not check note association.".into())
}
#[tauri::command]
pub fn is_local_only_note(id: i64) -> Result<bool, String> {
    let db = crate::db::open_database().map_err(|_| "Could not open local storage.".to_string())?;
    local(&db, id)?;
    Ok(!linked(&db, id)?)
}
pub(crate) fn begin(
    db: &mut Connection,
    server: &str,
    account: &str,
    id: i64,
) -> Result<Note, String> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "Could not prepare new-note upload.".to_string())?;
    if linked(&tx, id)? {
        return Err("This note already has a server association. Close and reopen Upload to update it instead.".into());
    }
    let note = local(&tx, id)?;
    let snapshot = serde_json::to_string(&note)
        .map_err(|_| "Could not preserve the local note.".to_string())?;
    tx.execute("INSERT INTO note_creation_attempts(local_id,server,account,local_snapshot) VALUES (?1,?2,?3,?4)",(id,server,account,snapshot)).map_err(|_| REVIEW.to_string())?;
    tx.commit().map_err(|_| {
        "Could not record the upload attempt. No create request was sent.".to_string()
    })?;
    Ok(note)
}
fn request(
    client: &Client,
    server: &str,
    username: &str,
    password: &str,
    note: &Note,
) -> Result<reqwest::Request, String> {
    let server = crate::settings::normalize_server_url(server)?;
    let url = Url::parse(&server)
        .and_then(|base| base.join("index.php/apps/notes/api/v1/notes"))
        .map_err(|_| "Invalid Notes address.".to_string())?;
    client.post(url).basic_auth(username,Some(password)).header("Accept","application/json")
        .json(&serde_json::json!({"title":note.title,"content":note.content,"category":note.category,"favorite":note.favorite}))
        .build().map_err(|_| "Could not prepare new-note request.".into())
}
async fn read_created(mut response: Response, note: &Note) -> Result<RemoteNote, String> {
    if response.status() != StatusCode::OK {
        return Err(format!(
            "Nextcloud returned HTTP {}. {REVIEW}",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| REVIEW.to_string())? {
        if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
            return Err(REVIEW.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let remote: RemoteNote = serde_json::from_slice(&bytes).map_err(|_| REVIEW.to_string())?;
    if remote.id <= 0
        || remote.modified < 0
        || remote.content != note.content
        || remote.favorite != note.favorite
        || remote.etag.as_deref().is_none_or(str::is_empty)
    {
        return Err(REVIEW.into());
    }
    Ok(remote)
}
pub(crate) async fn send(
    client: Client,
    server: &str,
    username: &str,
    password: &str,
    note: &Note,
) -> Result<RemoteNote, String> {
    send_with(&client, server, username, password, note, |request| {
        let client = client.clone();
        async move { client.execute(request).await.map_err(|_| ()) }
    })
    .await
}
async fn send_with<F, Fut>(
    client: &Client,
    server: &str,
    username: &str,
    password: &str,
    note: &Note,
    execute: F,
) -> Result<RemoteNote, String>
where
    F: FnOnce(reqwest::Request) -> Fut,
    Fut: std::future::Future<Output = Result<Response, ()>>,
{
    let request = request(client, server, username, password, note)?;
    let response = execute(request).await.map_err(|_| REVIEW.to_string())?;
    read_created(response, note).await
}
pub(crate) fn finish(
    db: &mut Connection,
    server: &str,
    account: &str,
    sent: &Note,
    remote: &RemoteNote,
) -> Result<(), String> {
    let snapshot = serde_json::to_string(remote).map_err(|_| REVIEW.to_string())?;
    // Keep the returned identity even if the subsequent association transaction fails.
    let recorded=db.execute("UPDATE note_creation_attempts SET server_snapshot=?1 WHERE local_id=?2 AND server=?3 AND account=?4 AND completed=0",(&snapshot,sent.id,server,account)).map_err(|_| REVIEW.to_string())?;
    if recorded != 1 {
        return Err(REVIEW.into());
    }
    let apply = || -> Result<(), Box<dyn std::error::Error>> {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO imported_notes(server,account,remote_id,local_id,original_snapshot) VALUES (?1,?2,?3,?4,?5)",(server,account,remote.id,sent.id,&snapshot))?;
        // If another local writer edited or deleted the note, retain that change.
        // The association still records the uploaded version as its baseline.
        tx.execute("UPDATE notes SET title=?1,content=?2,category=?3,favorite=?4,modified_at=?5 WHERE id=?6 AND title=?7 AND content=?8 AND category=?9 AND favorite=?10",(&remote.title,&remote.content,&remote.category,remote.favorite,remote.modified,sent.id,&sent.title,&sent.content,&sent.category,sent.favorite))?;
        tx.execute(
            "UPDATE note_creation_attempts SET completed=1 WHERE local_id=?1",
            [sent.id],
        )?;
        tx.commit()?;
        Ok(())
    };
    let mut apply = apply;
    apply().map_err(|_| REVIEW.into())
}
#[tauri::command]
pub async fn create_server_note(
    id: i64,
    state: tauri::State<'_, crate::auth::LoginState>,
) -> Result<(), String> {
    state.create_server_note(id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    const SERVER: &str = "https://cloud/nextcloud/";
    fn setup() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        seed(&db);
        db
    }
    fn seed(db: &Connection) {
        crate::db::initialize_database(db).unwrap();
        db.execute("INSERT INTO notes(title,content,category,favorite,modified_at) VALUES ('New note','my text','Work',1,50)",[]).unwrap();
    }
    fn remote() -> RemoteNote {
        RemoteNote {
            id: 701,
            title: "New note".into(),
            content: "my text".into(),
            category: "Work".into(),
            favorite: true,
            modified: 60,
            etag: Some("abc123".into()),
            readonly: Some(false),
        }
    }
    fn response(status: u16, body: String) -> Response {
        Response::from(
            http::Response::builder()
                .status(status)
                .body(reqwest::Body::from(body))
                .unwrap(),
        )
    }
    #[tokio::test]
    async fn creates_one_post_and_links_existing_local_note() {
        let mut db = setup();
        let sent = begin(&mut db, SERVER, "jim", 1).unwrap();
        let saved=send_with(&Client::new(),SERVER,"jim","fake-password",&sent,|request| {
            assert_eq!(request.method(),reqwest::Method::POST);
            assert_eq!(request.url().as_str(),"https://cloud/nextcloud/index.php/apps/notes/api/v1/notes");
            let json:serde_json::Value=serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
            assert_eq!(json,serde_json::json!({"title":"New note","content":"my text","category":"Work","favorite":true}));
            std::future::ready(Ok(response(200,serde_json::to_string(&remote()).unwrap())))
        }).await.unwrap();
        finish(&mut db, SERVER, "jim", &sent, &saved).unwrap();
        assert!(linked(&db, 1).unwrap());
        assert!(begin(&mut db, SERVER, "jim", 1).is_err());
        let summary = crate::refresh::store_refresh(&mut db, SERVER, "jim", &[remote()]).unwrap();
        assert_eq!(summary.added, 0);
        assert_eq!(summary.unchanged, 1);
        assert!(crate::upload::prepare(&db, SERVER, "jim", 1).is_ok());
    }
    #[tokio::test]
    async fn lost_response_blocks_recreation_after_database_reopen() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "rustynotes-create-test-{}-{stamp}.sqlite",
            std::process::id()
        ));
        {
            let mut db = Connection::open(&path).unwrap();
            seed(&db);
            let sent = begin(&mut db, SERVER, "jim", 1).unwrap();
            let result = send_with(&Client::new(), SERVER, "jim", "fake", &sent, |_| {
                std::future::ready(Err(()))
            })
            .await;
            assert!(result.err().unwrap().contains("prevent duplicates"));
        }
        {
            let mut db = Connection::open(&path).unwrap();
            crate::db::initialize_database(&db).unwrap();
            assert!(begin(&mut db, SERVER, "jim", 1).is_err());
            assert!(begin(&mut db, "https://other/", "other", 1).is_err());
            assert_eq!(local(&db, 1).unwrap().content, "my text");
            assert!(!linked(&db, 1).unwrap());
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn journal_failure_stops_before_network_and_retains_note() {
        let mut db = setup();
        db.execute_batch("CREATE TRIGGER fail_attempt BEFORE INSERT ON note_creation_attempts BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        assert!(begin(&mut db, SERVER, "jim", 1).is_err());
        assert_eq!(local(&db, 1).unwrap().content, "my text");
    }
    #[test]
    fn successful_link_preserves_newer_local_edits() {
        let mut db = setup();
        let sent = begin(&mut db, SERVER, "jim", 1).unwrap();
        db.execute("UPDATE notes SET content='newer edit' WHERE id=1", [])
            .unwrap();
        finish(&mut db, SERVER, "jim", &sent, &remote()).unwrap();
        assert_eq!(local(&db, 1).unwrap().content, "newer edit");
        assert_eq!(
            crate::refresh::store_refresh(&mut db, SERVER, "jim", &[remote()])
                .unwrap()
                .local_preserved,
            1
        );
    }
    #[test]
    fn local_deletion_during_request_is_not_resurrected() {
        let mut db = setup();
        let sent = begin(&mut db, SERVER, "jim", 1).unwrap();
        db.execute("DELETE FROM notes WHERE id=1", []).unwrap();
        finish(&mut db, SERVER, "jim", &sent, &remote()).unwrap();
        assert!(local(&db, 1).is_err());
        assert_eq!(
            crate::refresh::store_refresh(&mut db, SERVER, "jim", &[remote()])
                .unwrap()
                .locally_deleted,
            1
        );
    }
    #[test]
    fn failed_association_retains_response_identity_and_blocks_retry() {
        let mut db = setup();
        let sent = begin(&mut db, SERVER, "jim", 1).unwrap();
        db.execute_batch("CREATE TRIGGER fail_link BEFORE INSERT ON imported_notes BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        assert!(finish(&mut db, SERVER, "jim", &sent, &remote()).is_err());
        assert!(!linked(&db, 1).unwrap());
        assert!(begin(&mut db, SERVER, "jim", 1).is_err());
        let (snapshot, completed): (String, bool) = db
            .query_row(
                "SELECT server_snapshot,completed FROM note_creation_attempts WHERE local_id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<RemoteNote>(&snapshot).unwrap().id,
            701
        );
        assert!(!completed);
        assert_eq!(local(&db, 1).unwrap().content, "my text");
    }
    #[tokio::test]
    async fn rejects_failed_malformed_and_oversized_create_responses() {
        let mut db = setup();
        let sent = begin(&mut db, SERVER, "jim", 1).unwrap();
        for status in [302, 401, 423, 500] {
            let result =
                read_created(response(status, "private server message".into()), &sent).await;
            assert!(!result.err().unwrap().contains("private server"));
        }
        for body in ["not json".into(), " ".repeat(4 * 1024 * 1024 + 1)] {
            assert!(read_created(response(200, body), &sent).await.is_err());
        }
        let mut invalid = remote();
        invalid.id = 0;
        assert!(read_created(
            response(200, serde_json::to_string(&invalid).unwrap()),
            &sent
        )
        .await
        .is_err());
        invalid = remote();
        invalid.content = "truncated text".into();
        assert!(read_created(
            response(200, serde_json::to_string(&invalid).unwrap()),
            &sent
        )
        .await
        .is_err());
        assert!(begin(&mut db, SERVER, "jim", 1).is_err());
    }
    #[test]
    fn refuses_linked_notes_even_from_another_account() {
        let mut db = setup();
        db.execute("INSERT INTO imported_notes(server,account,remote_id,local_id,original_snapshot) VALUES ('https://other/','other',701,1,?1)",[serde_json::to_string(&remote()).unwrap()]).unwrap();
        assert!(begin(&mut db, SERVER, "jim", 1).is_err());
    }
}
