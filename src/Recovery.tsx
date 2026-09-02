import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

interface Version { id: number; title: string; content: string; category: string; favorite: boolean }
interface Item { local_id: number; title: string; server: string; account: string }
interface Candidate { copy: Version; remote: Version; token: string }
interface Review { local: Version; sent: Version; server: string; account: string; candidates: Candidate[] }

export function Recovery({ onClose, onRecover }: { onClose: () => void; onRecover: (id: number, copyId: number, token: string) => Promise<void> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [items, setItems] = useState<Item[]>([]);
  const [selected, setSelected] = useState("");
  const [review, setReview] = useState<Review | null>(null);
  const [copyId, setCopyId] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  useEffect(() => {
    let active = true;
    dialog.current?.showModal();
    invoke<Item[]>("list_creation_recoveries").then((value) => {
      if (active) { setItems(value); setSelected(value[0] ? String(value[0].local_id) : ""); }
    }).catch(() => { if (active) setError("Could not load interrupted uploads. Close and reopen to retry."); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);
  useEffect(() => {
    let active = true;
    setReview(null); setCopyId(""); setConfirmed(false);
    if (!selected) return;
    setError("");
    invoke<Review>("get_creation_recovery", { id: Number(selected) }).then((value) => { if (active) setReview(value); })
      .catch((failure: unknown) => { if (active) setError(typeof failure === "string" ? failure : "Could not load comparison."); });
    return () => { active = false; };
  }, [selected]);
  const candidate = review?.candidates.find((value) => String(value.copy.id) === copyId);
  async function recover() {
    if (!review || !candidate || !confirmed || busy) return;
    setBusy(true); setError(""); setMessage("");
    try {
      await onRecover(review.local.id, candidate.copy.id, candidate.token);
      setMessage("Link recovered. The original local note now updates that server note. The downloaded copy remains as a separate local-only note. No text was changed and nothing was uploaded or deleted.");
      setSelected(""); setReview(null); setConfirmed(false);
      setItems(await invoke<Item[]>("list_creation_recoveries"));
    } catch (failure) { setError(typeof failure === "string" ? failure : "Recovery could not be confirmed. Reopen this screen before retrying."); }
    finally { setBusy(false); }
  }
  return <dialog ref={dialog} className="settings-dialog conflict-dialog" aria-labelledby="recovery-title" onCancel={(event) => { if (busy) event.preventDefault(); else onClose(); }}>
    <h2 id="recovery-title">Recover interrupted uploads</h2>
    <p>This screen uses downloaded snapshots, not live server data. Close and use Refresh first to download possible server copies. Nothing here creates or deletes a server note.</p>
    {loading ? <p role="status">Loading…</p> : !items.length && !error ? <p>No interrupted uploads need recovery.</p> : items.length > 0 && <>
      <label htmlFor="recovery-attempt">Interrupted upload</label>
      <select id="recovery-attempt" disabled={busy} value={selected} onChange={(event) => { setSelected(event.target.value); setMessage(""); }}>
        <option value="">Choose an upload…</option>
        {items.map((item) => <option key={item.local_id} value={item.local_id}>{item.title || "Untitled"} — {item.account} @ {item.server}</option>)}
      </select>
    </>}
    {selected && !review && !error && <p role="status">Loading comparison…</p>}
    {review && <>
      <p>Server: {review.server} · Account: {review.account}</p>
      <div className="conflict-columns">{[{ label: "Current original local note", value: review.local }, { label: "Text sent during the interrupted upload", value: review.sent }].map(({ label, value }) => <section key={label}><h3>{label}</h3><strong>{value.title}</strong><p>Category: {value.category || "Uncategorized"} · Favorite: {value.favorite ? "Yes" : "No"}</p><pre>{value.content}</pre></section>)}</div>
      {!review.candidates.length ? <p>No eligible downloaded copy was found. Refresh first. Copies with local edits or unresolved conflicts are excluded. If the server text changed and no server ID was saved, recovery cannot safely suggest a match. The creation block stays in place.</p> : <>
        <label htmlFor="recovery-copy">Possible server copy—verify its identity yourself</label>
        <select id="recovery-copy" disabled={busy} value={copyId} onChange={(event) => { setCopyId(event.target.value); setConfirmed(false); }}>
          <option value="">Choose a server copy…</option>
          {review.candidates.map((value) => <option key={value.copy.id} value={value.copy.id}>{value.remote.title || "Untitled"} — server ID {value.remote.id}, local copy {value.copy.id}</option>)}
        </select>
      </>}
      {candidate && <>
        <section><h3>Downloaded server snapshot — ID {candidate.remote.id}</h3><strong>{candidate.remote.title}</strong><p>Category: {candidate.remote.category || "Uncategorized"} · Favorite: {candidate.remote.favorite ? "Yes" : "No"}</p><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere", maxHeight: "200px", overflow: "auto" }}>{candidate.remote.content}</pre></section>
        <p>Matching text alone does not prove identity. This transfers the server link from local copy {candidate.copy.id} to original note {review.local.id}. Both local texts remain unchanged; the extra copy becomes local-only. Future explicit uploads from the original note can update this server note.</p>
        <label><input type="checkbox" checked={confirmed} disabled={busy} onChange={(event) => setConfirmed(event.target.checked)} /> I verified this is the server copy of this interrupted upload.</label>
        <div className="settings-actions"><button disabled={busy || !confirmed} onClick={() => void recover()}>{busy ? "Recovering…" : "Confirm local link recovery"}</button></div>
      </>}
    </>}
    {error && <p role="alert">{error}</p>}
    {message && <p role="status">{message}</p>}
    <div className="settings-actions"><button disabled={busy} onClick={onClose}>Close</button></div>
  </dialog>;
}
