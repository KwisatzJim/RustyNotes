# RustyNotes desktop icon

`source.svg` is the editable source: a rust-orange notebook with cream writing
and a teal bookmark. The desktop PNG, ICNS, and ICO files were generated from
this source with the project's installed Tauri CLI. No external artwork,
fonts, or linked resources are used.

To regenerate, run `npm run tauri -- icon src-tauri/icons/source.svg` from the
project root. This also creates mobile icon folders; RustyNotes currently uses
only the desktop assets listed in `src-tauri/tauri.conf.json`.

Icon changes appear in newly built packages; they do not update previously
copied or installed AppImages or Debian packages.
