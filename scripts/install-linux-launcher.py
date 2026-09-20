#!/usr/bin/env python3
"""Register an existing RustyNotes AppImage for the current Linux user."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def desktop_string(value):
    return str(value).replace("\\", "\\\\").replace(" ", "\\s")


def exec_path(path):
    value = str(path)
    # Avoid field-code expansion and unsupported executable names.
    if any(c in value for c in "\n\r\t%="):
        raise ValueError("Move the AppImage to a path without tabs, newlines, % or =.")
    quoted = "".join("\\" + c if c in '\\"`$' else c for c in value)
    return '"' + quoted.replace("\\", "\\\\") + '"'


def atomic_write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as output:
        temporary = Path(output.name)
        output.write(content)
    try:
        temporary.chmod(0o644)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def install(appimage, data_home):
    appimage = Path(appimage).expanduser().resolve(strict=True)
    if not appimage.is_file() or appimage.suffix.lower() != ".appimage":
        raise ValueError("Choose an existing RustyNotes .AppImage file.")
    command = exec_path(appimage)
    data_home = Path(data_home)
    if not data_home.is_absolute() or any(c in str(data_home) for c in "\n\r\t"):
        raise ValueError("XDG_DATA_HOME must be an absolute path without tabs or newlines.")
    source_icon = Path(__file__).resolve().parent.parent / "src-tauri/icons/128x128.png"
    icon_bytes = source_icon.read_bytes()
    icon = data_home / "icons/hicolor/128x128/apps/rustynotes.png"
    launcher = data_home / "applications/rustynotes.desktop"
    content = (
        "[Desktop Entry]\nType=Application\nName=RustyNotes\n"
        "Comment=Local-first Markdown notes\n"
        f"Exec={command}\nIcon={desktop_string(icon)}\n"
        "Terminal=false\nCategories=Office;\nStartupWMClass=Rustynotes\n"
    ).encode("utf-8")
    # Preserve custom launchers and icons before replacing them. Never overwrite
    # a previous backup, including on repeated installations.
    existing = [p for p in (launcher, icon) if p.exists() or p.is_symlink()]
    if existing:
        backup_root = data_home / "RustyNotes/launcher-backups"
        backup_root.mkdir(parents=True, exist_ok=True)
        backup = Path(tempfile.mkdtemp(prefix="before-install-", dir=backup_root))
        for path in existing:
            shutil.copy2(path, backup / path.name)
        print(f"Previous launcher/icon saved in: {backup}")
    appimage.chmod(appimage.stat().st_mode | 0o100)
    atomic_write(icon, icon_bytes)
    atomic_write(launcher, content)
    return launcher


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("appimage", help="Path to your trusted RustyNotes AppImage")
    args = parser.parse_args()
    if sys.platform != "linux" or os.geteuid() == 0:
        parser.error("Run this on Linux as your normal user, without sudo.")
    try:
        launcher = install(args.appimage, os.environ.get("XDG_DATA_HOME") or str(Path.home() / ".local/share"))
    except (OSError, ValueError) as error:
        parser.exit(1, f"Installation failed: {error}\n")
    updater = shutil.which("update-desktop-database")
    if updater:
        result = subprocess.run([updater, str(launcher.parent)], check=False)
        if result.returncode:
            print("Launcher installed, but refreshing the desktop database failed.")
    print(f"Installed: {launcher}")
    print("Open RustyNotes from Applications. Keep the AppImage at its current path.")
    print("On COSMIC, save your work and log out and back in if the dock icon is stale.")


if __name__ == "__main__":
    main()
