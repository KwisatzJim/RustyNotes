export interface LocalChange { id: number; kind: "local_only" | "local_changes" | "conflict_saved" | "recovery_needed" | null }

export function localChangeIds(changes: LocalChange[], failedSaves: Iterable<number>): Set<number> {
  return new Set([...changes.map(change => change.id), ...failedSaves]);
}

export function localChangeLabel(kind: LocalChange["kind"] | undefined): string {
  switch (kind) {
    case "local_only": return "Local only";
    case "local_changes": return "Local edits";
    case "conflict_saved": return "Conflict — review first";
    case "recovery_needed": return "Upload recovery needed";
    default: return "Sync status needs review";
  }
}
