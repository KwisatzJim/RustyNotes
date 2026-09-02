import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

interface NoteIdentity { id: number }
interface Status {
  kind: "local_only" | "local_changes" | "matches_snapshot" | "conflict_saved" | "recovery_needed";
  server: string | null;
  account: string | null;
}
const labels: Record<Status["kind"], string> = {
  local_only: "Local only",
  local_changes: "Local changes",
  matches_snapshot: "Matches last server snapshot",
  conflict_saved: "Conflict saved",
  recovery_needed: "Upload recovery needed",
};
const explanations: Record<Status["kind"], string> = {
  local_only: "This note has no server association. Upload requires explicit confirmation.",
  local_changes: "This note differs from its saved server snapshot. Changes are not automatically uploaded.",
  matches_snapshot: "Matches the last saved server snapshot—not a live server check. Refresh to check for newer server changes.",
  conflict_saved: "An unresolved comparison is saved. Open Saved conflicts to review it.",
  recovery_needed: "A new-note upload was not confirmed. Open Recover uploads before attempting another creation.",
};

export function NoteSyncStatus({ note, saving, paused, saveFailed, onConflicts, onRecovery }: {
  note: NoteIdentity; saving: boolean; paused: boolean; saveFailed: boolean;
  onConflicts: () => void; onRecovery: () => void;
}) {
  const [result, setResult] = useState<{ note: NoteIdentity; status: Status | null } | null>(null);
  useEffect(() => {
    let active = true;
    setResult(null);
    if (saving || paused || saveFailed) return;
    void invoke<Status>("get_note_sync_status", { id: note.id })
      .then((status) => { if (active) setResult({ note, status }); })
      .catch(() => { if (active) setResult({ note, status: null }); });
    return () => { active = false; };
  }, [note, saving, paused, saveFailed]);
  if (saveFailed) return <span className="note-sync-status" role="status">Sync status unavailable—local save needs attention</span>;
  if (saving || paused || !result || result.note !== note) return <span className="note-sync-status" role="status">{saving ? "Sync status pending local save…" : "Checking saved sync status…"}</span>;
  const status = result.status;
  if (!status || !labels[status.kind]) return <span className="note-sync-status" role="status">Sync status unavailable</span>;
  const explanation = `${explanations[status.kind]}${status.server ? ` Associated account: ${status.account ?? ""} @ ${status.server}` : ""}`;
  const action = status.kind === "conflict_saved" ? onConflicts : status.kind === "recovery_needed" ? onRecovery : null;
  return <span className="note-sync-status" role="status" title={explanation}>
    {action ? <button title={explanation} onClick={action}>{labels[status.kind]} — review</button> : labels[status.kind]}
  </span>;
}
