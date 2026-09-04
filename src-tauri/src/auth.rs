//! Login and credential access; secrets never cross the Tauri command boundary.
use reqwest::{redirect::Policy, Client, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::{Duration, Instant};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;
use url::Url;

const LOGIN_LIFETIME: Duration = Duration::from_secs(20 * 60);
const BODY_LIMIT: usize = 64 * 1024;

#[derive(Default)]
pub struct LoginState(Mutex<Option<LoginSession>>);

impl LoginState {
    pub(crate) async fn restore_backup(&self, prepared: crate::backup_preview::PreparedBackup) -> Result<String, String> {
        // Serialize against the same lock used by server-writing workflows.
        let guard = self.0.lock().await;
        if guard.is_some() { return Err("Cancel the pending Nextcloud login in Settings before restoring.".into()); }
        tauri::async_runtime::spawn_blocking(move || crate::restore::restore_prepared(&crate::db::database_path(), prepared))
            .await.map_err(|_| "Restore outcome could not be confirmed. Reload local notes before continuing.".to_string())?
    }
    pub(crate) async fn create_server_note(&self, id: i64) -> Result<(), String> {
        let guard = self.0.lock().await;
        if guard.is_some() { return Err("Finish or cancel login before uploading.".into()); }
        let server = crate::settings::get_server_url()?.ok_or("Save and authorize your server first.")?;
        let credentials = tokio::task::spawn_blocking(move || load_credentials(&server)).await
            .map_err(|_| "Could not read credential storage.".to_string())??.ok_or("Please authorize first.")?;
        let client=client()?;
        // Authenticate before persisting an attempt. This check cannot create notes.
        let check=crate::notes_api::check_request(client.clone(), &credentials.server, &credentials.login_name, &credentials.app_password)?.send().await
            .map_err(|_| "Could not reach Nextcloud. No new-note upload was attempted.".to_string())?;
        if check.status()!=StatusCode::OK { return Err(format!("Nextcloud check returned HTTP {}. No new-note upload was attempted.",check.status().as_u16())); }
        let (server,account)=(credentials.server.clone(),credentials.login_name.clone());
        let note=tokio::task::spawn_blocking(move || {
            let mut db=crate::db::open_database().map_err(|_| "Could not open local storage.".to_string())?;
            crate::create_remote::begin(&mut db,&server,&account,id)
        }).await.map_err(|_| "Could not prepare new-note upload.".to_string())??;
        let remote=crate::create_remote::send(client,&credentials.server,&credentials.login_name,&credentials.app_password,&note).await?;
        tokio::task::spawn_blocking(move || {
            let mut db=crate::db::open_database().map_err(|_| "Server creation succeeded but local confirmation failed. Check Nextcloud and refresh; do not create another copy.".to_string())?;
            crate::create_remote::finish(&mut db,&credentials.server,&credentials.login_name,&note,&remote)
        }).await.map_err(|_| "Local confirmation failed. Check Nextcloud and refresh before further uploads.".to_string())?
    }

    pub(crate) async fn upload_note(&self, id: i64) -> Result<(), String> {
        let guard = self.0.lock().await;
        if guard.is_some() { return Err("Finish or cancel login before uploading.".into()); }
        let server = crate::settings::get_server_url()?.ok_or("Save and authorize your server first.")?;
        let credentials = tokio::task::spawn_blocking(move || load_credentials(&server)).await
            .map_err(|_| "Could not read credential storage.".to_string())??.ok_or("Please authorize first.")?;
        let (server, account) = (credentials.server.clone(), credentials.login_name.clone());
        let pending = tokio::task::spawn_blocking(move || {
            let db = crate::db::open_database().map_err(|_| "Could not open local storage.".to_string())?;
            crate::upload::prepare(&db, &server, &account, id)
        }).await.map_err(|_| "Could not prepare upload.".to_string())??;
        let remote = crate::upload::send(client()?, &credentials.server, &credentials.login_name, &credentials.app_password, &pending).await?;
        tokio::task::spawn_blocking(move || {
            let mut db = crate::db::open_database().map_err(|_| "Server accepted the note, but local storage could not be opened. Refresh before retrying.".to_string())?;
            crate::upload::finish(&mut db, &credentials.server, &credentials.login_name, &pending, &remote)
        }).await.map_err(|_| "Local upload confirmation failed. Refresh before retrying.".to_string())?
    }

    pub(crate) async fn refresh_notes(&self) -> Result<crate::refresh::RefreshSummary, String> {
        let guard = self.0.lock().await;
        if guard.is_some() { return Err("Finish or cancel login before refreshing notes.".into()); }
        let server = crate::settings::get_server_url()?.ok_or("Save and authorize your server first.")?;
        let credentials = tokio::task::spawn_blocking(move || load_credentials(&server)).await
            .map_err(|_| "Could not read credential storage.".to_string())??
            .ok_or("No saved credentials. Please authorize first.")?;
        let notes = tokio::time::timeout(Duration::from_secs(120), crate::notes_api::download_notes(
            client()?, &credentials.server, &credentials.login_name, &credentials.app_password,
        )).await.map_err(|_| "Download timed out. No refresh changes were applied.".to_string())??;
        tokio::task::spawn_blocking(move || {
            let mut connection = crate::db::open_database().map_err(|_| "Could not open local storage for refresh.".to_string())?;
            crate::refresh::store_refresh(&mut connection, &credentials.server, &credentials.login_name, &notes)
        }).await.map_err(|_| "Refresh task failed. Restart to reload local notes before editing.".to_string())?
    }

    pub(crate) async fn import_notes(&self) -> Result<crate::import::ImportSummary, String> {
        // Keep server changes, reauthorization and concurrent imports serialized.
        let guard = self.0.lock().await;
        if guard.is_some() { return Err("Finish or cancel login before importing notes.".into()); }
        let server = crate::settings::get_server_url()?.ok_or("Save and authorize your server first.")?;
        let credentials = tokio::task::spawn_blocking(move || load_credentials(&server)).await
            .map_err(|_| "Could not read credential storage.".to_string())??
            .ok_or("No saved credentials. Please authorize first.")?;
        let notes = tokio::time::timeout(Duration::from_secs(120), crate::notes_api::download_notes(
            client()?, &credentials.server, &credentials.login_name, &credentials.app_password,
        )).await.map_err(|_| "Download timed out. Nothing was imported; please retry.".to_string())??;
        tokio::task::spawn_blocking(move || {
            let mut connection = crate::db::open_database().map_err(|_| "Could not open local storage for import.".to_string())?;
            crate::import::store_batch(&mut connection, &credentials.server, &credentials.login_name, &notes)
        }).await.map_err(|_| "Import task failed. Repeating the import is safe.".to_string())?
    }

    pub(crate) async fn prepare_notes_check(&self) -> Result<reqwest::RequestBuilder, String> {
        let guard = self.0.lock().await;
        if guard.is_some() {
            return Err("Finish or cancel login before checking the Notes API.".into());
        }
        let server = crate::settings::get_server_url()?.ok_or("Save and authorize your server first.")?;
        let credentials = tokio::task::spawn_blocking(move || load_credentials(&server)).await
            .map_err(|_| "Could not read credential storage.".to_string())??
            .ok_or("No saved credentials for this server. Please authorize first.")?;
        crate::notes_api::check_request(client()?, &credentials.server, &credentials.login_name, &credentials.app_password)
    }

    async fn cancel(&self) {
        *self.0.lock().await = None;
    }

    pub(crate) async fn save_address(&self, server_url: &str) -> Result<String, String> {
        let guard = self.0.lock().await;
        if guard.is_some() {
            return Err("Cancel the active login before changing the server address.".into());
        }
        crate::settings::persist_server_url(server_url)
    }
}

struct LoginSession {
    server: String,
    client: Client,
    poll: Poll,
    started: Instant,
    last_poll: Option<Instant>,
    // A successful poll can only be consumed once. Retain it if secure storage fails.
    received: Option<Credentials>,
}

impl LoginSession {
    fn expired(&self) -> bool {
        self.received.is_none() && self.started.elapsed() >= LOGIN_LIFETIME
    }
}

async fn finish_login<F, Fut>(session: &mut Option<LoginSession>, store: F) -> Result<Option<String>, String>
where
    F: FnOnce(Credentials) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let credentials = session.as_ref().and_then(|s| s.received.as_ref())
        .ok_or("Missing login result.")?.clone();
    let login_name = credentials.login_name.clone();
    store(credentials).await?;
    *session = None;
    Ok(Some(login_name))
}

