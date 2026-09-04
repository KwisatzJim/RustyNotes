import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface BackupPreview { path: string; token: string; notes: number; trashed_notes: number }

export function Backup({ onClose, onBackup, onRestore }: { onClose: () => void; onBackup: () => Promise<string | null>; onRestore: (token: string) => Promise<string> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [preview, setPreview] = useState<BackupPreview | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [openingFolder, setOpeningFolder] = useState(false);
  const [folderError, setFolderError] = useState("");
  const folderRunning = useRef(false);
  const running = useRef(false);
  useEffect(() => { dialog.current?.showModal(); }, []);
  async function openDataFolder() {
    if (running.current || folderRunning.current) return;
    folderRunning.current = true;
    setOpeningFolder(true); setFolderError("");
    try { await invoke("open_local_data_folder"); }
    catch (failure) { setFolderError(typeof failure === "string" ? failure : "Could not open the data folder."); }
    finally { folderRunning.current = false; setOpeningFolder(false); }
  }
  async function save() {
    if (running.current) return;
    running.current = true;
    setBusy(true); setMessage(""); setError(""); setPreview(null); setConfirmed(false);
    try {
      const path = await onBackup();
      setMessage(path === null ? "Backup canceled. No file was written." : `Local backup saved and verified: ${path}`);
    } catch (failure) {
      setError(typeof failure === "string" ? failure : "Backup could not be confirmed. Check your chosen folder.");
    } finally { running.current = false; setBusy(false); }
  }
  async function inspect() {
    if (running.current) return;
    running.current = true;
    setBusy(true); setMessage(""); setError(""); setPreview(null); setConfirmed(false);
    try {
      const result = await invoke<BackupPreview | null>("preview_local_backup");
      setPreview(result);
      if (result === null) setMessage("Preview canceled. Nothing was restored.");
    } catch (failure) {
      setError(typeof failure === "string" ? failure : "Could not preview this backup. Nothing was restored.");
    } finally { running.current = false; setBusy(false); }
  }
  async function restore() {
    if (running.current || !preview || !confirmed) return;
    running.current = true;
    setBusy(true); setMessage(""); setError("");
    try {
      const safety = await onRestore(preview.token);
      setMessage(`Local data restored. Nextcloud and keyring credentials were not changed. Safety backup of your previous local data: ${safety}`);
    } catch (failure) {
      setError(typeof failure === "string" ? failure : "Restore could not be confirmed. Reload local notes before continuing.");
    } finally {
      setPreview(null); setConfirmed(false); running.current = false; setBusy(false);
    }
  }
  return <dialog ref={dialog} className="settings-dialog backup-dialog" aria-labelledby="backup-title" onCancel={event => { if (busy) event.preventDefault(); else onClose(); }}>
    <h2 id="backup-title">Local backup and restore</h2>
    <p>Saves all local notes, categories, favorites, Trash, settings, saved conflicts, and sync history in one SQLite backup. Pending edits are saved first.</p>
    <p>Passwords and keyring credentials are not included. Creating or previewing a backup does not change your notes or Nextcloud server.</p>
    <p>Backups are not encrypted. Store them somewhere private. For a new backup, choose a new filename outside the app’s data folder; existing files will not be replaced.</p>
    <p>You can also preview an existing backup (up to 128 MiB). Preview checks integrity and table structure and shows counts only. It does not restore or change data.</p>
    {busy && <p role="status">Working… Complete or cancel the native file dialog, then wait for verification.</p>}
    {preview && <div role="status">
      <p style={{ overflowWrap: "anywhere" }}>{preview.path}</p>
      <p>Integrity and structure checks passed.</p>
      <p>Notes: <strong>{preview.notes}</strong> · Local Trash: <strong>{preview.trashed_notes}</strong></p>
      <p>Nothing was restored. Your current notes and server are unchanged.</p>
      <h3>Restore this backup?</h3>
      <p>This replaces all current local notes, Trash, settings, and sync history with the selected backup. It does not merge notes or change Nextcloud. A verified safety backup of current data is saved in a private before-restore folder beside your live database first.</p>
      <p>Close any other RustyNotes copies using this data. After restore, review the server address and Refresh before uploading old local versions.</p>
      <label><input type="checkbox" checked={confirmed} disabled={busy} onChange={event => setConfirmed(event.target.checked)} /> I understand this replaces my local data, and other RustyNotes copies are closed.</label>
      <div className="settings-actions"><button disabled={busy || !confirmed} onClick={() => void restore()}>Restore this backup</button></div>
    </div>}
    {message && <p role="status" style={{ overflowWrap: "anywhere" }}>{message}</p>}
    {error && <p role="alert">{error}</p>}
    <p>Safety backups are in <code>before-restore-…</code> folders inside your data folder. Open it below to find them, even when Library is hidden. Do not move or edit the live <code>rustynotes.db</code> file.</p>
    {folderError && <p role="alert">{folderError}</p>}
    <div className="settings-actions">
      <button disabled={busy || openingFolder} onClick={() => void openDataFolder()}>{openingFolder ? "Opening folder…" : "Open data folder"}</button>
      <button disabled={busy} onClick={() => void save()}>Choose backup destination…</button>
      <button disabled={busy} onClick={() => void inspect()}>Preview backup…</button>
      <button disabled={busy} onClick={onClose}>Close</button>
    </div>
  </dialog>;
}
