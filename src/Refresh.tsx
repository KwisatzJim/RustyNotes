import { useEffect, useRef, useState } from "react";
import type { RefreshSummary } from "./syncTypes";

export function Refresh({ onClose, onRefresh }: { onClose: () => void; onRefresh: () => Promise<RefreshSummary> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const started = useRef(false);
  const [busy, setBusy] = useState(true);
  const [summary, setSummary] = useState<RefreshSummary | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    dialog.current?.showModal();
    // React may rerun effects in development. Start only one refresh.
    if (started.current) return;
    started.current = true;
    void onRefresh().then(setSummary).catch((failure: unknown) => {
      setError(typeof failure === "string" ? failure : "Refresh could not be confirmed. Close and try again.");
    }).finally(() => setBusy(false));
  }, [onRefresh]);
  return <dialog ref={dialog} className="settings-dialog" aria-labelledby="refresh-title" onCancel={(event) => { if (busy) event.preventDefault(); else onClose(); }}>
    <h2 id="refresh-title">Refresh from Nextcloud</h2>
    <p>Download only. Local edits are protected; nothing is uploaded or deleted from your server.</p>
    {busy && <p role="status">Saving pending edits and refreshing…</p>}
    {error && <p role="alert">{error}</p>}
    {summary && <p role="status">Refresh complete: {summary.added} added, {summary.updated} updated, {summary.unchanged} unchanged, {summary.local_preserved} local edits kept, {summary.conflicts} conflicts preserved. {summary.locally_deleted} locally deleted copies skipped; {summary.server_missing} notes missing from the server kept locally. Your server was not changed.</p>}
    <div className="settings-actions"><button disabled={busy} onClick={onClose}>Close</button></div>
  </dialog>;
}
