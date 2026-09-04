import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";
import "./App.css";
import { acknowledgeSave, createSaveQueue } from "./saveQueue";
import { Settings } from "./Settings";
import { Conflicts } from "./Conflicts";
import { Upload } from "./Upload";
import { Refresh } from "./Refresh";
import { Export } from "./Export";
import { Backup } from "./Backup";
import { MarkdownPreview, PreviewBoundary } from "./MarkdownPreview";
import { localChangeIds, localChangeLabel, type LocalChange } from "./localChanges";
import { oppositeTheme, saveTheme, THEME_KEY, type Theme } from "./theme";
import { ImportMarkdown } from "./ImportMarkdown";
import { Recovery } from "./Recovery";
import { NoteSyncStatus } from "./NoteSyncStatus";
import { Trash } from "./Trash";
import type { ConflictSummary, RefreshSummary, ResolutionChoice } from "./syncTypes";

interface Note {
  id: number;
  title: string;
  content: string;
  category: string;
  favorite: boolean;
  modified_at: number;
}

function App() {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [theme, setTheme] = useState<Theme>(() => document.documentElement.dataset.theme === "dark" ? "dark" : "light");
  const [themeSaveFailed, setThemeSaveFailed] = useState(false);
  function toggleTheme() {
    const next = oppositeTheme(theme);
    document.documentElement.dataset.theme = next;
    setTheme(next);
    setThemeSaveFailed(!saveTheme(next, value => localStorage.setItem(THEME_KEY, value)));
  }
  const [backupOpen, setBackupOpen] = useState(false);
  const [refreshOpen, setRefreshOpen] = useState(false);
  const [conflictsOpen, setConflictsOpen] = useState(false);
  const [uploadTarget, setUploadTarget] = useState<{ id: number; title: string } | null>(null);
  const [exportTarget, setExportTarget] = useState<{ id: number; title: string } | null>(null);
  const [importMarkdownOpen, setImportMarkdownOpen] = useState(false);
  const [recoveryOpen, setRecoveryOpen] = useState(false);
  const [trashOpen, setTrashOpen] = useState(false);
  const [trashBusy, setTrashBusy] = useState(false);
  const trashLocked = useRef(false);
  const [trashError, setTrashError] = useState("");
  const [conflicts, setConflicts] = useState<ConflictSummary[]>([]);
  const [reloadRequired, setReloadRequired] = useState(false);
  const failedSaves = useRef(new Set<number>());
  const enqueueSave = useMemo(() => createSaveQueue(), []);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [pendingSaves, setPendingSaves] = useState(0);
  const [notes, setNotes] = useState<Note[]>([]);
  const [selectedNoteId, setSelectedNoteId] = useState<number | null>(null);
  const [search, setSearch] = useState("");
  const [selectedCategory, updateSelectedCategory] = useState("All Notes");
  const [localChangesOnly, setLocalChangesOnly] = useState(false);
  const [changes, setChanges] = useState<LocalChange[]>([]);
  const [changesLoading, setChangesLoading] = useState(true);
  const [changesError, setChangesError] = useState(false);
  const [changesRetry, setChangesRetry] = useState(0);
  function setSelectedCategory(category: string) {
    setLocalChangesOnly(false);
    updateSelectedCategory(category);
  }
  useEffect(() => {
    let active = true;
    setChangesLoading(true);
    if (pendingSaves > 0) return () => { active = false; };
    // Debounce rapid saves; one native call reads a consistent local snapshot.
    const timer = window.setTimeout(() => {
      void invoke<LocalChange[]>("list_local_changes")
        .then(result => { if (active) { setChanges(result); setChangesError(false); } })
        .catch(() => { if (active) setChangesError(true); })
        .finally(() => { if (active) setChangesLoading(false); });
    }, 150);
    return () => { active = false; window.clearTimeout(timer); };
  }, [notes, pendingSaves, changesRetry]);
  const changedIds = useMemo(() => localChangeIds(changes, failedSaves.current), [changes, saveError, pendingSaves]);
  const [deleteConfirmation, setDeleteConfirmation] = useState<number | null>(null);
  const [pendingLink, setPendingLink] = useState<{
    noteId: number;
    start: number;
    end: number;
    text: string;
    url: string;
  } | null>(null);

  const editorRef = useRef<HTMLTextAreaElement>(null);
  const [previewMode, setPreviewMode] = useState(false);
  const editorPosition = useRef<{ id: number; start: number; end: number; scroll: number } | null>(null);
  const focusEditor = useRef(false);
  function changeEditorMode(preview: boolean) {
    if (preview === previewMode) return;
    if (preview && selectedNoteId !== null && editorRef.current) {
      const editor = editorRef.current;
      editorPosition.current = { id: selectedNoteId, start: editor.selectionStart, end: editor.selectionEnd, scroll: editor.scrollTop };
      setPendingLink(null);
    }
    focusEditor.current = !preview;
    setPreviewMode(preview);
  }
  useEffect(() => {
    if (previewMode || !focusEditor.current || !editorRef.current) return;
    focusEditor.current = false;
    const editor = editorRef.current;
    editor.focus();
    const saved = editorPosition.current;
    if (saved?.id === selectedNoteId) {
      editor.setSelectionRange(saved.start, saved.end);
      editor.scrollTop = saved.scroll;
    }
  }, [previewMode, selectedNoteId]);

  useEffect(() => {
    setPendingLink(null);
    setDeleteConfirmation(null);
  }, [selectedNoteId]);

  useEffect(() => {
    loadNotes();
    void invoke<ConflictSummary[]>("list_refresh_conflicts").then(setConflicts).catch(() => undefined);
  }, []);

  async function loadNotes() {
    try {
      const storedNotes = await invoke<Note[]>("get_notes");
      setNotes(storedNotes);

      if (storedNotes.length > 0) {
        setSelectedNoteId(storedNotes[0].id);
      }
    } catch (error) {
      console.error("Failed to load notes:", error);
    }
  }

  async function refreshImportedNotes() {
    const storedNotes = await invoke<Note[]>("get_notes");
    // Refresh only newly imported rows: do not overwrite edits or pending saves.
    setNotes((current) => {
      const existing = new Set(current.map((note) => note.id));
      return [...storedNotes.filter((note) => !existing.has(note.id)), ...current];
    });
    setSelectedNoteId((current) => current ?? storedNotes[0]?.id ?? null);
    setSearch("");
    setSelectedCategory("All Notes");
  }

  async function refreshFromServer(): Promise<RefreshSummary> {
    return enqueueSave(async () => {
      if (failedSaves.current.size > 0 || reloadRequired) {
        throw "Close this dialog and retry failed saves or reload local notes before refreshing.";
      }
      setPendingLink(null);
      try {
        const summary = await invoke<RefreshSummary>("refresh_server_notes");
        const storedNotes = await invoke<Note[]>("get_notes");
        // Both refresh entry points are modal and the save queue is held, so no edits can race
        // with this replacement. All pending saves finished before refresh.
        setNotes(storedNotes);
        setSelectedNoteId((current) => current ?? storedNotes[0]?.id ?? null);
        setSearch("");
        setSelectedCategory("All Notes");
        setConflicts(await invoke<ConflictSummary[]>("list_refresh_conflicts"));
        return summary;
      } catch (error) {
        // A transport error can obscure whether the database commit succeeded.
        // Reload before permitting edits, or fail closed to avoid stale saves.
        try { setNotes(await invoke<Note[]>("get_notes")); }
        catch { setReloadRequired(true); }
        throw error;
      }
    });
  }

  async function resolveConflict(id: number, choice: ResolutionChoice): Promise<void> {
    return enqueueSave(async () => {
      if (failedSaves.current.size > 0 || reloadRequired) throw "Reload or retry failed local saves before resolving.";
      setPendingLink(null);
      try {
        await invoke("resolve_conflict", { id, choice });
        setNotes(await invoke<Note[]>("get_notes"));
        setConflicts(await invoke<ConflictSummary[]>("list_refresh_conflicts"));
      } catch (error) {
        try { setNotes(await invoke<Note[]>("get_notes")); }
        catch { setReloadRequired(true); }
        throw error;
      }
    });
  }

  async function recoverCreation(id: number, copyId: number, token: string): Promise<void> {
    return enqueueSave(async () => {
      if (failedSaves.current.size > 0 || reloadRequired) throw "Close Recovery and retry failed saves or reload before recovering.";
      await invoke("recover_creation", { id, copyId, token });
      setNotes(await invoke<Note[]>("get_notes"));
    });
  }

  async function exportNote(id: number): Promise<string | null> {
    return enqueueSave(async () => {
      if (failedSaves.current.size > 0 || reloadRequired) throw "Close Export and retry failed saves or reload local notes before exporting.";
      // The modal prevents editing while pending saves finish and the native
      // dialog is open. Rust reads the saved note, not a stale React snapshot.
      return invoke<string | null>("export_note", { id });
    });
  }

  async function backupLocalData(): Promise<string | null> {
    return enqueueSave(async () => {
      if (failedSaves.current.size > 0 || reloadRequired) throw "Close Backup and retry failed saves or reload local notes before backing up.";
      return invoke<string | null>("backup_local_data");
    });
  }

  async function restoreLocalBackup(token: string): Promise<string> {
    return enqueueSave(async () => {
      if (failedSaves.current.size > 0 || reloadRequired) throw "Close Backup and retry failed saves or reload local notes before restoring.";
      setPendingLink(null);
      setDeleteConfirmation(null);
      async function reloadRestoredData() {
        const restored = await invoke<Note[]>("get_notes");
        const comparisons = await invoke<ConflictSummary[]>("list_refresh_conflicts");
        setNotes(restored);
        setSelectedNoteId(restored[0]?.id ?? null);
        setConflicts(comparisons);
        setSearch(""); setSelectedCategory("All Notes");
      }
      let safety: string | null = null;
      try {
        safety = await invoke<string>("restore_local_backup", { token, confirmed: true });
        await reloadRestoredData();
        return safety;
      } catch (error) {
        // IPC can fail after a committed restore. Never let stale editor state
        // overwrite the database, even when the operation reports an error.
        try { await reloadRestoredData(); } catch { setReloadRequired(true); }
        throw safety ? `Restore completed, but reloading the screen failed. Reload local notes before continuing. Safety backup: ${safety}` : error;
      }
    });
  }

  async function importMarkdown(): Promise<string | null> {
    return enqueueSave(async () => {
      if (failedSaves.current.size > 0 || reloadRequired) throw "Close Import and retry failed saves or reload before importing.";
      try {
        const note = await invoke<Note | null>("import_markdown");
        if (!note) return null;
        setNotes((current) => [note, ...current.filter((item) => item.id !== note.id)]);
        setSearch("");
        setSelectedCategory("All Notes");
        setSelectedNoteId(note.id);
        return note.title;
      } catch (error) {
        // An IPC failure can hide a successful insert. Reload, never retry it.
        try { setNotes(await invoke<Note[]>("get_notes")); }
        catch { setReloadRequired(true); }
        throw error;
      }
    });
  }

  async function uploadNote(id: number, createNew: boolean): Promise<void> {
    return enqueueSave(async () => {
      if (failedSaves.current.size > 0 || reloadRequired) throw "Retry failed local saves or reload before uploading.";
      setPendingLink(null);
      try {
        await invoke(createNew ? "create_server_note" : "upload_note", { id });
        setNotes(await invoke<Note[]>("get_notes"));
      } catch (error) {
        try { setNotes(await invoke<Note[]>("get_notes")); }
        catch { setReloadRequired(true); }
        throw error;
      }
    });
  }

  const selectedNote = notes.find(
    (note) => note.id === selectedNoteId,
  );

  const categories = useMemo(() => {
    const values = notes
      .map((note) => note.category)
      .filter((category) => category.trim() !== "");

    return Array.from(new Set(values)).sort();
  }, [notes]);

  const filteredNotes = useMemo(() => {
    const query = search.toLowerCase();

    return notes.filter((note) => {
      const matchesSearch =
        note.title.toLowerCase().includes(query) ||
        note.content.toLowerCase().includes(query);

      const matchesCategory = localChangesOnly ? changedIds.has(note.id) :
        selectedCategory === "All Notes" ||
        (selectedCategory === "Favorites" && note.favorite) ||
        note.category === selectedCategory;

      return matchesSearch && matchesCategory;
    });
  }, [notes, search, selectedCategory, localChangesOnly, changedIds]);

  function formatSelection(prefix: string, suffix: string = prefix) {
    const textarea = editorRef.current;

    if (!textarea || !selectedNote) return;

    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;

    const selectedText = selectedNote.content.slice(start, end);

    const newText =
      selectedText.length > 0
        ? prefix + selectedText + suffix
        : prefix + suffix;

    const updatedContent =
      selectedNote.content.slice(0, start) +
      newText +
      selectedNote.content.slice(end);

    updateNoteField("content", updatedContent);

    saveNote({
      ...selectedNote,
      content: updatedContent,
    });

    requestAnimationFrame(() => {
      textarea.focus();

      if (selectedText.length > 0) {
        textarea.setSelectionRange(
          start + prefix.length,
          start + prefix.length + selectedText.length,
        );
      } else {
        const cursorPosition = start + prefix.length;
        textarea.setSelectionRange(
          cursorPosition,
          cursorPosition,
        );
      }
    });
  }

  function formatLink() {
    const textarea = editorRef.current;

    if (!textarea || !selectedNote) return;

    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;

    const selectedText = selectedNote.content.slice(start, end);

    if (selectedText.length === 0) return;

    setPendingLink({ noteId: selectedNote.id, start, end, text: selectedText, url: "" });
  }

  function applyLink() {
    const textarea = editorRef.current;

    if (!textarea || !selectedNote || !pendingLink) return;
    if (pendingLink.noteId !== selectedNote.id) return;

    const url = pendingLink.url.trim();

    if (!url) return;

    const newText = `[${pendingLink.text}](${url})`;

    const updatedContent =
      selectedNote.content.slice(0, pendingLink.start) +
      newText +
      selectedNote.content.slice(pendingLink.end);

    updateNoteField("content", updatedContent);
    saveNote({
      ...selectedNote,
      content: updatedContent,
    });

    setPendingLink(null);

    requestAnimationFrame(() => {
      textarea.focus();

      textarea.setSelectionRange(
        pendingLink.start + 1,
        pendingLink.start + 1 + pendingLink.text.length,
      );
    });
  }

  function formatHeading() {
    const textarea = editorRef.current;

    if (!textarea || !selectedNote) return;

    const content = selectedNote.content;
    const cursorPosition = textarea.selectionStart;

    const lineStart = content.lastIndexOf("\n", cursorPosition - 1) + 1;

    let lineEnd = content.indexOf("\n", cursorPosition);

      if (lineEnd === -1) {
      lineEnd = content.length;
      }

    const line = content.slice(lineStart, lineEnd);

    const headingMatch = line.match(/^(#{1,6})\s+/);

    let newLine: string;

      if (!headingMatch) {
        newLine = `# ${line}`;
      } else {
    const currentLevel = headingMatch[1].length;

      if (currentLevel < 6) {
        newLine = `${"#".repeat(currentLevel + 1)}${line.slice(
          headingMatch[1].length,
        )}`;
      } else {
        newLine = line.replace(/^#{6}\s+/, "");
      }
    }

  const updatedContent =
    content.slice(0, lineStart) +
    newLine +
    content.slice(lineEnd);

  updateNoteField("content", updatedContent);

  requestAnimationFrame(() => {
    textarea.focus();

    const newCursorPosition =
      cursorPosition + (newLine.length - line.length);

    textarea.setSelectionRange(
      newCursorPosition,
      newCursorPosition,
    );
  });
}

function formatBulletedList() {
  const textarea = editorRef.current;

  if (!textarea || !selectedNote) return;

  const content = selectedNote.content;
  const selectionStart = textarea.selectionStart;
  const selectionEnd = textarea.selectionEnd;

  const blockStart =
    content.lastIndexOf("\n", selectionStart - 1) + 1;

  let blockEnd = content.indexOf("\n", selectionEnd);

  if (blockEnd === -1) {
    blockEnd = content.length;
  }

  const block = content.slice(blockStart, blockEnd);
  const lines = block.split("\n");

  const nonEmptyLines = lines.filter(
    (line) => line.trim() !== "",
  );

  if (nonEmptyLines.length === 0) return;

  const allBulleted = nonEmptyLines.every((line) =>
    /^\s*-\s+/.test(line),
  );

  const newLines = lines.map((line) => {
    if (line.trim() === "") return line;

    if (allBulleted) {
      return line.replace(/^(\s*)-\s+/, "$1");
    }

    return `- ${line}`;
  });

  const newBlock = newLines.join("\n");

  const updatedContent =
    content.slice(0, blockStart) +
    newBlock +
    content.slice(blockEnd);

  updateNoteField("content", updatedContent);

  requestAnimationFrame(() => {
    textarea.focus();

    const lengthDifference =
      newBlock.length - block.length;

    textarea.setSelectionRange(
      Math.max(0, selectionStart + lengthDifference),
      Math.max(0, selectionEnd + lengthDifference),
    );
  });
}

function formatNumberedList() {
  const textarea = editorRef.current;

  if (!textarea || !selectedNote) return;

  const content = selectedNote.content;
  const selectionStart = textarea.selectionStart;
  const selectionEnd = textarea.selectionEnd;

  const blockStart =
    content.lastIndexOf("\n", selectionStart - 1) + 1;

  let blockEnd = content.indexOf("\n", selectionEnd);

  if (blockEnd === -1) {
    blockEnd = content.length;
  }

  const block = content.slice(blockStart, blockEnd);
  const lines = block.split("\n");

  const nonEmptyLines = lines.filter(
    (line) => line.trim() !== "",
  );

  if (nonEmptyLines.length === 0) return;

  const allNumbered = nonEmptyLines.every((line) =>
    /^\s*\d+\.\s+/.test(line),
  );

  let number = 1;

  const newLines = lines.map((line) => {
    if (line.trim() === "") return line;

    if (allNumbered) {
      return line.replace(/^(\s*)\d+\.\s+/, "$1");
    }

    const newLine = `${number}. ${line}`;
    number++;

    return newLine;
  });

  const newBlock = newLines.join("\n");

  const updatedContent =
    content.slice(0, blockStart) +
    newBlock +
    content.slice(blockEnd);

  updateNoteField("content", updatedContent);

  requestAnimationFrame(() => {
    textarea.focus();

    const lengthDifference =
      newBlock.length - block.length;

    textarea.setSelectionRange(
      Math.max(0, selectionStart + lengthDifference),
      Math.max(0, selectionEnd + lengthDifference),
    );
  });
}

function formatChecklist() {
  const textarea = editorRef.current;

  if (!textarea || !selectedNote) return;

  const content = selectedNote.content;
  const selectionStart = textarea.selectionStart;
  const selectionEnd = textarea.selectionEnd;

  const blockStart =
    content.lastIndexOf("\n", selectionStart - 1) + 1;

  let blockEnd = content.indexOf("\n", selectionEnd);

  if (blockEnd === -1) {
    blockEnd = content.length;
  }

  const block = content.slice(blockStart, blockEnd);
  const lines = block.split("\n");

  const nonEmptyLines = lines.filter(
    (line) => line.trim() !== "",
  );

  if (nonEmptyLines.length === 0) return;

  const checklistPattern =
    /^\s*-\s+\[[ xX]\]\s+/;

  const allChecklist = nonEmptyLines.every((line) =>
    checklistPattern.test(line),
  );

  const newLines = lines.map((line) => {
    if (line.trim() === "") return line;

    if (allChecklist) {
      return line.replace(
        /^(\s*)-\s+\[[ xX]\]\s+/,
        "$1",
      );
    }

    return `- [ ] ${line}`;
  });

  const newBlock = newLines.join("\n");

  const updatedContent =
    content.slice(0, blockStart) +
    newBlock +
    content.slice(blockEnd);

  updateNoteField("content", updatedContent);

  requestAnimationFrame(() => {
    textarea.focus();

    textarea.setSelectionRange(
      blockStart,
      blockStart + newBlock.length,
    );
  });
}

  function updateNoteField(
    field: "title" | "content" | "category",
    value: string,
  ) {
    if (!selectedNote) return;

    void saveNote({ ...selectedNote, [field]: value });

    setNotes((currentNotes) =>
      currentNotes.map((note) =>
        note.id === selectedNote.id
          ? { ...note, [field]: value }
          : note,
      ),
    );
  }

  function toggleFavorite() {
    if (!selectedNote) return;

    const updatedFavorite = !selectedNote.favorite;

    setNotes((currentNotes) =>
      currentNotes.map((note) =>
        note.id === selectedNote.id
          ? { ...note, favorite: updatedFavorite }
          : note,
      ),
    );

    saveNote({
      ...selectedNote,
      favorite: updatedFavorite,
    });
  }

  async function saveNote(note: Note) {
    setPendingSaves((count) => count + 1);
    try {
      const updatedNote = await enqueueSave(() => invoke<Note>("update_note", {
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

  async function saveCurrentNote() {
    if (!selectedNote) return;
    await saveNote(selectedNote);
  }

  async function createNote() {
    try {
      const newNote = await invoke<Note>("create_note", {
        title: "Untitled Note",
        content: "# Untitled Note\n\n",
        category: "Personal",
        favorite: false,
      });

      setNotes((currentNotes) => [newNote, ...currentNotes]);
      setSelectedNoteId(newNote.id);
      setPreviewMode(false);
      focusEditor.current = true;
      setSelectedCategory("All Notes");
    } catch (error) {
      console.error("Failed to create note:", error);
    }
  }

  async function deleteNote() {
    if (!selectedNote) return;
    if (deleteConfirmation !== selectedNote.id) return;
    if (trashLocked.current) return;
    trashLocked.current = true;
    setTrashBusy(true);
    setTrashError("");

    try {
      await enqueueSave(async () => {
        if (failedSaves.current.size > 0 || reloadRequired) throw "Save your changes successfully before moving a note to Trash.";
        await invoke("delete_note", { id: selectedNote.id });
      });

      const remainingNotes = notes.filter(
        (note) => note.id !== selectedNote.id,
      );

      setNotes((current) => current.filter((note) => note.id !== selectedNote.id));
      setSelectedNoteId((current) => current === selectedNote.id
        ? remainingNotes[0]?.id ?? null : current);
      setDeleteConfirmation(null);
    } catch (error) {
      console.error("Failed to delete note:", error);
      setTrashError(typeof error === "string" ? error : "Could not move the note to Trash. Check Trash before trying again.");
    } finally {
      trashLocked.current = false;
      setTrashBusy(false);
    }
  }

  async function restoreNote(id: number) {
    await enqueueSave(async () => {
      if (failedSaves.current.size > 0 || reloadRequired) throw "Save your changes successfully before restoring a note.";
      const restored = await invoke<Note>("restore_note", { id });
      setNotes((current) => [restored, ...current.filter((note) => note.id !== id)]);
      setSelectedNoteId(id);
      setSelectedCategory("All Notes");
      setSearch("");
      setTrashError("");
    });
  }

  function formatDate(timestamp: number) {
    if (!timestamp) return "";

    return new Date(timestamp * 1000).toLocaleDateString(
      undefined,
      {
        month: "short",
        day: "numeric",
        year: "numeric",
      },
    );
  }

  return (
    <div className="app">
      <header className="topbar" inert={trashBusy}>
        <div className="brand">
          <div className="brand-mark">R</div>
          <span>RustyNotes</span>
        </div>

        <div className="topbar-actions">
          <button className="theme-toggle" onClick={toggleTheme} title={`Switch to ${oppositeTheme(theme)} theme`} aria-label={`Switch to ${oppositeTheme(theme)} theme`}>
            <span aria-hidden="true">{theme === "dark" ? "☀" : "☾"}</span> {theme === "dark" ? "Light" : "Dark"}
          </button>
          {themeSaveFailed && <span role="status" className="theme-save-warning">Theme changed, but could not be saved for next launch.</span>}
          <button
            className="icon-button"
            title="Search"
            onClick={() =>
              document
                .querySelector<HTMLInputElement>(".search-box input")
                ?.focus()
            }
          >
            ⌕
          </button>

          <button disabled={reloadRequired || refreshOpen} title="Refresh from Nextcloud (download only)" onClick={() => setRefreshOpen(true)}>↻ Refresh</button>
          <button disabled={reloadRequired} onClick={() => setBackupOpen(true)}>Back up local data…</button>
          <button disabled={reloadRequired} onClick={() => setImportMarkdownOpen(true)}>Import Markdown…</button>
          <button disabled={!selectedNote || reloadRequired} onClick={() => { if (selectedNote) setExportTarget({ id: selectedNote.id, title: selectedNote.title }); }}>Export Markdown…</button>
          <button disabled={!selectedNote || reloadRequired} onClick={() => { if (selectedNote) setUploadTarget({ id: selectedNote.id, title: selectedNote.title }); }}>Upload selected note…</button>
          <button onClick={() => setConflictsOpen(true)}>Saved conflicts{conflicts.length > 0 ? ` (${conflicts.length})` : ""}</button>
          <button disabled={reloadRequired} onClick={() => setRecoveryOpen(true)}>Recover uploads…</button>
          <button className="icon-button" title="Settings" disabled={reloadRequired} onClick={() => setSettingsOpen(true)}>
            ⚙
          </button>
        </div>
      </header>
      {settingsOpen && <Settings onClose={() => setSettingsOpen(false)} onImported={refreshImportedNotes} onRefresh={refreshFromServer} />}
      {refreshOpen && <Refresh onClose={() => setRefreshOpen(false)} onRefresh={refreshFromServer} />}
      {exportTarget && <Export title={exportTarget.title} onClose={() => setExportTarget(null)} onExport={() => exportNote(exportTarget.id)} />}
      {backupOpen && <Backup onClose={() => setBackupOpen(false)} onBackup={backupLocalData} onRestore={restoreLocalBackup} />}
      {importMarkdownOpen && <ImportMarkdown onClose={() => setImportMarkdownOpen(false)} onImport={importMarkdown} />}
      {conflictsOpen && <Conflicts onClose={() => setConflictsOpen(false)} onResolve={resolveConflict} />}
      {recoveryOpen && <Recovery onClose={() => setRecoveryOpen(false)} onRecover={recoverCreation} />}
      {trashOpen && <Trash onClose={() => setTrashOpen(false)} onRestore={restoreNote} />}
      {uploadTarget && <Upload id={uploadTarget.id} title={uploadTarget.title} onClose={() => setUploadTarget(null)} onUpload={(createNew) => uploadNote(uploadTarget.id, createNew)} />}
      {reloadRequired && <div role="alert">Local notes could not be reloaded after refresh. Editing is paused to protect your notes. <button onClick={() => window.location.reload()}>Reload local notes</button></div>}

      {trashError && <p role="alert">{trashError}</p>}
      <div className="workspace" inert={reloadRequired || trashBusy}>
        <aside className="sidebar">
          <button
            className="new-note-button"
            onClick={createNote}
          >
            <span>＋</span>
            New Note
          </button>

          <nav className="navigation">
            <button className="nav-item" onClick={() => setTrashOpen(true)}>🗑 Local Trash</button>
            <button
              className={`nav-item ${
                !localChangesOnly && selectedCategory === "All Notes" ? "active" : ""
              }`}
              onClick={() => setSelectedCategory("All Notes")}
            >
              <span>▤</span>
              All Notes
              <span className="nav-count">{notes.length}</span>
            </button>

            <button
              className={`nav-item ${
                !localChangesOnly && selectedCategory === "Favorites" ? "active" : ""
              }`}
              onClick={() => setSelectedCategory("Favorites")}
            >
              <span>★</span>
              Favorites
              <span className="nav-count">
                {notes.filter((note) => note.favorite).length}
              </span>
            </button>
            <button className={`nav-item ${localChangesOnly ? "active" : ""}`} onClick={() => { setLocalChangesOnly(true); setSearch(""); }} title="Local-only notes, local edits, conflicts, and upload-recovery or unknown status. No server requests.">
              <span>↥</span>Local changes
              <span className="nav-count">{changesLoading ? "…" : changesError ? "?" : notes.filter(note => changedIds.has(note.id)).length}</span>
            </button>
          </nav>

          <div className="sidebar-section">
            <div className="section-title">Categories</div>

            {categories.map((category) => (
              <button
                key={category}
                className={`nav-item ${
                  !localChangesOnly && selectedCategory === category ? "active" : ""
                }`}
                onClick={() => setSelectedCategory(category)}
              >
                <span className="category-dot" />
                {category}
              </button>
            ))}
          </div>

          <div className="sidebar-footer">
            <div className="sync-status">
              <span className="status-dot" />
              <div>
                <strong>Local</strong>
                <small>Offline ready</small>
              </div>
            </div>
          </div>
        </aside>

        <section className="note-list">
          <div className="note-list-header">
            <div>
              <h1>{localChangesOnly ? "Local changes" : selectedCategory}</h1>
              <span>{localChangesOnly && changesLoading ? "Checking local changes…" : `${filteredNotes.length} notes`}</span>
            </div>
          </div>

          {localChangesOnly && <div className="local-changes-help">
            <p>Local-only notes and changes or sync issues needing review. Uploads still require individual confirmation.</p>
            {saveError && <p role="status">Some edits need to be saved before upload.</p>}
            {changesError && <p role="alert">Could not update this list. It may be incomplete. <button onClick={() => setChangesRetry(value => value + 1)}>Retry</button></p>}
            {!changesLoading && !changesError && filteredNotes.length === 0 && <p role="status">{search ? "No local changes match this search." : "No local changes or sync issues found in saved local data. This is not a live server check."}</p>}
          </div>}
          <div className="search-box">
            <span>⌕</span>

            <input
              type="text"
              placeholder="Search notes..."
              value={search}
              onChange={(event) =>
                setSearch(event.target.value)
              }
            />

            {search && (
              <button
                onClick={() => setSearch("")}
                className="clear-search"
              >
                ×
              </button>
            )}
          </div>

          <div className="notes">
            {filteredNotes.map((note) => (
              <button
                key={note.id}
                className={`note-item ${
                  selectedNoteId === note.id ? "selected" : ""
                }`}
                onClick={() => setSelectedNoteId(note.id)}
              >
                <div className="note-item-title">
                  {note.favorite && (
                    <span className="favorite-star">★</span>
                  )}

                  {note.title || "Untitled Note"}
                </div>

                <div className="note-item-preview">
                  {note.content
                    .replace(/^#+\s*/gm, "")
                    .replace(/\*\*/g, "")
                    .replace(/\*/g, "")
                    .replace(/`/g, "")
                    .slice(0, 90)}
                </div>

                <div className="note-item-meta">
                  <span>{note.category}</span>
                  <span>{formatDate(note.modified_at)}</span>
                </div>
                {localChangesOnly && <div className="note-change-reason">{failedSaves.current.has(note.id) ? "Local save needs retry" : changesLoading ? "Checking status…" : localChangeLabel(changes.find(change => change.id === note.id)?.kind)}</div>}
              </button>
            ))}
          </div>
        </section>

        <main className="editor">
          {selectedNote ? (
            <>
              <div className="editor-header">
                <div className="editor-title">
                  <input
                    value={selectedNote.title}
                    onChange={(event) =>
                      updateNoteField(
                        "title",
                        event.target.value,
                      )
                    }
                    onBlur={saveCurrentNote}
                  />

                  <input
                    className="category-input"
                    value={selectedNote.category}
                    onChange={(event) =>
                      updateNoteField(
                        "category",
                        event.target.value,
                      )
                    }
                    onBlur={saveCurrentNote}
                    aria-label="Category"
                  />
                </div>

                <div className="editor-actions">
                  <button
                    className={`editor-icon-button ${
                      selectedNote.favorite ? "favorited" : ""
                    }`}
                    onClick={toggleFavorite}
                    title="Favorite"
                  >
                    {selectedNote.favorite ? "★" : "☆"}
                  </button>

                  <button
                    className="editor-icon-button"
                    onClick={() => setDeleteConfirmation(selectedNote.id)}
                    title="Move note to local Trash"
                  >
                    🗑
                  </button>
                </div>
              </div>

              {deleteConfirmation === selectedNote.id && (
                <div className="link-editor" role="alert">
                  <span>Move “{selectedNote.title}” to local Trash? You can restore it later. Nextcloud is unchanged.</span>
                  <button onClick={deleteNote}>Move to Trash</button>
                  <button onClick={() => setDeleteConfirmation(null)}>Cancel</button>
                </div>
              )}

              <div className="editor-mode" role="group" aria-label="Editor display mode">
                <button aria-pressed={!previewMode} onClick={() => changeEditorMode(false)}>Edit</button>
                <button aria-pressed={previewMode} onClick={() => changeEditorMode(true)}>Preview</button>
                {previewMode && <span>Read-only text · links inactive · images not loaded</span>}
              </div>
              {!previewMode && <div className="formatting-toolbar" onMouseDown={(event) => event.preventDefault()}>
                <button
                  title="Bold"
                  onClick={() => formatSelection("**")}><strong>B</strong></button>

                <button
                  title="Italic"
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      formatSelection("*");
                    }
                  }}
                  onMouseDown={(event) => {
                    event.preventDefault();
                    formatSelection("*");
                  }}
                  >
                    <em>I</em>
                </button>

                <button
                  title="Strikethrough"
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      formatSelection("~~");
                    }
                  }}
                  onMouseDown={(event) => {
                    event.preventDefault();
                    formatSelection("~~");
                  }}
                >
                  <s>S</s>
                </button>

                <span className="toolbar-divider" />

                <button
                  title="Heading"
                  onClick={formatHeading}>H</button>
                <button 
                  title="Bulleted list"
                  onClick={formatBulletedList}>☷</button>
                <button 
                  title="Numbered list"
                  onClick={formatNumberedList}>☰</button>
                <button 
                  title="Checklist"
                  onClick={formatChecklist}>☑</button>



                <button
                  type="button"
                  title="Link"
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      formatLink();
                    }
                  }}
                  onMouseDown={(event) => {
                    event.preventDefault();
                    formatLink();
                  }}
                >
                  ↗
                </button>

                <button
                  type="button"
                  title="Code"
                  onClick={() => formatSelection("`")}
                >
                  &lt;/&gt;
                </button>
              </div>}

              {pendingLink && !previewMode && (
                <form
                  className="link-editor"
                  onSubmit={(event) => {
                    event.preventDefault();
                    applyLink();
                  }}
                >
                  <span>URL</span>
                  <input
                    autoFocus
                    aria-label="Link URL"
                    type="url"
                    placeholder="https://example.com"
                    value={pendingLink.url}
                    onChange={(event) =>
                      setPendingLink({
                        ...pendingLink,
                        url: event.target.value,
                      })
                    }
                  />
                  <button type="submit">Apply</button>
                  <button
                    type="button"
                    onClick={() => setPendingLink(null)}
                  >
                    Cancel
                  </button>
                </form>
              )}

              {previewMode ? <section key={selectedNote.id} className="markdown-preview" aria-label="Markdown preview" tabIndex={0}>
                <PreviewBoundary key={selectedNote.id}><MarkdownPreview content={selectedNote.content} /></PreviewBoundary>
              </section> : <textarea
                ref={editorRef}
                className="editor-content"
                value={selectedNote.content}
                onChange={(event) =>
                  updateNoteField(
                    "content",
                    event.target.value,
                  )
                }
                onBlur={saveCurrentNote}
                spellCheck={false}
              />}

              <div className="editor-footer">
                <NoteSyncStatus note={selectedNote} saving={pendingSaves > 0}
                  paused={settingsOpen || refreshOpen || conflictsOpen || recoveryOpen || trashOpen || trashBusy || importMarkdownOpen || !!exportTarget || !!uploadTarget}
                  saveFailed={!!saveError || reloadRequired}
                  onConflicts={() => setConflictsOpen(true)} onRecovery={() => setRecoveryOpen(true)} />
                <span>Markdown</span>
                <span>•</span>
                <span role="status">{saveError ?? (pendingSaves > 0 ? "Saving…" : "Saved locally")}</span>
                {saveError && <button onClick={saveCurrentNote}>Retry save</button>}
              </div>
            </>
          ) : (
            <div className="empty-editor">
              <div className="empty-icon">✎</div>
              <h2>No note selected</h2>
              <p>Create a new note to get started.</p>

              <button
                className="new-note-button"
                onClick={createNote}
              >
                <span>＋</span>
                New Note
              </button>
            </div>
          )}
        </main>
      </div>
    </div>
  );
}

export default App;
