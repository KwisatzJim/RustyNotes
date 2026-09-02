import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

export interface TrashedNote {
  id: number; title: string; content: string; category: string;
  favorite: boolean; modified_at: number;
}

export function Trash({ onClose, onRestore }: { onClose: () => void; onRestore: (id: number) => Promise<void> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const locked = useRef(false);
  const [notes, setNotes] = useState<TrashedNote[]>([]);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  useEffect(() => {
    dialog.current?.showModal();
    let canceled = false;
    void invoke<TrashedNote[]>("list_trashed_notes").then((result) => {
      if (!canceled) setNotes(result);
    }).catch(() => { if (!canceled) setError("Could not load Trash. Close and try again."); })
      .finally(() => { if (!canceled) setBusy(false); });
    return () => { canceled = true; };
  }, []);
  async function restore(note: TrashedNote) {
    if (locked.current) return;
    locked.current = true;
    setBusy(true); setError(""); setMessage("");
    try {
      await onRestore(note.id);
      setNotes((current) => current.filter((item) => item.id !== note.id));
      setMessage(`Restored “${note.title}”. Close to view it. Nextcloud was not changed.`);
    } catch (failure) {
      setError(typeof failure === "string" ? failure : "Restore could not be confirmed. Close and check your local notes before trying again.");
    } finally { locked.current = false; setBusy(false); }
  }
  return <dialog ref={dialog} className="settings-dialog" aria-labelledby="trash-title" onCancel={(event) => { if (busy) event.preventDefault(); else onClose(); }}>
    <h2 id="trash-title">Local Trash</h2>
    <p>Notes here are kept on this computer. Restore brings back their text, category, favorite status, and existing sync association. Nothing is deleted from or uploaded to Nextcloud.</p>
    <p>Notes permanently deleted before this feature cannot be recovered here.</p>
    {busy && <p role="status">Working…</p>}
    {!busy && !error && notes.length === 0 && <p>Trash is empty.</p>}
    {notes.map((note) => <details key={note.id}>
      <summary>{note.title || "Untitled"} — {note.category}</summary>
      <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere", maxHeight: "12rem", overflow: "auto" }}>{note.content}</pre>
      <button disabled={busy} onClick={() => void restore(note)}>Restore {note.title || "Untitled"}</button>
    </details>)}
    {message && <p role="status">{message}</p>}
    {error && <p role="alert">{error}</p>}
    <div className="settings-actions"><button disabled={busy} onClick={onClose}>Close</button></div>
  </dialog>;
}
