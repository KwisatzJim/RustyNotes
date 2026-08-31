import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";
import "./App.css";

interface Note {
  id: number;
  title: string;
  content: string;
  category: string;
  favorite: boolean;
  modified_at: number;
}

function App() {
  const [notes, setNotes] = useState<Note[]>([]);
  const [selectedNoteId, setSelectedNoteId] = useState<number | null>(null);
  const [search, setSearch] = useState("");
  const [selectedCategory, setSelectedCategory] = useState("All Notes");

  const editorRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    loadNotes();
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

      const matchesCategory =
        selectedCategory === "All Notes" ||
        (selectedCategory === "Favorites" && note.favorite) ||
        note.category === selectedCategory;

      return matchesSearch && matchesCategory;
    });
  }, [notes, search, selectedCategory]);

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
    alert("FORMAT LINK CLICKED");

    const textarea = editorRef.current;

    if (!textarea || !selectedNote) return;

    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;

    const selectedText = selectedNote.content.slice(start, end);

    if (selectedText.length === 0) return;

    const url = window.prompt("Enter URL:");

    if (!url) return;

    const newText = `[${selectedText}](${url})`;

    const updatedContent =
      selectedNote.content.slice(0, start) +
      newText +
      selectedNote.content.slice(end);

      updateNoteField("content", updatedContent);

    requestAnimationFrame(() => {
      textarea.focus();

      textarea.setSelectionRange(
      start + 1,
      start + 1 + selectedText.length,
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
    try {
      const updatedNote = await invoke<Note>("update_note", {
        id: note.id,
        title: note.title,
        content: note.content,
        category: note.category,
        favorite: note.favorite,
      });

      setNotes((currentNotes) =>
        currentNotes.map((currentNote) =>
          currentNote.id === updatedNote.id
            ? updatedNote
            : currentNote,
        ),
      );
    } catch (error) {
      console.error("Failed to save note:", error);
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
      setSelectedCategory("All Notes");
    } catch (error) {
      console.error("Failed to create note:", error);
    }
  }

  async function deleteNote() {
    if (!selectedNote) return;

    const confirmed = window.confirm(
      `Delete "${selectedNote.title}"?`,
    );

    if (!confirmed) return;

    try {
      await invoke("delete_note", {
        id: selectedNote.id,
      });

      const remainingNotes = notes.filter(
        (note) => note.id !== selectedNote.id,
      );

      setNotes(remainingNotes);
      setSelectedNoteId(
        remainingNotes.length > 0 ? remainingNotes[0].id : null,
      );
    } catch (error) {
      console.error("Failed to delete note:", error);
    }
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
      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">R</div>
          <span>RustyNotes</span>
        </div>

        <div className="topbar-actions">
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

          <button className="icon-button" title="Settings">
            ⚙
          </button>
        </div>
      </header>

      <div className="workspace">
        <aside className="sidebar">
          <button
            className="new-note-button"
            onClick={createNote}
          >
            <span>＋</span>
            New Note
          </button>

          <nav className="navigation">
            <button
              className={`nav-item ${
                selectedCategory === "All Notes" ? "active" : ""
              }`}
              onClick={() => setSelectedCategory("All Notes")}
            >
              <span>▤</span>
              All Notes
              <span className="nav-count">{notes.length}</span>
            </button>

            <button
              className={`nav-item ${
                selectedCategory === "Favorites" ? "active" : ""
              }`}
              onClick={() => setSelectedCategory("Favorites")}
            >
              <span>★</span>
              Favorites
              <span className="nav-count">
                {notes.filter((note) => note.favorite).length}
              </span>
            </button>
          </nav>

          <div className="sidebar-section">
            <div className="section-title">Categories</div>

            {categories.map((category) => (
              <button
                key={category}
                className={`nav-item ${
                  selectedCategory === category ? "active" : ""
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
              <h1>{selectedCategory}</h1>
              <span>{filteredNotes.length} notes</span>
            </div>
          </div>

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
                    onClick={deleteNote}
                    title="Delete note"
                  >
                    🗑
                  </button>
                </div>
              </div>

              <div className="formatting-toolbar">
                <button
                  title="Bold"
                  onClick={() => formatSelection("**")}><strong>B</strong></button>

                <button
                  title="Italic"
                  onClick={() => formatSelection("*")}><em>I</em></button>

                <button
                  title="Strikethrough"
                  onClick={() => formatSelection("~~")}><s>S</s></button>

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
                  onClick={() => {
                    console.log("LINK CLICK");
                    alert("LINK CLICK");
                  }}
                >
                  ↗
                </button>

                <button
                  type="button"
                  title="Code"
                  onClick={() => {
                    console.log("CODE CLICK");
                    alert("CODE CLICK");
                  }}
                >
                  &lt;/&gt;
                </button>
              </div>

              <textarea
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
              />

              <div className="editor-footer">
                <span>Markdown</span>
                <span>•</span>
                <span>Saved locally</span>
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