#[derive(Deserialize)]
struct Poll {
    token: String,
    endpoint: String,
}

#[derive(Deserialize)]
struct Flow {
    login: String,
    poll: Poll,
}

// Deliberately no Debug implementation: these values must not reach logs.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Credentials {
    server: String,
    login_name: String,
    app_password: String,
}

#[derive(Serialize)]
pub struct LoginStatus {
    login_name: Option<String>,
    pending: bool,
}

fn trusted_endpoint(base: &Url, input: &str) -> Result<Url, String> {
    let url = Url::parse(input).map_err(|_| "Nextcloud returned an invalid login address.".to_string())?;
    if url.scheme() != "https" || url.origin() != base.origin()
        || !url.username().is_empty() || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Login stopped: Nextcloud returned an address outside your configured HTTPS server.".into());
    }
    Ok(url)
}

fn validate_flow(base: &Url, flow: &Flow) -> Result<(), String> {
    trusted_endpoint(base, &flow.login)?;
    trusted_endpoint(base, &flow.poll.endpoint)?;
    if flow.poll.token.is_empty() || flow.poll.token.len() > 8192 {
        return Err("Nextcloud returned an invalid polling token.".into());
    }
    Ok(())
}

fn validate_credentials(server: &str, credentials: &mut Credentials) -> Result<(), String> {
    let returned = crate::settings::normalize_server_url(&credentials.server)
        .map_err(|_| "Nextcloud returned an invalid credential server.".to_string())?;
    if returned != server {
        return Err("Login stopped: credentials belong to a different server address.".into());
    }
    if credentials.login_name.is_empty() || credentials.app_password.is_empty() {
        return Err("Nextcloud returned incomplete credentials.".into());
    }
    credentials.server = returned;
    Ok(())
}

