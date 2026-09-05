import assert from "node:assert/strict";
import test from "node:test";
import { extractBuildId, parseDpkgOwners } from "../scripts/audit-appimage-licenses.mjs";

test("AppImage audit parses Debian package owners without confusing architecture", () => {
  assert.deepEqual(
    parseDpkgOwners("libgtk-3-0:amd64: /usr/lib/x86_64-linux-gnu/libgtk-3.so.0\nlibrsvg2-2: /usr/lib/librsvg-2.so.2\ninvalid\n"),
    [
      { packageName: "libgtk-3-0", path: "/usr/lib/x86_64-linux-gnu/libgtk-3.so.0" },
      { packageName: "librsvg2-2", path: "/usr/lib/librsvg-2.so.2" },
    ],
  );
});

test("AppImage audit accepts only a hexadecimal ELF build ID", () => {
  assert.equal(extractBuildId("    Build ID: A1b2c3d4\n"), "a1b2c3d4");
  assert.equal(extractBuildId("Build ID: unsafe/value\n"), null);
  assert.equal(extractBuildId("no notes"), null);
});
