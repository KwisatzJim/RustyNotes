# Nextcloud authentication test (macOS)

This milestone implements Login Flow v2, a read-only Notes API connection
check, insert-only import, download-only refresh, local conflict resolution,
and an explicitly confirmed upload of one existing imported note. There are
no automatic uploads, bulk uploads, or server deletions. Local-only notes can
now be explicitly created on the server through a separate confirmation.

## Manual test

1. Restart the current development application (`npm run tauri dev`).
2. Open Settings and confirm the HTTPS Nextcloud base address.
3. Select **Authorize with Nextcloud**.
4. In your default browser, verify the server address and grant RustyNotes
   access. Enter your normal password only on your Nextcloud login page.
5. Return to RustyNotes. It polls automatically and should display
   **Credentials saved for …**. Allow RustyNotes access to Keychain if macOS asks.
6. Restart the application and reopen Settings. The saved-credentials status
   should remain. This means credentials exist locally, not that server access
   has been rechecked or that synchronization is enabled.

## Read-only Notes connection check

After authorization, select **Check Notes connection** in Settings. Expect
**Notes API connected — N notes found on the server**. Compare that count
with the Notes app on your server (zero is a valid result).

This makes one authenticated GET to `index.php/apps/notes/api/v1/notes`,
requesting exclusion of note contents and other metadata. Only the count
reaches React. No notes are saved to SQLite, and the result is not persisted.
Redirects, unexpected partial lists, malformed IDs, and responses over 4 MiB
are rejected rather than reporting a misleading count. Real-server behavior
is verified manually; automated tests use fake responses and credentials.

## Download-only import

Select **Import server notes (download only)** in Settings after authorization.
The first import should report the number of new copies added (434 if the
server still has the same notes). Close Settings and inspect a few titles,
Markdown contents, categories and favorites. Restart to verify offline storage.
Repeat the import: expect zero added and the previously imported count skipped.

- All pages are downloaded and validated before a single SQLite transaction
  inserts the new copies. Failures roll back that transaction.
- Existing local notes are never matched by title or overwritten.
- Identity is the normalized server address, login name, and remote note ID.
- Repeated imports **skip** existing copies, even if the server has changed.
  This protects local edits until conflict-safe synchronization is implemented.
- Locally deleted imported copies are not resurrected by repeating import.
  Their association is retained locally; no server deletion is sent.
- Categories, favorites, Markdown and timestamps are copied. The original
  server snapshot, including ETag/read-only metadata if supplied, is retained
  for conflict checking. Local edits are uploaded only through explicit confirmation.
- Downloads are bounded to 64 MiB, 1,000 pages and two minutes. Exceeding a
  limit aborts without importing any part of the batch.
- Import and refresh use only GET requests. No automatic import runs.

## Safe download-only refresh

The main-screen **↻ Refresh** button starts the same download-only refresh as
Settings. A progress dialog prevents edits or dismissal until it finishes,
then shows the result or error. It never uploads. Check that one click shows
the summary, Close returns to the editor, and the toolbar fits a narrow window.

Select **Refresh from Nextcloud (download only)** in Settings. It waits for
queued local saves, refuses to proceed with failed local saves, downloads a
complete snapshot, then applies a single database transaction.

- New server notes are added; existing local-only notes remain untouched.
- If the local note still matches its baseline, server changes are applied.
- If only the local note changed, local changes are preserved.
- If local and remote match each other, the baseline advances without conflict.
- If both changed differently, the local note stays untouched and snapshots
  of the baseline, local version and server version are saved as a conflict.
- Comparison uses title, content, category and favorite—not modification time.
- Missing server notes are retained locally, and local deletions aren't undone.
- **Saved conflicts** shows persistent snapshots, not live server versions.
  Repeated refreshes do not duplicate an unresolved pair of versions.
- Local resolution offers **Keep my version**, **Use saved server version**,
  or **Keep both**, followed by a separate confirmation. Both original snapshots
  remain available in history. Resolution itself never uploads anything.
- Keep both leaves the original local note in place and creates an unlinked
  local note ending in `(server copy)`. Retrying does not create another copy.
- Resolution acknowledges the saved server snapshot as the new comparison
  baseline. It rejects newer local edits, deleted notes, outdated baselines,
  and older comparisons. Resolve the newest comparison after a fresh refresh.
- Resolving the newest comparison archives earlier ones without deleting them.
  If the same conflict recurs later, a new unresolved record is saved.
- If the app cannot reload local notes following an uncertain refresh result,
  editing is paused until the frontend reloads from SQLite.

Suggested manual checks, using disposable notes:

