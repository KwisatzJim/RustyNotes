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
- Search, categories, and favorites.
- Markdown file import and export using native file dialogs.
- Local Trash with confirmation and restore; it does not delete server notes.
- Nextcloud Login Flow v2 and the official Nextcloud Notes API.
- Read-only connection checks, initial import, and download-only Refresh.
- Explicit upload of one selected note, including creation of a new server note.
- Saved conflict comparisons with keep-local, use-saved-server, and keep-both choices.
- Per-note sync status and recovery tools for uncertain new-note uploads.

Note bodies are plain Markdown **stored in a local SQLite database**, not a
folder of `.md` files. Use Markdown export when you want a standalone file.

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
and verifies recovered copyright notices. See [AppImage build details](APPIMAGE_BUILD.md).
The host must provide `libwayland-client.so.0`; the tested image does not need
an `LD_PRELOAD` workaround.

On macOS, a local application bundle can be built with:

```bash
npm run tauri build -- --bundles app
```

Output: `src-tauri/target/release/bundle/macos/`. This does not establish Developer ID
signing or notarization. Public distribution, complete third-party license review,
and AppStream metadata remain release-preparation work.

The custom notebook icon is generated from [an editable SVG](src-tauri/icons/source.svg).
Icon and metadata changes require rebuilding and replacing the old package.

## Local data and credentials

- macOS database: `~/Library/Application Support/RustyNotes/rustynotes.db`.
- Linux database: `${XDG_DATA_HOME:-$HOME/.local/share}/RustyNotes/rustynotes.db`
  (the default is `~/.local/share/RustyNotes/rustynotes.db`).
- Credentials: macOS Keychain or Linux Secret Service, separate from the database.

For a database backup, fully quit RustyNotes and copy its entire `RustyNotes`
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
