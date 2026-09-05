# RustyNotes

![RustyNotes notebook icon](src-tauri/icons/128x128.png)

A local-first desktop Markdown notes app for your own Nextcloud Notes server,
built with Rust, Tauri, React, TypeScript, and SQLite.

I got frustrated with existing apps that either didn't fit my use case or had
features I didn't need or want. So I made my own.

**Status: alpha (0.1.0).** Working features are being developed and tested in
small steps. Keep backups of important notes. Synchronization is manual, not
an automatic background service.

## What works today

- Offline note creation, editing, and persistence across restarts.
- Plain Markdown editing with formatting controls, including links and lists.
- Read-only Markdown preview with headings, tables, code blocks, and task lists.
- Search, categories, and favorites.
- A **Local changes** sidebar filter for local-only notes, edits, conflicts, and uncertain or unknown sync status.
- Light and dark themes with a remembered top-toolbar toggle.
- Markdown file import and export using native file dialogs.
- Local Trash with confirmation and restore; it does not delete server notes.
- Verified local database backups, read-only preview, and explicitly confirmed restore with a safety backup.
- Nextcloud Login Flow v2 and the official Nextcloud Notes API.
- Read-only connection checks, initial import, and download-only Refresh.
- Explicit upload of one selected note, including creation of a new server note.
- Saved conflict comparisons with keep-local, use-saved-server, and keep-both choices.
- Per-note sync status and recovery tools for uncertain new-note uploads.

Note bodies are plain Markdown **stored in a local SQLite database**, not a
folder of `.md` files. Use Markdown export when you want a standalone file.

## Markdown preview

Use **Preview** above the editor to see formatted Markdown, then **Edit** to
return to the original text. Switching modes does not alter the Markdown or
upload it. New notes open in Edit mode.

Preview supports tables, task lists, and strikethrough as well as basic Markdown.
Raw HTML is disabled, links are inactive, and images appear as labeled
placeholders: preview does not fetch external content. Task checkboxes are
read-only. Notes over 100,000 characters remain editable but cannot be previewed.

## Appearance

Use the **Dark** (moon) or **Light** (sun) button in the top toolbar to switch
themes. On first launch, the app uses your system preference; your explicit
choice is then remembered locally between launches. The setting is specific
to this installation's webview storage, not synced to Nextcloud or included in
SQLite backups. Native file pickers and operating-system window decorations
still follow the operating system's appearance.

## How Nextcloud synchronization works

1. In **Settings**, save the final HTTPS address of your Nextcloud server.
2. Choose **Authorize with Nextcloud** and complete authorization in your browser.
   Enter your normal password on the Nextcloud page, not in RustyNotes.
3. Run **Check Notes connection**, then **Import server notes (download only)**.
4. Use **Refresh** on the main screen to download subsequent server changes.
5. To send a local change, choose **Upload selected note…** and confirm it.

Import skips previously imported notes. Refresh updates unchanged local copies,
preserves local edits, and saves a comparison when both sides changed differently.
Resolving a conflict is a local action; it does not automatically upload anything.

There are **no automatic uploads, bulk uploads, or server deletions**. Notes
missing from the server are retained locally. A server-change check can stop an
upload; refresh and review the versions before trying again. If an upload outcome
is uncertain, check Nextcloud and use the recovery workflow rather than repeatedly
creating the same note.

The server needs the Nextcloud Notes app. Existing-note uploads require advertised
Notes API 1.2 or newer and a usable strong ETag (a server version marker).
See [the detailed synchronization test guide](AUTH_TESTING.md) for safeguards
and disposable-note test scenarios.

## Platforms tested

| Platform | Manually tested |
| --- | --- |
| macOS | Development app, persistent notes, Nextcloud workflows, native file dialogs |
| Ubuntu 24.04 LTS / GNOME | Development app, installed `.deb`, and AppImage |
| CachyOS / KDE Plasma | Development app and Ubuntu-built AppImage |
| Pop!_OS 24.04 / KDE Plasma | Development app, installed `.deb`, and Ubuntu-built AppImage |

The latest copyright-packaging AppImage passed launch and Notes connection checks
on all three Linux systems, with Refresh also confirmed on Ubuntu and Pop!_OS.
These are checks on the tested machines, not a guarantee for every graphics driver
or distribution. Windows is not currently supported by the credential-storage
implementation, even though Windows icon assets are included.

## Build and run from source

Install Rust and Cargo through rustup, Node.js with npm, and the
[Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/).
Node 22.23.2 and 24.20.0 have been used in Linux testing; these are tested versions,
not a declaration of minimum requirements.

For Ubuntu / Pop!_OS 24.04, the build dependencies used are:

```bash
sudo apt update
sudo apt install build-essential pkg-config curl wget file libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev
```

On macOS, install Xcode Command Line Tools. On CachyOS, follow the Arch dependency
section of the Tauri prerequisites. Linux authorization also needs a running,
unlocked Secret Service-compatible desktop keyring; there is no plaintext fallback.

```bash
git clone https://github.com/KwisatzJim/RustyNotes.git
cd RustyNotes
npm ci
npm run tauri dev
```

