import { invoke } from "@tauri-apps/api/core";
import { useRef, useState, type Dispatch, type SetStateAction } from "react";
import { acknowledgeSave, createSaveQueue } from "./saveQueue";

interface SavableNote { id: number; title: string; content: string; category: string; favorite: boolean; modified_at: number }

/** Coordinate queued writes and acknowledge only the snapshot that was submitted. */
export function useNoteSaving<T extends SavableNote>(setNotes: Dispatch<SetStateAction<T[]>>, enqueueSave: ReturnType<typeof createSaveQueue>) {
  const failedSaves = useRef(new Set<number>());
  const [saveError, setSaveError] = useState<string | null>(null);
  const [pendingSaves, setPendingSaves] = useState(0);
  async function saveNote(note: T) {
    setPendingSaves((count) => count + 1);
    try {
      const updatedNote = await enqueueSave(() => invoke<T>("update_note", {
        id: note.id,
        title: note.title,
        content: note.content,
        category: note.category,
        favorite: note.favorite,
      }));

      setNotes((currentNotes) =>
        currentNotes.map((currentNote) =>
          currentNote.id === updatedNote.id
            ? acknowledgeSave(currentNote, note, updatedNote)
            : currentNote,
        ),
      );
      failedSaves.current.delete(note.id);
      if (failedSaves.current.size === 0) setSaveError(null);
    } catch (error) {
      console.error("Failed to save note:", error);
      failedSaves.current.add(note.id);
      setSaveError("Save failed. Your edits remain here; please retry before closing.");
    } finally {
      setPendingSaves((count) => count - 1);
    }
  }

  return { saveNote, failedSaves, saveError, pendingSaves };
}
