import { useEffect, useRef, useState } from "react";

export function Export({ title, onClose, onExport }: { title: string; onClose: () => void; onExport: () => Promise<string | null> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const started = useRef(false);
  const [busy, setBusy] = useState(true);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    dialog.current?.showModal();
    if (started.current) return;
    started.current = true;
    void onExport().then((path) => {
      setMessage(path === null ? "Export canceled. No file was written." : `Markdown exported to ${path}`);
    }).catch((failure: unknown) => {
      setError(typeof failure === "string" ? failure : "Export could not be confirmed. Check your chosen folder.");
    }).finally(() => setBusy(false));
  }, [onExport]);
  return <dialog ref={dialog} className="settings-dialog" aria-labelledby="export-title" onCancel={(event) => { if (busy) event.preventDefault(); else onClose(); }}>
    <h2 id="export-title">Export Markdown</h2>
    <p><strong>{title || "Untitled"}</strong></p>
    <p>Exports the note’s text exactly as plain Markdown. The title suggests the filename; categories and favorites are not added to the file. Your note and server are unchanged.</p>
    {busy && <p role="status">Saving pending edits. Choose a destination in the Save dialog, or cancel it.</p>}
    {message && <p role="status" style={{ overflowWrap: "anywhere" }}>{message}</p>}
    {error && <p role="alert">{error}</p>}
    <div className="settings-actions"><button disabled={busy} onClick={onClose}>Close</button></div>
  </dialog>;
}
