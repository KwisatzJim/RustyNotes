// Run from the RustyNotes project root on Linux:
// node scripts/build-appimage.mjs
import { spawnSync } from 'node:child_process';
import { readdirSync, existsSync, readFileSync, lstatSync, mkdtempSync, mkdirSync, renameSync, realpathSync, writeFileSync, appendFileSync, rmSync } from 'node:fs';
import { resolve, join, dirname, basename, relative } from 'node:path';
import { homedir, tmpdir } from 'node:os';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { auditAppDir, installMissingNotices, writeAuditReport } from './audit-appimage-licenses.mjs';

export function queryWithLibraryAliases(args, query, canonical = realpathSync) {
  const original = query(args);
  // Leave every other dpkg-query operation and successful lookup untouched.
  if (original.status !== 1 || args.length !== 2 || args[0] !== '-S') return original;
  const path = args[1];
  if (!/^\/(?:usr\/)?lib(?:64)?\//.test(path)) return original;
  let resolved;
  try { resolved = canonical(path); } catch { return original; }
  const candidates = new Set([resolved,
    path.startsWith('/usr/') ? path.slice(4) : `/usr${path}`,
    resolved.startsWith('/usr/') ? resolved.slice(4) : `/usr${resolved}`]);
  for (const candidate of candidates) {
    if (candidate === path || !/^\/(?:usr\/)?lib(?:64)?\//.test(candidate)) continue;
    // Never substitute a similarly named but different library.
    try { if (canonical(candidate) !== resolved) continue; } catch { continue; }
    const result = query(['-S', candidate]);
    if (result.status === 0) return { ...result, recoveredPath: candidate };
  }
  return original;
}

export function noticeForQuery(stdout) {
  const lines = stdout.trim().split('\n');
  if (lines.length !== 1) throw new Error('Ambiguous recovered package ownership; stopping.');
  const match = /^([a-z0-9][a-z0-9+.-]*)(?::[a-z0-9-]+)?: \/.+$/.exec(lines[0]);
  if (!match) throw new Error('Unrecognized recovered package ownership; stopping.');
  return `/usr/share/doc/${match[1]}/copyright`;
}

function copyrightQueryMain(args) {
  const result = queryWithLibraryAliases(args, queryArgs => spawnSync('/usr/bin/dpkg-query', queryArgs, {
    encoding: 'utf8', env: { ...process.env, LC_ALL: 'C' },
  }));
  if (result.error) throw result.error;
  if (result.recoveredPath) {
    const notice = noticeForQuery(result.stdout);
    // Do not hide missing notices behind a successful ownership lookup.
    if (!existsSync(notice)) throw new Error(`Recovered package has no copyright notice: ${notice}`);
    appendFileSync(process.env.RUSTYNOTES_COPYRIGHT_LOG, `${notice}\n`);
    console.error(`RustyNotes: recovered copyright lookup via ${result.recoveredPath}`);
  }
  if (result.stdout) process.stdout.write(result.stdout);
  if (result.stderr) process.stderr.write(result.stderr);
  process.exitCode = result.status ?? 1;
}

export function verifyRecoveredNotices(directory, notices, read = readFileSync) {
  for (const notice of new Set(notices)) {
    if (!/^\/usr\/share\/doc\/[a-z0-9][a-z0-9+.-]*\/copyright$/.test(notice)) {
      throw new Error('Invalid recovered copyright path.');
    }
    const bundled = join(directory, notice.slice(1));
    if (!read(notice).equals(read(bundled))) throw new Error(`Bundled copyright notice differs: ${notice}`);
  }
}

function copyrightEnvironment(env, folder) {
  if (!existsSync('/usr/bin/dpkg-query')) throw new Error('Build this package on Ubuntu with /usr/bin/dpkg-query available.');
  // Scoped to these child processes only; no system tools are replaced.
  const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
  writeFileSync(join(folder, 'dpkg-query'), `#!/bin/sh\nexec ${quote(process.execPath)} ${quote(fileURLToPath(import.meta.url))} --copyright-dpkg-query "$@"\n`, { mode: 0o700 });
  const log = join(folder, 'recovered-notices');
  writeFileSync(log, '');
  return { ...env, PATH: `${folder}:${env.PATH || '/usr/bin:/bin'}`, RUSTYNOTES_COPYRIGHT_LOG: log };
}

export function packagingEnvironment(current) {
  // linuxdeploy reads semicolon-separated exclusion patterns, including in plugins.
  // Keep the host's Wayland client paired with its Mesa/EGL drivers.
  const exclusions = new Set((current.LINUXDEPLOY_EXCLUDED_LIBRARIES || '').split(';').filter(Boolean));
  exclusions.add('libwayland-client.so*');
  return { ...current, LINUXDEPLOY_EXCLUDED_LIBRARIES: [...exclusions].join(';') };
}

function bundledClients(directory) {
  if (!lstatSync(directory).isDirectory()) throw new Error('AppDir must be a real directory.');
  const conflicts = [];
  function inspect(folder) {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const path = join(folder, entry.name);
      // Check symlink names too; never traverse symlinked directories.
      if (/^libwayland-client\.so(?:\..*)?$/.test(entry.name)) conflicts.push(path);
      if (entry.isDirectory()) inspect(path);
    }
  }
  inspect(directory);
  return conflicts;
}

export function verifyAppDir(directory) {
  const conflicts = bundledClients(directory);
  if (conflicts.length) {
    throw new Error(`Wayland client is still bundled. Do not distribute this AppImage. Your cached linuxdeploy may not support the exclusion setting:\n${conflicts.join('\n')}`);
  }
  if (!existsSync(join(directory, 'AppRun'))) throw new Error('AppDir is incomplete: missing AppRun.');
}

export function repackWithoutWayland({ directory, image, tool, arch, env, run = spawnSync }) {
  const clients = bundledClients(directory);
  if (!clients.length) return;
  if (!existsSync(join(directory, 'AppRun'))) throw new Error('AppDir is incomplete: missing AppRun.');
  if (!existsSync(tool)) throw new Error(`Cannot find cached linuxdeploy: ${tool}`);
  if (!lstatSync(image).isFile()) throw new Error('Expected an ordinary AppImage output file.');
  if (clients.some(path => lstatSync(path).isDirectory())) throw new Error('Unexpected directory named as a Wayland library; stopping.');
  // Keep backups outside the appimage folder that Tauri clears on each build.
  const backup = mkdtempSync(join(dirname(dirname(directory)), 'appimage-backup-'));
  renameSync(image, join(backup, basename(image)));
  console.log(`Original AppImage and excluded libraries preserved in: ${backup}`);
  for (const path of clients) {
    const saved = join(backup, 'libraries', relative(directory, path));
    mkdirSync(dirname(saved), { recursive: true });
    renameSync(path, saved);
  }
  // GTK is already deployed. Do not run its input plugin again during repacking.
  const result = run(tool, ['--appimage-extract-and-run', '--appdir', directory,
    '--exclude-library=libwayland-client.so*', '--output', 'appimage'], {
    cwd: dirname(directory), stdio: 'inherit',
    env: { ...packagingEnvironment(env), OUTPUT: image, ARCH: arch, APPIMAGE_EXTRACT_AND_RUN: '1' },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Repack failed (${result.signal || result.status}). Original preserved in ${backup}.`);
  verifyAppDir(directory);
  if (!existsSync(image) || !lstatSync(image).isFile() || lstatSync(image).size === 0) {
    throw new Error(`Repack did not produce an AppImage. Original preserved in ${backup}.`);
  }
}

export function repackWithPackagedNotices({ directory, image, tool, arch, env, run = spawnSync }) {
  verifyAppDir(directory);
  if (!existsSync(tool)) throw new Error(`Cannot find cached linuxdeploy: ${tool}`);
  if (!lstatSync(image).isFile()) throw new Error('Expected an ordinary AppImage output file.');
  // Keep the pre-notice AppImage recoverable if linuxdeploy cannot repack the AppDir.
  const backup = mkdtempSync(join(dirname(dirname(directory)), 'appimage-pre-license-backup-'));
  renameSync(image, join(backup, basename(image)));
  console.log(`Pre-license AppImage preserved in: ${backup}`);
  // GTK and its dependencies are already deployed. Only rerun the AppImage output step.
  const result = run(tool, ['--appimage-extract-and-run', '--appdir', directory,
    '--exclude-library=libwayland-client.so*', '--output', 'appimage'], {
    cwd: dirname(directory), stdio: 'inherit',
    env: { ...packagingEnvironment(env), OUTPUT: image, ARCH: arch, APPIMAGE_EXTRACT_AND_RUN: '1' },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`License repack failed (${result.signal || result.status}). Previous AppImage preserved in ${backup}.`);
  verifyAppDir(directory);
  if (!existsSync(image) || !lstatSync(image).isFile() || lstatSync(image).size === 0) {
    throw new Error(`License repack did not produce an AppImage. Previous AppImage preserved in ${backup}.`);
  }
}

function build(root, buildEnv) {
  // Use a known output directory so we inspect the build we just requested.
  const result = spawnSync('npm', ['run', 'tauri', 'build', '--', '--bundles', 'appimage'], {
    cwd: root, stdio: 'inherit',
    env: { ...packagingEnvironment(buildEnv), CARGO_TARGET_DIR: join(root, 'src-tauri/target') },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`AppImage build failed (${result.signal || result.status}).`);
  const output = join(root, 'src-tauri/target/release/bundle/appimage');
  const entries = readdirSync(output, { withFileTypes: true });
  const appDirs = entries.filter(entry => entry.isDirectory() && entry.name.endsWith('.AppDir'));
  const images = entries.filter(entry => entry.isFile() && entry.name.endsWith('.AppImage'));
  if (appDirs.length !== 1 || images.length !== 1) {
    throw new Error('Cannot identify exactly one generated AppDir and AppImage. Do not distribute until inspected.');
  }
  const directory = join(output, appDirs[0].name);
  const image = join(output, images[0].name);
  const arch = { x64: 'x86_64', arm64: 'aarch64', arm: 'armhf', ia32: 'i386' }[process.arch];
  if (!arch) throw new Error(`Unsupported build architecture: ${process.arch}`);
  const cache = process.env.XDG_CACHE_HOME || join(homedir(), '.cache');
  const tool = join(cache, 'tauri', `linuxdeploy-${arch}.AppImage`);
  repackWithoutWayland({ directory, image, arch, env: buildEnv,
    tool });
  verifyAppDir(directory);
  const notices = readFileSync(buildEnv.RUSTYNOTES_COPYRIGHT_LOG, 'utf8').split('\n').filter(Boolean);
  verifyRecoveredNotices(directory, notices);
  console.log(`Verified recovered copyright notices in AppDir: ${new Set(notices).size}.`);
  const copiedNotices = installMissingNotices(directory);
  if (copiedNotices.length) {
    console.log(`Added ${copiedNotices.length} missing package copyright notice(s) to the AppDir.`);
    repackWithPackagedNotices({ directory, image, tool, arch, env: buildEnv });
  }
  const audit = auditAppDir(directory);
  const { reportPath } = writeAuditReport(output, directory, audit);
  if (audit.unresolved.length) {
    throw new Error(`AppImage license audit is incomplete. Review ${reportPath} before distribution.`);
  }
  console.log(`Verified AppImage library notices: ${audit.verified.length}; unresolved: 0.`);
  console.log(`License audit report: ${reportPath}`);
  console.log('Verified generated AppDir: no bundled libwayland-client.');
  console.log(`AppImage ready for normal-launch testing: ${join(output, images[0].name)}`);
}

function main() {
  if (process.platform !== 'linux') throw new Error('Build the AppImage on Linux, not macOS.');
  const root = process.cwd();
  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  if (pkg.name !== 'rustynotes' || !existsSync(join(root, 'src-tauri/tauri.conf.json'))) {
    throw new Error('Run this script from the RustyNotes project root.');
  }
  const folder = mkdtempSync(join(tmpdir(), 'rustynotes-copyright-'));
  try { build(root, copyrightEnvironment(process.env, folder)); }
  finally { rmSync(folder, { recursive: true, force: true }); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv[2] === '--copyright-dpkg-query') copyrightQueryMain(process.argv.slice(3));
    else main();
  }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
