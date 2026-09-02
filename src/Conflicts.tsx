import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import type { ConflictSummary, ResolutionChoice } from "./syncTypes";

interface Version { title: string; content: string; category: string; favorite: boolean }
interface Detail { local: Version; server: Version; resolution: string | null }
const choices: Record<ResolutionChoice, string> = { keep_local: "Keep my version", use_server: "Use saved server version", keep_both: "Keep both" };

export function Conflicts({ onClose, onResolve }: { onClose: () => void; onResolve: (id: number, choice: ResolutionChoice) => Promise<void> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [conflicts, setConflicts] = useState<ConflictSummary[]>([]);
  const [selected, setSelected] = useState("");
  const [detail, setDetail] = useState<Detail | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [choice, setChoice] = useState<ResolutionChoice | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    dialog.current?.showModal();
    invoke<ConflictSummary[]>("list_refresh_conflicts")
      .then((items) => { if (active) { setConflicts(items); setSelected(items[0] ? String(items[0].id) : ""); } })
      .catch(() => { if (active) setError("Could not load saved conflicts. Close and reopen to retry."); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);
  useEffect(() => {
    let active = true;
    setDetail(null);
    setChoice(null);
    if (!selected) return;
    setError("");
    invoke<Detail>("get_refresh_conflict", { id: Number(selected) })
      .then((value) => { if (active) setDetail(value); })
      .catch(() => { if (active) setError("Could not read this saved comparison."); });
    return () => { active = false; };
  }, [selected]);
  async function resolve() {
    if (!choice || busy || !detail || detail.resolution) return;
    setBusy(true); setError("");
    try {
      await onResolve(Number(selected), choice);
      setDetail(await invoke<Detail>("get_refresh_conflict", { id: Number(selected) }));
      setConflicts(await invoke<ConflictSummary[]>("list_refresh_conflicts"));
      setChoice(null);
    } catch (failure) { setError(typeof failure === "string" ? failure : "Could not resolve this comparison. Please reopen it before retrying."); }
    finally { setBusy(false); }
  }
  return (
    <dialog ref={dialog} className="settings-dialog conflict-dialog" aria-labelledby="conflict-title" onCancel={(event) => { if (busy) event.preventDefault(); else onClose(); }}>
      <h2 id="conflict-title">Saved conflicts</h2>
      <p>These are preserved snapshots from refresh—not live server data. Choices affect local notes only. Both snapshots remain saved; nothing is uploaded.</p>
      {error && <p role="alert">{error}</p>}
      {loading ? <p role="status">Loading…</p> : conflicts.length === 0 ? <p>No saved conflicts.</p> : (
        <>
          <label htmlFor="conflict-choice">Choose a saved comparison</label>
          <select id="conflict-choice" value={selected} disabled={busy} onChange={(event) => setSelected(event.target.value)}>
            {conflicts.map((item) => <option key={item.id} value={item.id}>{item.title || "Untitled"} — comparison {item.id}{item.resolution ? " (resolved/history)" : " (unresolved)"}</option>)}
          </select>
          {!detail && !error && <p role="status">Loading comparison…</p>}
          {detail && <div className="conflict-columns">{(["local", "server"] as const).map((side) => (
            <section key={side}>
              <h3>{side === "local" ? "Your local version" : "Server version"}</h3>
              <strong>{detail[side].title}</strong>
              <p>Category: {detail[side].category || "Uncategorized"} · Favorite: {detail[side].favorite ? "Yes" : "No"}</p>
              <pre>{detail[side].content}</pre>
            </section>
          ))}</div>}
          {detail?.resolution && <p role="status">Recorded choice: {detail.resolution === "superseded" ? "Superseded by a newer resolved comparison" : choices[detail.resolution as ResolutionChoice] ?? detail.resolution}. The comparison remains available.</p>}
          {detail && !detail.resolution && <>
            <div className="settings-actions">{(Object.keys(choices) as ResolutionChoice[]).map((value) => <button key={value} disabled={busy} onClick={() => setChoice(value)}>{choices[value]}</button>)}</div>
            {choice && <div>
              <p>{choice === "keep_local" ? "Keep the original local note and acknowledge this server snapshot. Your version is not uploaded." : choice === "use_server" ? "Replace the local note with the server snapshot shown above. Your previous local version remains in this saved comparison." : "Keep the original local note and create a separate local-only note named with ‘(server copy)’ containing the server snapshot."}</p>
              <div className="settings-actions"><button disabled={busy} onClick={() => setChoice(null)}>Cancel choice</button><button disabled={busy} onClick={() => void resolve()}>{busy ? "Saving…" : "Confirm local resolution"}</button></div>
            </div>}
          </>}
        </>
      )}
      <div className="settings-actions"><button disabled={busy} onClick={onClose}>Close</button></div>
    </dialog>
  );
}
