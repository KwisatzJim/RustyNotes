import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import type { RefreshSummary } from "./syncTypes";

interface LoginStatus { login_name: string | null; pending: boolean }
interface ImportSummary { added: number; skipped: number }

export function Settings({ onClose, onImported, onRefresh }: { onClose: () => void; onImported: () => Promise<void>; onRefresh: () => Promise<RefreshSummary> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [serverUrl, setServerUrl] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const [loginName, setLoginName] = useState<string | null>(null);
  const [loginState, setLoginState] = useState<"idle" | "starting" | "waiting" | "checking" | "connected">("idle");
  const [canceling, setCanceling] = useState(false);
  const [checkingNotes, setCheckingNotes] = useState(false);
  const [noteCount, setNoteCount] = useState<number | null>(null);
  const [importing, setImporting] = useState(false);
  const [importSummary, setImportSummary] = useState<ImportSummary | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshSummary, setRefreshSummary] = useState<RefreshSummary | null>(null);
  const networkBusy = checkingNotes || importing || refreshing;
  const loginActive = loginState === "starting" || loginState === "waiting" || loginState === "checking";

  useEffect(() => {
    let active = true;
    dialog.current?.showModal();
    invoke<string | null>("get_server_url")
      .then(async (value) => {
        if (!active) return;
        setServerUrl(value ?? "");
        const status = await invoke<LoginStatus>("get_login_status");
        if (active) {
          setLoginName(status.login_name);
          setLoginState(status.pending ? "waiting" : status.login_name ? "connected" : "idle");
        }
      })
      .catch((failure) => { if (active) setError(typeof failure === "string" ? failure : "Could not load settings. Close this panel and try again."); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    if (loginState !== "waiting" || error || canceling) return;
    const timer = window.setTimeout(() => { void checkLogin(); }, 2000);
    return () => window.clearTimeout(timer);
  }, [loginState, error, canceling]);

  async function save() {
    if (networkBusy || loginActive || saving) return;
    setNoteCount(null);
    setImportSummary(null);
    setRefreshSummary(null);
    setSaving(true);
    setSaved(false);
    setError("");
    try {
      const normalized = await invoke<string>("save_server_url", { serverUrl });
      setServerUrl(normalized);
      setSaved(true);
      const status = await invoke<LoginStatus>("get_login_status");
      setLoginName(status.login_name);
      setLoginState(status.pending ? "waiting" : status.login_name ? "connected" : "idle");
    } catch (failure) {
      setError(typeof failure === "string" ? failure : "Could not save settings.");
    } finally {
      setSaving(false);
    }
  }

  async function beginLogin() {
    if (networkBusy) return;
    setNoteCount(null);
    setImportSummary(null);
    setRefreshSummary(null);
    setError("");
    setLoginState("starting");
    try {
      const normalized = await invoke<string>("save_server_url", { serverUrl });
      setServerUrl(normalized);
      await invoke("begin_login");
      setLoginState("waiting");
    } catch (failure) {
      setLoginState(loginName ? "connected" : "idle");
      setError(typeof failure === "string" ? failure : "Could not start login.");
    }
  }

  async function checkLogin() {
    setError("");
    setLoginState("checking");
    try {
      const login = await invoke<string | null>("poll_login");
      if (login) { setLoginName(login); setLoginState("connected"); }
      else setLoginState("waiting");
    } catch (failure) {
      setLoginState("waiting");
      setError(typeof failure === "string" ? failure : "Could not check login.");
    }
  }

  async function close() {
    if (loginState === "starting" || loginState === "checking" || canceling || networkBusy || saving) return;
    setCanceling(true);
    try {
      if (loginState === "waiting") await invoke("cancel_login");
      onClose();
    } catch {
      setError("Could not cancel login. Please try again.");
      setCanceling(false);
    }
  }

  async function checkNotes() {
    if (networkBusy || loginState !== "connected" || saving) return;
    setCheckingNotes(true);
    setNoteCount(null);
    setError("");
    try {
      setNoteCount(await invoke<number>("check_notes_connection"));
    } catch (failure) {
      setError(typeof failure === "string" ? failure : "Could not check the Notes API.");
    } finally {
      setCheckingNotes(false);
    }
  }

  async function importNotes() {
    if (networkBusy || loginState !== "connected" || saving) return;
    setImporting(true);
    setImportSummary(null);
    setRefreshSummary(null);
    setNoteCount(null);
    setError("");
    try {
      const summary = await invoke<ImportSummary>("import_server_notes");
      setImportSummary(summary);
      try { await onImported(); }
      catch { setError("Import saved locally, but the note list could not refresh. Restart RustyNotes to view it."); }
    } catch (failure) {
      setError(typeof failure === "string" ? failure : "Import failed. Repeating the import is safe.");
    } finally {
      setImporting(false);
    }
  }

  async function refreshNotes() {
    if (networkBusy || loginState !== "connected" || saving) return;
    setRefreshing(true);
    setRefreshSummary(null);
    setImportSummary(null);
    setNoteCount(null);
    setError("");
    try { setRefreshSummary(await onRefresh()); }
    catch (failure) { setError(typeof failure === "string" ? failure : "Could not complete the safe refresh. Close Settings and verify local notes before retrying."); }
    finally { setRefreshing(false); }
  }

  return (
    <dialog ref={dialog} className="settings-dialog" aria-labelledby="settings-title" onCancel={(event) => { event.preventDefault(); void close(); }}>
      <form onSubmit={(event) => { event.preventDefault(); void save(); }}>
        <h2 id="settings-title">Nextcloud settings</h2>
        <p>Your notes stay available offline. Login uses your default browser and stores only an app-specific password in macOS Keychain.</p>
        <label htmlFor="server-url">Nextcloud server address</label>
        <input id="server-url" type="text" inputMode="url" autoCapitalize="none" spellCheck={false}
          placeholder="https://cloud.example.com" value={serverUrl} disabled={loading || saving || loginActive || canceling || networkBusy}
          aria-describedby="server-url-help" onChange={(event) => {
            setServerUrl(event.target.value); setSaved(false); setError(""); setLoginName(null); setLoginState("idle"); setNoteCount(null); setImportSummary(null); setRefreshSummary(null);
          }} />
        <p id="server-url-help">Use your HTTPS base address, including a subfolder if needed (for example /nextcloud). Do not enter a password or a Notes-page URL.</p>
        {error && <p role="alert">{error}</p>}
        <p role="status">{loading ? "Loading…" : loginState === "connected" ? `Credentials saved for ${loginName}. Note synchronization is not enabled yet.` : loginState === "starting" ? "Contacting your Nextcloud server…" : loginState === "checking" ? "Checking authorization…" : loginState === "waiting" ? "Finish authorizing RustyNotes in your browser. This window checks automatically." : saved ? "Address saved locally. Ready to authorize." : "Save or authorize this server. No note synchronization occurs yet."}</p>
        {!loading && loginState !== "waiting" && loginState !== "checking" && (
          <button className="connect-button" type="button" disabled={saving || networkBusy || loginState === "starting" || !serverUrl.trim()} onClick={() => void beginLogin()}>
            {loginState === "starting" ? "Starting…" : loginState === "connected" ? "Authorize again" : "Authorize with Nextcloud"}
          </button>
        )}
        {(loginState === "waiting" || loginState === "checking") && (
          <button className="connect-button" type="button" disabled={loginState === "checking" || canceling} onClick={() => void checkLogin()}>
            {loginState === "checking" ? "Checking…" : "Retry login check"}
          </button>
        )}
        {loginActive && <p>Cancel stops local login checks. If you already granted access, you can revoke the RustyNotes app password in Nextcloud’s Security settings.</p>}
        {loginState === "connected" && (
          <>
            <button className="connect-button" type="button" disabled={networkBusy || saving || loading} onClick={() => void checkNotes()}>
              {checkingNotes ? "Checking Notes API…" : "Check Notes connection"}
            </button>
            <p>Read-only check: counts server notes without importing or changing them.</p>
            <button className="connect-button" type="button" disabled={networkBusy || saving || loading} onClick={() => void importNotes()}>
              {importing ? "Downloading and importing…" : "Import server notes (download only)"}
            </button>
            <p>Adds new local copies for offline use. Previously imported notes, local edits, and local deletions are left untouched. No uploads or server changes.</p>
            <button className="connect-button" type="button" disabled={networkBusy || saving || loading} onClick={() => void refreshNotes()}>
              {refreshing ? "Refreshing safely…" : "Refresh from Nextcloud (download only)"}
            </button>
            <p>Updates unchanged local copies. Local edits are kept; if both versions changed, a comparison is saved under Saved conflicts. No uploads or deletions.</p>
          </>
        )}
        {noteCount !== null && <p role="status">Notes API connected — {noteCount} {noteCount === 1 ? "note" : "notes"} found on the server. Nothing was imported or changed.</p>}
        {importing && <p role="status">Downloading notes. Nothing is saved until the complete download is validated; this may take up to two minutes.</p>}
        {importSummary && <p role="status">Import complete: {importSummary.added} added, {importSummary.skipped} previously imported notes skipped. Your server was not changed.</p>}
        {refreshing && <p role="status">Waiting for local saves, then downloading a complete snapshot. This may take up to two minutes.</p>}
        {refreshSummary && <p role="status">Refresh complete: {refreshSummary.added} added, {refreshSummary.updated} updated, {refreshSummary.unchanged} unchanged, {refreshSummary.local_preserved} local edits kept, {refreshSummary.conflicts} conflicts preserved. {refreshSummary.locally_deleted} locally deleted copies skipped; {refreshSummary.server_missing} notes missing from the server kept locally. Your server was not changed.</p>}
        <div className="settings-actions">
          <button type="button" disabled={loginState === "starting" || loginState === "checking" || canceling || saving || networkBusy} onClick={() => void close()}>{canceling ? "Canceling…" : loginState === "waiting" ? "Cancel login" : "Close"}</button>
          <button type="submit" disabled={loading || saving || networkBusy || loginState === "starting" || loginState === "waiting" || loginState === "checking" || !serverUrl.trim()}>{saving ? "Saving…" : "Save address"}</button>
        </div>
      </form>
    </dialog>
  );
}
