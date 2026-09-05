import assert from "node:assert/strict";
import test from "node:test";
import { extractBuildId, missingNoticePlan, parseDpkgOwners } from "../scripts/audit-appimage-licenses.mjs";

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

test("AppImage audit plans one notice per package and keeps unresolved libraries", () => {
  const entries = [
    { relativeLibrary: "usr/lib/one.so", packages: ["shared-package"], buildId: "aa" },
    { relativeLibrary: "usr/lib/two.so", packages: ["shared-package"], buildId: "bb" },
    { relativeLibrary: "usr/lib/three.so", packages: ["missing-package"], buildId: "cc" },
    { relativeLibrary: "usr/lib/four.so", packages: [], buildId: null },
  ];
  const plan = missingNoticePlan(entries, path => path.includes("shared-package"));
  assert.deepEqual(plan.packages, ["shared-package"]);
  assert.deepEqual(plan.unresolved, [
    "usr/lib/three.so\tmissing-package\tcc",
    "usr/lib/four.so\tno matching installed package\tno build ID",
  ]);
});
