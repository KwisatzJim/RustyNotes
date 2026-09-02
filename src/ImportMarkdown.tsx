import { useEffect, useRef, useState } from "react";

export function ImportMarkdown({ onClose, onImport }: { onClose: () => void; onImport: () => Promise<string | null> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const started = useRef(false);
  const [busy, setBusy] = useState(true);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    dialog.current?.showModal();
    if (started.current) return;
    started.current = true;
    void onImport().then((title) => {
      setMessage(title === null ? "Import canceled. No note was added." : `Imported “${title}” as a new local note in Personal. Close to view it. Nothing was uploaded.`);
    }).catch((failure: unknown) => {
      setError(typeof failure === "string" ? failure : "Import could not be confirmed. Check your local notes before importing again.");
    }).finally(() => setBusy(false));
  }, [onImport]);
  return <dialog ref={dialog} className="settings-dialog" aria-labelledby="import-title" onCancel={(event) => { if (busy) event.preventDefault(); else onClose(); }}>
    <h2 id="import-title">Import Markdown</h2>
    <p>Choose one UTF-8 .md file, up to 4 MiB. The filename becomes the title; its text is kept unchanged. Each import adds a separate local note, even if that filename already exists.</p>
    <p>The original file, existing notes, and Nextcloud are not changed.</p>
    {busy && <p role="status">Choose a file in the Open dialog, or cancel it.</p>}
    {message && <p role="status">{message}</p>}
    {error && <p role="alert">{error}</p>}
    <div className="settings-actions"><button disabled={busy} onClick={onClose}>Close</button></div>
  </dialog>;
}
