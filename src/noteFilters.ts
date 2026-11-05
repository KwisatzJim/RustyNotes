export type NoteFilter = { kind: "all" } | { kind: "favorites" } | { kind: "category"; name: string };

export function matchesNoteFilter(note: { category: string; favorite: boolean }, filter: NoteFilter): boolean {
  if (filter.kind === "all") return true;
  if (filter.kind === "favorites") return note.favorite;
  return note.category === filter.name;
}

export function noteFilterLabel(filter: NoteFilter): string {
  return filter.kind === "all" ? "All Notes" : filter.kind === "favorites" ? "Favorites" : filter.name;
}
