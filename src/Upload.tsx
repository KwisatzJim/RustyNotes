import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export function Upload({ id, title, onClose, onUpload }: { id: number; title: string; onClose: () => void; onUpload: (createNew: boolean) => Promise<void> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [attempted, setAttempted] = useState(false);
  const [createNew, setCreateNew] = useState<boolean | null>(null);
  useEffect(() => {
    let active = true;
    dialog.current?.showModal();
    invoke<boolean>("is_local_only_note", { id }).then((value) => { if (active) setCreateNew(value); })
      .catch(() => { if (active) setMessage("Could not check this note's upload status. Close and reopen to retry."); });
    return () => { active = false; };
  }, [id]);
  async function upload() {
    if (busy || attempted || createNew === null) return;
    setBusy(true);
    setAttempted(true);
    try {
      await onUpload(createNew);
      setMessage(createNew ? "New server note created and linked to this local note. Future uploads will update it. No other notes were uploaded." : "Server and local note confirmed in sync. No other notes were uploaded.");
    } catch (error) {
      setMessage(typeof error === "string" ? error : "Upload could not be confirmed. Check Nextcloud and refresh before retrying.");
    } finally { setBusy(false); }
  }
  return <dialog ref={dialog} className="settings-dialog" aria-labelledby="upload-title" onCancel={(event) => { if (busy) event.preventDefault(); else onClose(); }}>
    <h2 id="upload-title">Upload this note?</h2>
    <p><strong>{title || "Untitled"}</strong></p>
    {createNew === null ? <p>Checking this note’s upload status…</p> : createNew ? <>
      <p>This is a local-only note. Confirmation will create one new note on your authorized Nextcloud server, including its text, title, category, and favorite status. No other notes are uploaded or deleted.</p>
      <p>If creation cannot be confirmed, another creation attempt for this note is blocked—even after restarting—to prevent duplicates. You will need to check Nextcloud and review the result. Use a disposable note for this first test.</p>
    </> : <>
      <p>This will update this existing note on your authorized Nextcloud server, including its text, title, category, and favorite status. It will not upload other notes or create or delete server notes.</p>
      <p>If the server version has changed, the upload stops. The note must belong to the currently authorized account.</p>
    </>}
    {message && <p role="status">{message}</p>}
    <div className="settings-actions">
      <button disabled={busy} onClick={onClose}>{attempted ? "Close" : "Cancel"}</button>
      {!attempted && <button disabled={createNew === null} onClick={() => void upload()}>{createNew ? "Confirm create on Nextcloud" : "Confirm upload to Nextcloud"}</button>}
      {busy && <span role="status">Checking and uploading…</span>}
    </div>
  </dialog>;
}
