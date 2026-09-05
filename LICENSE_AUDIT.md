# Third-party license audit

Audit date: 2026-09-05

This is an engineering inventory, not legal advice. It records the licenses
declared by RustyNotes' exact locked source dependencies. A new audit is needed
whenever `Cargo.lock` or `package-lock.json` changes.

## Results

- `cargo metadata --offline --locked` resolved 571 Rust packages across all
  platforms, build dependencies, and development dependencies. None lacked a
  license or license-file declaration.
- The installed locked JavaScript tree contained 169 unique package/version
  entries and 295 production dependency nodes. None of the inspected packages
  lacked a license declaration.
- No dependency declared AGPL or a mandatory GPL/LGPL license.
- `r-efi` offers MIT or Apache-2.0 as alternatives to LGPL-2.1-or-later;
  RustyNotes can use a permissive option.
- Five Rust packages declare MPL-2.0: `cssparser`, `cssparser-macros`,
  `dtoa-short`, `option-ext`, and `selectors`. MPL-2.0 is file-level copyleft;
  using these unmodified dependencies does not require relicensing RustyNotes,
  but their notices and source-availability terms still apply to distribution.
- Other observed declarations are permissive licenses such as MIT, Apache-2.0,
  BSD, ISC, Zlib, Unicode-3.0, 0BSD, Unlicense, CC0, BSL-1.0, and CC-BY-4.0.

The locked source dependency graph is compatible with RustyNotes' MIT license.
That conclusion does not remove the obligation to distribute applicable
third-party copyright and license notices.

## Binary-distribution checklist

- Generate and include third-party notices for statically bundled Rust and
  JavaScript components in release artifacts.
- Repeat this audit after any dependency-lockfile change.
- On Ubuntu, inspect the final `.deb` and AppImage contents. The AppImage build
  already copies and verifies many system-package copyright notices, but this
  is not yet a complete inventory of every bundled library.
- Confirm that every library actually bundled in the AppImage has its required
  notice and that any source-offer or relinking obligations are satisfied.
- Keep macOS signing/notarization separate from licensing verification.

## Commands used

```bash
cargo metadata --offline --locked --format-version 1 \
  --manifest-path src-tauri/Cargo.toml
npm ls --omit=dev --all --json
```

The individual package license values were read from Cargo metadata and the
installed packages' `package.json` files. The lockfiles remain the authoritative
version inputs.

Run `npm run audit:appimage` on the Ubuntu build machine to perform and record
the remaining binary-library notice check.