1. With no edits on either side, refresh and expect unchanged notes.
2. Change a test note only in Nextcloud; refresh and confirm the local update.
3. Change a test note only locally; refresh and confirm the local edit survives.
4. Change the same imported test note differently on both sides; refresh and
   inspect **Saved conflicts** for both versions. Repeat and restart to confirm
   no duplicate comparison and persistence. No server change is sent by the app.

## Single-note upload (first server-writing milestone)

- Select an imported disposable note, then **Upload selected note…**. The modal
  names the note and requires **Confirm upload to Nextcloud**. Pending local
  saves finish first; failed saves block uploading. The modal prevents editing
  or switching notes during the request.
- Existing-note updates require the currently authorized server/account's
  association; unresolved conflicts stop. Local-only notes use the new-note
  creation path described below, never an unconditional existing-note update.
- A GET checks the selected note and requires the response to advertise Notes
  API 1.2 or newer in `X-Notes-API-Versions`. Missing support fails closed.
- The server fields must match the saved baseline, and `readonly` must be false.
  A single PUT includes a quoted, strong `If-Match` tag from that checked note.
  A 412 response stops; there is no unconditional fallback or automatic retry.
- Successful response text must match the submitted text. Server-normalized
  title/category and the new baseline are committed locally in one transaction.
  Newer local edits are never replaced by that confirmation.
- A timeout, interrupted/malformed success response, or local confirmation
  failure is not reported as a confirmed upload. Check the server and refresh
  before retrying. The baseline does not advance on an unconfirmed result.
- Manual checkpoint: change only the local text of a disposable imported note,
  confirm its upload, and verify that same note in Nextcloud. Then refresh:
  the uploaded note should now be unchanged, not a local edit or conflict.
- Follow-up safety check: change the same note on both sides, then attempt
  upload without refreshing first. It must stop without replacing server text.

