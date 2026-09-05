import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url)));
const production = config.app.security.csp;
const development = config.app.security.devCsp;

test("project manifests and license file consistently declare MIT", () => {
  const packageManifest = JSON.parse(readFileSync(new URL("../package.json", import.meta.url)));
  const cargoManifest = readFileSync(new URL("../src-tauri/Cargo.toml", import.meta.url), "utf8");
  const license = readFileSync(new URL("../LICENSE", import.meta.url), "utf8");
  assert.equal(packageManifest.license, "MIT");
  assert.match(cargoManifest, /^license = "MIT"$/m);
  assert.match(license, /^MIT License$/m);
  assert.match(license, /Copyright \(c\) 2026 Jim Kelley/);
});

test("application metadata contains RustyNotes branding instead of starter labels", () => {
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  assert.equal(config.productName, "RustyNotes");
  assert.equal(config.app.windows[0].title, "RustyNotes");
  assert.match(html, /<title>RustyNotes<\/title>/);
  assert.match(html, /href="\/rustynotes\.svg"/);
  assert.doesNotMatch(html, /Vite|Tauri \+ React/i);
  assert.doesNotThrow(() => readFileSync(new URL("../public/rustynotes.svg", import.meta.url)));
});

test("production CSP permits Tauri IPC without remote web or websocket access", () => {
  assert.equal(production["default-src"], "'self'");
  assert.equal(production["connect-src"], "ipc: http://ipc.localhost");
  assert.equal(production["object-src"], "'none'");
  assert.equal(production["base-uri"], "'none'");
  assert.equal(production["form-action"], "'none'");
  assert.equal(production["frame-ancestors"], "'none'");
  assert.doesNotMatch(JSON.stringify(production), /https:|ws:|\*/);
});

test("only development CSP permits Vite websocket reloads", () => {
  assert.match(development["connect-src"], /\bws:/);
  assert.doesNotMatch(production["connect-src"], /\bws:/);
  for (const directive of ["object-src", "base-uri", "form-action", "frame-ancestors"]) {
    assert.equal(development[directive], "'none'");
  }
});
