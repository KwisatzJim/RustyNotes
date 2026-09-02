export interface RefreshSummary {
  added: number;
  updated: number;
  local_preserved: number;
  conflicts: number;
  unchanged: number;
  locally_deleted: number;
  server_missing: number;
}
export interface ConflictSummary { id: number; local_id: number; title: string; resolution: string | null }
export type ResolutionChoice = "keep_local" | "use_server" | "keep_both";