fn client() -> Result<Client, String> {
    Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .no_proxy()
        .referer(false)
        .user_agent("RustyNotes/0.1.0")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|_| "Could not initialize secure networking.".to_string())
}

fn network_error(_: reqwest::Error) -> String {
    // reqwest errors may contain URLs with login tokens. Never forward them.
    "Could not reach Nextcloud securely. Check the address, connection, and HTTPS certificate, then retry.".into()
}

fn check_status(status: StatusCode, polling: bool) -> Result<bool, String> {
    if polling && status == StatusCode::NOT_FOUND { return Ok(false); }
    if status == StatusCode::OK { return Ok(true); }
    if status.is_redirection() {
        return Err("Nextcloud redirected the request. Save its final HTTPS base address and start login again.".into());
    }
    Err(format!("Nextcloud returned HTTP {}. Retry, or cancel and check your server address.", status.as_u16()))
}

async fn read_json<T: DeserializeOwned>(mut response: Response) -> Result<T, String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        if bytes.len() + chunk.len() > BODY_LIMIT {
            return Err("Nextcloud's login response was too large.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "Nextcloud returned an invalid login response.".into())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn storage_help() -> &'static str {
    if cfg!(target_os = "macos") {
        "Allow access to macOS Keychain."
    } else {
        "Enable and unlock a Secret Service-compatible desktop wallet (such as GNOME Keyring or KWallet) in your desktop session."
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn credential_entry(server: &str) -> Result<keyring::Entry, String> {
    // Preserve the existing macOS service/account identifiers exactly.
    keyring::Entry::new("com.rustynotes.nextcloud", server)
        .map_err(|_| format!("Could not access secure credential storage. {}", storage_help()))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn store_credentials(credentials: &Credentials) -> Result<(), String> {
    store_credentials_in(&credential_entry(&credentials.server)?, credentials)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn store_credentials_in(entry: &keyring::Entry, credentials: &Credentials) -> Result<(), String> {
    let secret = serde_json::to_string(credentials).map_err(|_| "Could not prepare credentials.".to_string())?;
    entry.set_password(&secret).map_err(|_| format!("Could not save credentials securely. {} Then choose Retry login check. Do not cancel or quit if you want to retry saving this authorization.", storage_help()))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn store_credentials(_: &Credentials) -> Result<(), String> {
    Err("Secure login storage is currently implemented only for macOS and Linux.".into())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn load_credentials(server: &str) -> Result<Option<Credentials>, String> {
    load_credentials_from(&credential_entry(server)?, server)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn load_credentials_from(entry: &keyring::Entry, server: &str) -> Result<Option<Credentials>, String> {
    match entry.get_password() {
        Ok(secret) => {
            let mut credentials: Credentials = serde_json::from_str(&secret)
                .map_err(|_| "Stored credentials could not be read. Please authorize again.".to_string())?;
            validate_credentials(server, &mut credentials)?;
            Ok(Some(credentials))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(format!("Could not read secure credential storage. {} Then reopen Settings.", storage_help())),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn load_credentials(_: &str) -> Result<Option<Credentials>, String> {
    Err("Secure credential storage is currently implemented only for macOS and Linux.".into())
}

fn stored_login(server: &str) -> Result<Option<String>, String> {
    load_credentials(server).map(|credentials| credentials.map(|value| value.login_name))
}

#[tauri::command]
pub async fn get_login_status(state: tauri::State<'_, LoginState>) -> Result<LoginStatus, String> {
    let guard = state.0.lock().await;
    if guard.is_some() {
        return Ok(LoginStatus { login_name: None, pending: true });
    }
    let login_name = if let Some(server) = crate::settings::get_server_url()? {
        tokio::task::spawn_blocking(move || stored_login(&server)).await
            .map_err(|_| "Could not read credential storage.".to_string())??
    } else { None };
    Ok(LoginStatus { login_name, pending: false })
}

#[tauri::command]
pub async fn begin_login(app: tauri::AppHandle, state: tauri::State<'_, LoginState>) -> Result<(), String> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return Err("Secure login storage is currently implemented only for macOS and Linux.".into());
    }
    let mut guard = state.0.lock().await;
    if guard.is_some() { return Err("A login is already active. Cancel it before starting again.".into()); }
    let server = crate::settings::get_server_url()?.ok_or("Save your server address first.")?;
    let server = crate::settings::normalize_server_url(&server)?;
    let base = Url::parse(&server).map_err(|_| "Invalid server address.".to_string())?;
    let endpoint = base.join("index.php/login/v2").map_err(|_| "Invalid login endpoint.".to_string())?;
    let client = client()?;
    let started = Instant::now();
    let response = client.post(endpoint).header("Accept", "application/json").send().await.map_err(network_error)?;
    check_status(response.status(), false)?;
    let flow: Flow = read_json(response).await?;
    validate_flow(&base, &flow)?;
    app.opener().open_url(flow.login, None::<&str>)
        .map_err(|_| "Could not open your browser. Please try again.".to_string())?;
    *guard = Some(LoginSession { server, client, poll: flow.poll, started, last_poll: None, received: None });
    Ok(())
}

#[tauri::command]
pub async fn poll_login(state: tauri::State<'_, LoginState>) -> Result<Option<String>, String> {
    let mut guard = state.0.lock().await;
    let session = guard.as_mut().ok_or("No active login. Cancel and start again.")?;
    if crate::settings::get_server_url()?.as_deref() != Some(&session.server) {
        return Err("Server address changed. Cancel and start login again.".into());
    }
    if session.received.is_none() {
        if session.expired() {
            return Err("Login expired after 20 minutes. Cancel and start again.".into());
        }
        if session.last_poll.is_some_and(|last| last.elapsed() < Duration::from_secs(1)) {
            return Ok(None);
        }
        session.last_poll = Some(Instant::now());
        let response = session.client.post(&session.poll.endpoint)
            .header("Accept", "application/json")
            .form(&[("token", session.poll.token.as_str())])
            .send().await.map_err(network_error)?;
        if !check_status(response.status(), true)? { return Ok(None); }
        let mut credentials: Credentials = read_json(response).await?;
        validate_credentials(&session.server, &mut credentials)?;
        session.received = Some(credentials);
    }
    finish_login(&mut guard, |credentials| async move {
        tokio::task::spawn_blocking(move || store_credentials(&credentials)).await
            .map_err(|_| "Credential storage failed. Retry login check.".to_string())?
    }).await
}

#[tauri::command]
pub async fn cancel_login(state: tauri::State<'_, LoginState>) -> Result<(), String> {
    state.cancel().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Entry-local mocks avoid touching the user's wallet or changing global builders.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn mock_entry() -> keyring::Entry {
        keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()))
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn secure_entry_round_trip_and_missing_entry() {
        let entry = mock_entry();
        let credentials = fake_session().received.unwrap();
        assert!(load_credentials_from(&entry, &credentials.server).unwrap().is_none());
        store_credentials_in(&entry, &credentials).unwrap();
        let loaded = load_credentials_from(&entry, &credentials.server).unwrap().unwrap();
        assert_eq!(loaded.server, credentials.server);
        assert_eq!(loaded.login_name, credentials.login_name);
        assert_eq!(loaded.app_password, credentials.app_password);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn stored_invalid_credentials_and_wrong_server_are_rejected() {
        let entry = mock_entry();
        let credentials = fake_session().received.unwrap();
        entry.set_password("malformed-secret-do-not-echo").unwrap();
        let error = load_credentials_from(&entry, &credentials.server).err().unwrap();
        assert!(!error.contains("malformed-secret"));
        store_credentials_in(&entry, &credentials).unwrap();
        assert!(load_credentials_from(&entry, "https://other.example.com/").is_err());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn wallet_errors_are_not_missing_entries_and_do_not_expose_secrets() {
        let entry = mock_entry();
        let credentials = fake_session().received.unwrap();
        let mock: &keyring::mock::MockCredential = entry.get_credential().downcast_ref().unwrap();
        mock.set_error(keyring::Error::Invalid("private-provider-detail".into(), "fake-password".into()));
        let error = store_credentials_in(&entry, &credentials).unwrap_err();
        assert!(error.contains("Retry login check"));
        assert!(!error.contains("private-provider-detail") && !error.contains("fake-password"));
        assert!(load_credentials_from(&entry, &credentials.server).unwrap().is_none());
        store_credentials_in(&entry, &credentials).unwrap();
        mock.set_error(keyring::Error::NoStorageAccess(Box::new(std::io::Error::other("private-provider-detail"))));
        let error = load_credentials_from(&entry, &credentials.server).err().unwrap();
        assert!(error.contains("reopen Settings"));
        assert!(!error.contains("private-provider-detail"));
        assert!(load_credentials_from(&entry, &credentials.server).unwrap().is_some());
    }

    fn fake_session() -> LoginSession {
        LoginSession {
            server: "https://cloud.example.com/".into(),
            client: client().unwrap(),
            poll: Poll { token: "fake-token".into(), endpoint: "https://cloud.example.com/poll".into() },
            started: Instant::now(), last_poll: None,
            received: Some(Credentials { server: "https://cloud.example.com/".into(), login_name: "jim".into(), app_password: "fake-password".into() }),
        }
    }

    #[tokio::test]
    async fn keychain_failure_retains_one_time_result_for_retry() {
        let mut session = Some(fake_session());
        let result = finish_login(&mut session, |_| async { Err("Keychain locked".into()) }).await;
        assert!(result.is_err());
        assert!(session.as_ref().unwrap().received.is_some());
        let result = finish_login(&mut session, |credentials| async move {
            assert_eq!(credentials.app_password, "fake-password");
            Ok(())
        }).await.unwrap();
        assert_eq!(result.as_deref(), Some("jim"));
        assert!(session.is_none());
    }

    #[tokio::test]
    async fn cancel_discards_pending_secrets_without_contacting_server() {
        let state = LoginState(Mutex::new(Some(fake_session())));
        state.cancel().await;
        assert!(state.0.lock().await.is_none());
    }

    #[tokio::test]
    async fn cannot_change_address_during_login() {
        let state = LoginState(Mutex::new(Some(fake_session())));
        // Rejection happens before any database access.
        assert!(state.save_address("https://other.example.com/").await.is_err());
    }

    #[test]
    fn timeout_does_not_discard_already_received_credentials() {
        let mut session = fake_session();
        session.started = Instant::now() - LOGIN_LIFETIME;
        assert!(!session.expired());
        session.received = None;
        assert!(session.expired());
    }

    #[tokio::test]
    async fn rejects_oversized_and_malformed_responses_without_echoing_body() {
        let response = Response::from(http::Response::new(reqwest::Body::from(vec![b'x'; BODY_LIMIT + 1])));
        assert_eq!(read_json::<Flow>(response).await.err().unwrap(), "Nextcloud's login response was too large.");
        let response = Response::from(http::Response::new(reqwest::Body::from("fake-secret malformed json")));
        let error = read_json::<Flow>(response).await.err().unwrap();
        assert!(!error.contains("fake-secret"));
    }

    #[test]
    fn only_same_https_origin_is_allowed() {
        let base = Url::parse("https://cloud.example.com/nextcloud/").unwrap();
        assert!(trusted_endpoint(&base, "https://cloud.example.com/nextcloud/login?state=abc").is_ok());
        for address in ["https://evil.example.com/poll", "http://cloud.example.com/poll", "https://cloud.example.com:444/poll", "https://user:secret@cloud.example.com/poll", "https://cloud.example.com/poll#fragment", "/poll"] {
            assert!(trusted_endpoint(&base, address).is_err());
        }
    }

    #[test]
    fn login_endpoint_preserves_subdirectory() {
        let base = Url::parse("https://cloud.example.com/nextcloud/").unwrap();
        assert_eq!(base.join("index.php/login/v2").unwrap().as_str(), "https://cloud.example.com/nextcloud/index.php/login/v2");
    }

    #[test]
    fn polling_404_means_wait_not_failure() {
        assert!(!check_status(StatusCode::NOT_FOUND, true).unwrap());
        assert!(check_status(StatusCode::NOT_FOUND, false).is_err());
        assert!(check_status(StatusCode::OK, true).unwrap());
        for status in [StatusCode::FOUND, StatusCode::UNAUTHORIZED, StatusCode::TOO_MANY_REQUESTS, StatusCode::INTERNAL_SERVER_ERROR] {
            assert!(check_status(status, true).is_err());
        }
    }

    #[test]
    fn credentials_must_match_server_and_have_both_secrets() {
        let server = "https://cloud.example.com/nextcloud/";
        let mut credentials = Credentials { server: server.trim_end_matches('/').into(), login_name: "jim".into(), app_password: "test-only".into() };
        assert!(validate_credentials(server, &mut credentials).is_ok());
        credentials.server = "https://evil.example.com/".into();
        assert!(validate_credentials(server, &mut credentials).is_err());
        credentials.server = "https://cloud.example.com/other/".into();
        assert!(validate_credentials(server, &mut credentials).is_err());
        credentials.server = server.into();
        credentials.app_password.clear();
        assert!(validate_credentials(server, &mut credentials).is_err());
    }

    #[test]
    fn validates_both_login_and_poll_destinations_before_opening_browser() {
        let base = Url::parse("https://cloud.example.com/").unwrap();
        let mut flow = Flow { login: "https://cloud.example.com/login".into(), poll: Poll { endpoint: "https://evil.example.com/poll".into(), token: "test-only".into() } };
        assert!(validate_flow(&base, &flow).is_err());
        flow.poll.endpoint = "https://cloud.example.com/poll".into();
        assert!(validate_flow(&base, &flow).is_ok());
        flow.poll.token.clear();
        assert!(validate_flow(&base, &flow).is_err());
    }
}
