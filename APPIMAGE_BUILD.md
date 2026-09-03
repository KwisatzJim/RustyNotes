# AppImage build

On Linux, from the project root, use `npm run build:appimage`.
The equivalent command is `node scripts/build-appimage.mjs`.
The script can also be copied separately and run by absolute path while the
current directory is the project root.

## Ubuntu copyright lookup fallback

The Ubuntu build reported missing copyright files for libraries queried under
`/lib`, although dpkg recorded them under `/usr/lib`. The user verified this
for librsvg and confirmed its notice exists in `/usr/share/doc/librsvg2-2`.

During this build only, a temporary `dpkg-query` helper retries failed single
library ownership queries using equivalent `/lib` and `/usr/lib` paths. It
requires both paths to resolve to the same file. Successful queries, other
commands, and tool errors are left unchanged. No system tools are modified.
The helper uses `/usr/bin/dpkg-query` and records recovered notice paths.
After packaging, the script verifies those notices exist in the AppDir and
match the installed notice bytes. The temporary helper is removed afterward.
Unresolved upstream warnings are not suppressed. This is not a complete
third-party license audit or a redistribution-readiness guarantee.

## Wayland compatibility

This runs the normal Tauri AppImage build with
`LINUXDEPLOY_EXCLUDED_LIBRARIES=libwayland-client.so*`, preserving any additional
exclusions supplied by the caller. It then checks the generated AppDir for
remaining Wayland client files or symlinks. If any remain, it moves those files
and the original AppImage to a unique backup folder alongside the appimage
output folder. It repacks the prepared AppDir using cached linuxdeploy's explicit
`--exclude-library=libwayland-client.so*` option, without rerunning the GTK input
plugin. It checks again and fails if the client is reintroduced or output is
missing. Backups survive subsequent builds; their paths are printed.

Why: the Ubuntu-built AppImage rendered a blank window with EGL_BAD_PARAMETER
on the user's CachyOS AMD/Wayland system. Preloading that system's
libwayland-client fixed rendering and the Notes connection check. This build
excludes that client library so the installed system library can be used with
the host graphics drivers. The host therefore needs libwayland-client.so.0.
No other graphics libraries are deliberately excluded in this step.

The script does not set runtime LD_PRELOAD or disable accelerated rendering.
It does not modify the source application, credentials, notes, or installed
packages. It regenerates the AppImage build output under src-tauri/target;
retain any previous test artifacts elsewhere before rebuilding.
The ordinary Tauri AppImage command bypasses this wrapper. Debian packaging
and macOS builds are unchanged.

Verification: local script tests pass. The user confirmed normal launch without
environment overrides, notes visible, and Check Notes connection successful
for the same rebuilt AppImage on CachyOS, Ubuntu, and Pop_OS. This verifies
those workflows on those systems, not every distribution or graphics driver.
The copyright lookup fallback is unit-tested locally but still requires an
Ubuntu build test and review of any remaining warnings before distribution.

References:
- https://github.com/linuxdeploy/linuxdeploy/blob/master/src/core/appdir.cpp
- https://github.com/linuxdeploy/linuxdeploy/blob/master/src/core/copyright/copyright_dpkgquery.cpp
- https://v2.tauri.app/distribute/appimage/