For later launches, use `npm run tauri dev`; reinstall JavaScript dependencies
when the dependency files change, not before every launch. `npm run dev` alone
starts only the web frontend and cannot exercise the native database or keyring.

### Automated checks

Run these from the project root:

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```

Automated checks do not replace real-server and desktop interaction tests.
Use disposable notes for server-writing tests.

GitHub Actions runs the frontend checks on Ubuntu and the Rust suite on both
Ubuntu 24.04 and macOS after pushes and pull requests. CI uses only fixture data;
it does not receive Nextcloud credentials or contact a notes server.

## Build desktop packages

On Ubuntu, build a Debian package with:

```bash
npm run tauri build -- --bundles deb
```

Output: `src-tauri/target/release/bundle/deb/`.

For the AppImage, **build on Ubuntu using the project wrapper**:

```bash
npm run build:appimage
```

Output: `src-tauri/target/release/bundle/appimage/`. Do not substitute the plain
Tauri AppImage command: the wrapper preserves the tested Wayland-library fix
and verifies a byte-identical Ubuntu package notice for every bundled ELF
library, adding and repacking missing notices when needed. See
[AppImage build details](APPIMAGE_BUILD.md).
The host must provide `libwayland-client.so.0`; the tested image does not need
an `LD_PRELOAD` workaround.

On macOS, a local application bundle can be built with:

```bash
npm run tauri build -- --bundles app
```

Output: `src-tauri/target/release/bundle/macos/`. This does not establish Developer ID
signing or notarization. The locked source dependencies have been reviewed in
[LICENSE_AUDIT.md](LICENSE_AUDIT.md); final binary notice generation, an Ubuntu
AppImage library audit, and AppStream metadata remain release-preparation work.

The custom notebook icon is generated from [an editable SVG](src-tauri/icons/source.svg).
Icon and metadata changes require rebuilding and replacing the old package.

## Local data and credentials

- macOS database: `~/Library/Application Support/RustyNotes/rustynotes.db`.
- Linux database: `${XDG_DATA_HOME:-$HOME/.local/share}/RustyNotes/rustynotes.db`
  (the default is `~/.local/share/RustyNotes/rustynotes.db`).
- Credentials: macOS Keychain or Linux Secret Service, separate from the database.

Use **Back up local data…** in the top toolbar, then **Choose backup destination…**.
The app waits for pending saves and creates a consistent, verified `.sqlite3`
snapshot including notes, Trash, settings, saved conflicts, and sync history.
Choose a new filename outside the live data folder; existing files are not replaced.
The backup is **not encrypted** and excludes keyring credentials. Keep it private.
In the same dialog, **Preview backup…** opens an existing `.sqlite3` backup
(up to 128 MiB), checks database integrity and the expected table structure,
and shows note and Trash counts. It inspects a private temporary copy, not the
live database, and does not display note contents. Files with live SQLite
companion files or an unsupported schema are rejected. Preview never changes your
current data or Nextcloud server. Preview also rejects more than 50,000 active or
trashed notes, more than 100,000 rows in another app table, an individual text value
over 8 MiB, or more than 96 MiB of text in total.

To restore, preview the backup, read the replacement warning, check the confirmation
box, and choose **Restore this backup**. Close other RustyNotes copies using the same
data first. Restore waits for queued saves, refuses failed saves or an active login,
and revalidates the file. If its bytes changed after preview, a new preview is required.
It creates and verifies a safety backup before replacing local rows in one SQLite
transaction. Failed row replacement rolls back; the safety backup remains available.
Notes, Trash, settings, and sync history are replaced, not merged. Nothing is uploaded
or deleted on Nextcloud, and keyring credentials are unchanged.

The result shows the safety backup path, in a `before-restore-*` folder alongside
`rustynotes.db`. These backups are private, unencrypted, and not automatically removed.
Use **Open data folder** in the backup-and-restore dialog to find them in Finder
or your Linux file manager without navigating hidden folders. Do not move or edit
the live `rustynotes.db` file.
They can be previewed and restored through the same workflow. After restoring, review
your server address and Refresh before uploading old local versions. Restore reloads
the editor from SQLite; if reloading fails, editing is paused until you reload.

For a manual backup instead, fully quit RustyNotes and copy its entire `RustyNotes`
data directory. Markdown exports contain the note text, not categories, favorites,
server associations, or conflict history. A database backup does not back up
the separate credential store.

Nextcloud API requests use the configured HTTPS server; redirects and system
proxies are disabled to protect credentials. Browser authorization requires
access to that server. Building the app downloads dependencies and packaging
tools from other services; that is separate from the app's Notes API traffic.

## Project layout

- `src/`: React editor, dialogs, and local-save coordination.
- `src-tauri/src/`: SQLite storage, Nextcloud requests, credentials, and native commands.
- `scripts/build-appimage.mjs`: Linux packaging and verification wrapper.
- `tests/`: JavaScript tests; Rust tests live alongside the backend code.
- [AUTH_TESTING.md](AUTH_TESTING.md): detailed manual sync and safety checks.

## License

RustyNotes is available under the [MIT License](LICENSE). Copyright © 2026
Jim Kelley. Third-party components remain subject to their respective licenses.