Protocol references: [conditional updates](https://github.com/nextcloud/notes/blob/main/docs/api/v1.md#update-note-put-notesid)
and [API version detection](https://github.com/nextcloud/notes/blob/main/docs/api/README.md#capabilites).

## Explicit new-note creation

1. Create a disposable note in RustyNotes with a unique title and some text.
2. Select **Upload selected note…**. Verify the dialog says it is local-only
   and will create one new server note.
3. Choose **Confirm create on Nextcloud**, then verify one matching server note.
4. Refresh: the same local note should remain, with no added duplicate.
5. Reopen Upload: it should now offer an existing-note update, not creation.

The implementation uses [POST /notes](https://github.com/nextcloud/notes/blob/main/docs/api/v1.md#create-note-post-notes).
Before sending that POST, SQLite commits an attempt record including the
server, account, local ID and submitted snapshot. There are no automatic
retries. A validated response supplies the remote ID; its snapshot is retained
and the association is committed transactionally. Server-normalized title and
category are adopted only if the local fields still match the submitted note.
Newer edits and local deletion during a request are preserved.

Current conservative limitation: any unconfirmed POST blocks creation for that
local note, across restart and account changes. Refresh does not clear this
guard. Use **Recover uploads…** to review a downloaded server copy and explicitly
transfer its association to the original local note. There is no automatic
matching or reset button: matching text alone cannot prove server identity.
If no eligible copy appears, stop and diagnose before attempting another creation.

## Interrupted new-note upload recovery

- This is local-only; use Refresh first to download possible server copies.
  The screen lists unfinished creation attempts whose original local notes
  still exist, with the recorded server/account. Deleted originals are not
  recreated and completed uploads do not appear.
- Candidates must belong to the attempt's exact server/account. If a server
  response ID was saved, only that ID is eligible. Otherwise the saved submitted
  content and favorite flag must match; the user must verify title/category,
  content and identity. Multiple matches remain separate choices.
- Locally edited downloaded copies, unresolved conflicts and unfinished
  creation attempts on the copy are excluded. Deletion or any change to the
  reviewed original/copy/baseline invalidates confirmation.
- Confirmation atomically moves the downloaded copy's server association to
  the original note, marks creation complete and retains the reviewed snapshots
  in a recovery-history table. Neither local note's text is changed or deleted.
  The extra copy is now local-only. Future explicit uploads from the original
  use the existing conditional-update path. Repeating the confirmation is safe.
- Manual checkpoint with no interrupted attempts: open **Recover uploads…**;
  expect “No interrupted uploads need recovery.” Close normally. Do not
  intentionally interrupt a real server write just to populate this screen.
  Database tests simulate lost responses and imported copies to exercise actual
  recovery, wrong-account rejection, ambiguous matches, stale comparisons,
  failed transactions and persistence across reopen.

## Single-note Markdown export

- Select **Export Markdown…** on the main toolbar. Pending saves finish before
  Rust reads the selected note; failed saves block export. The dialog prevents
  further editing while the file chooser is open.
- The native Save dialog suggests a sanitized title ending in `.md`. Choose a
  location or cancel. Existing-file replacement requires the native dialog's
  confirmation. Non-Markdown destinations and symbolic links are rejected.
- UTF-8 note content is exported exactly, without inserting a heading or
  category/favorite metadata. A sibling temporary file is fully written and
  synced before replacing the destination. No database or network write occurs.
- Manual checkpoint: edit a disposable note and immediately choose Export;
  save to a new `.md` file and open it in a text editor. Verify the latest text
  and Markdown syntax. Then test canceling the Save dialog: no file is written
  and RustyNotes returns to editing after closing the result dialog.

Native dialog reference: [Tauri Dialog](https://v2.tauri.app/plugin/dialog/).

## Single-file Markdown import

- **Import Markdown…** opens a native single-file chooser. Only regular `.md`
  files containing UTF-8 text, up to 4 MiB, are accepted. Invalid encoding,
  embedded NUL bytes, symbolic links, directories, and larger files are rejected.
- The filename without `.md` supplies the title. Text (including line endings
  and any UTF-8 BOM) is preserved. The new note uses Personal and is not a favorite.
- Each explicit import creates a separate local-only note, even if the same
  file or title was imported before. No existing note is overwritten, no source
  file is modified, and no server association or upload is created.
- The frontend waits for pending saves, then selects the new note and clears
  filters so it is visible. Cancel adds nothing. Uncertain command failures
  reload local notes but do not retry the insertion.
- Manual checkpoint: import the disposable `.md` exported earlier, close the
  result dialog, and verify the new note's title and exact text. The original
  note should still be present. Restart to check persistence. Test cancellation
  separately and verify the note count does not change.

## Selected-note sync status

- The editor footer reports **Local only**, **Local changes**, **Matches last
  server snapshot**, **Conflict saved**, or **Upload recovery needed**. Hover
  for an explanation and the associated server/account when one exists.
- All information comes from a consistent local database snapshot; no network
  request is made. A match never claims that the live server is unchanged.
- Unfinished creation attempts take priority. Unresolved saved conflicts take
  priority over a plain local-change comparison. Resolved historical conflicts
  do not keep the conflict status active. Modification times alone are ignored.
- Pending or failed local saves do not display a stale matching status. Responses
  for an old selection are ignored. Status is reread after modal operations close,
  including link recovery, upload, refresh, import and conflict resolution.
- Manual checkpoint: select a known uploaded note and expect **Matches last
  server snapshot**. Edit it locally and wait for **Saved locally**; expect
  **Local changes**. A new or Markdown-imported unlinked note says **Local only**.
  Conflict/recovery indicators link to their review screens.

## Failure handling

Next manual checkpoint: open the latest unresolved comparison, choose
**Keep my version**, then **Confirm local resolution**. Verify the local text
remains, the comparison records the choice, and an unchanged-server refresh
reports the note as a local edit kept rather than a conflict. Restart and verify
the saved choice and both snapshots remain. No server writes occur.

- A 404 polling response means authorization is still pending.
- Network/certificate errors pause polling. Fix the problem and choose
  **Retry login check**. Invalid certificates are never bypassed.
- If Keychain rejects the write after authorization, the one-time result stays
  in Rust memory for retry. Do not quit or cancel if you want to retry saving it.
- Login polling expires after 20 minutes. Cancel and authorize again.
- Cancel stops polling locally; it does not revoke an app password already
  granted in the browser. Revoke that grant in Nextcloud's Security settings
  if you no longer want it. Browser windows are not closed automatically.
- Redirects are rejected. Configure the final HTTPS base address, including
  a deployment subdirectory when applicable. Returned login/poll URLs must
  share its HTTPS origin, and the returned credential server must match its
  normalized base address. Reverse-proxy misconfiguration needs a server-side
  correction rather than weakening these checks.

## Implementation boundaries

- HTTP and authentication are Rust-only; credentials and polling tokens are
  never returned to React or stored in SQLite.
- macOS Keychain stores the credential bundle under service
  `com.rustynotes.nextcloud`, keyed by the normalized server address.
- Other platforms fail explicitly before starting login until their credential
  stores are implemented. No plaintext or in-memory mock-store fallback.
- HTTPS certificate validation is enabled; the application HTTP client does
  not use proxies, follow redirects, or include cookies.
- Opening the login page hands control to the user's browser. That browser's
  resources, extensions, and any server-configured external identity provider
  are outside the application's HTTP-client restrictions.
- No live server or real Keychain credentials are used by automated tests.

## Automated checks

```sh
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --offline
```

Reference: [Nextcloud Login Flow v2](https://docs.nextcloud.com/server/stable/developer_manual/client_apis/LoginFlow/index.html#login-flow-v2).
