// Run on the Ubuntu machine that built the AppImage, from the project root.
import { copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, writeFileSync } from 'node:fs';
import { basename, join, relative, resolve, sep } from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export function parseDpkgOwners(output) {
  return output.split('\n').filter(Boolean).map(line => {
    const separator = line.indexOf(': ');
    if (separator < 1) return null;
    const packageWithArch = line.slice(0, separator);
    const path = line.slice(separator + 2);
    const packageName = packageWithArch.replace(/:[a-z0-9-]+$/, '');
    return packageName && path.startsWith('/') ? { packageName, path } : null;
  }).filter(Boolean);
}

export function extractBuildId(output) {
  return /^\s*Build ID:\s*([0-9a-f]+)\s*$/im.exec(output)?.[1]?.toLowerCase() ?? null;
}

function command(commandName, args) {
  const result = spawnSync(commandName, args, {
    encoding: 'utf8',
    env: { ...process.env, LC_ALL: 'C' },
  });
  if (result.error) throw result.error;
  return result;
}

function buildId(path) {
  const result = command('/usr/bin/readelf', ['-n', path]);
  return result.status === 0 ? extractBuildId(result.stdout) : null;
}

function libraries(directory) {
  const root = realpathSync(directory);
  const found = new Map();
  function walk(folder) {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const path = join(folder, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.isFile() || entry.isSymbolicLink()) {
        let actual;
        try { actual = realpathSync(path); } catch { continue; }
        if (actual !== root && !actual.startsWith(`${root}${sep}`)) throw new Error(`Library link escapes AppDir: ${path}`);
        if (!lstatSync(actual).isFile()) continue;
        const header = readFileSync(actual).subarray(0, 4);
        if (header.equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))) found.set(actual, path);
      }
    }
  }
  walk(root);
  return [...found.entries()].map(([actual, displayed]) => ({ actual, displayed }));
}

function matchingPackages(library) {
  const id = buildId(library.actual);
  if (!id) return { id: null, packages: [] };
  const query = command('/usr/bin/dpkg-query', ['-S', `*${basename(library.actual)}`]);
  const owners = parseDpkgOwners(query.stdout);
  const packages = owners.filter(owner => {
    try { return existsSync(owner.path) && buildId(owner.path) === id; } catch { return false; }
  }).map(owner => owner.packageName);
  return { id, packages: [...new Set(packages)] };
}

function inspectAppDir(appDir) {
  const libDir = join(appDir, 'usr', 'lib');
  if (!lstatSync(libDir).isDirectory()) throw new Error(`Missing AppDir library folder: ${libDir}`);
  return libraries(libDir).map(library => {
    const match = matchingPackages(library);
    return {
      relativeLibrary: relative(appDir, library.displayed),
      buildId: match.id,
      packages: match.packages,
    };
  });
}

export function missingNoticePlan(entries, noticeExists = existsSync) {
  const packages = new Set();
  const unresolved = [];
  for (const entry of entries) {
    const packageName = entry.packages.find(candidate => noticeExists(`/usr/share/doc/${candidate}/copyright`));
    if (packageName) packages.add(packageName);
    else unresolved.push(`${entry.relativeLibrary}\t${entry.packages.join(',') || 'no matching installed package'}\t${entry.buildId || 'no build ID'}`);
  }
  return { packages: [...packages].sort(), unresolved };
}

export function installMissingNotices(appDir) {
  const plan = missingNoticePlan(inspectAppDir(appDir));
  if (plan.unresolved.length) {
    throw new Error(`Cannot package complete library notices:\n${plan.unresolved.join('\n')}`);
  }
  const copied = [];
  for (const packageName of plan.packages) {
    const source = `/usr/share/doc/${packageName}/copyright`;
    const destination = join(appDir, 'usr', 'share', 'doc', packageName, 'copyright');
    if (existsSync(destination)) {
      if (!readFileSync(source).equals(readFileSync(destination))) {
        throw new Error(`Bundled copyright notice differs: ${source}`);
      }
      continue;
    }
    mkdirSync(join(appDir, 'usr', 'share', 'doc', packageName), { recursive: true });
    copyFileSync(source, destination);
    if (!readFileSync(source).equals(readFileSync(destination))) {
      throw new Error(`Copied copyright notice differs: ${source}`);
    }
    copied.push(packageName);
  }
  return copied;
}

export function auditAppDir(appDir) {
  const verified = [];
  const unresolved = [];
  for (const entry of inspectAppDir(appDir)) {
    const accepted = entry.packages.find(packageName => {
      const installed = `/usr/share/doc/${packageName}/copyright`;
      const bundled = join(appDir, 'usr', 'share', 'doc', packageName, 'copyright');
      try { return readFileSync(installed).equals(readFileSync(bundled)); } catch { return false; }
    });
    if (accepted) verified.push(`${entry.relativeLibrary}\t${accepted}\t${entry.buildId}`);
    else unresolved.push(`${entry.relativeLibrary}\t${entry.packages.join(',') || 'no matching installed package'}\t${entry.buildId || 'no build ID'}`);
  }
  return { verified, unresolved };
}

export function writeAuditReport(output, appDir, audit) {
  const report = [`AppImage license audit`, `AppDir: ${appDir}`, `Verified libraries: ${audit.verified.length}`, `Unresolved libraries: ${audit.unresolved.length}`, '', '[verified]', ...audit.verified, '', '[unresolved]', ...audit.unresolved, ''].join('\n');
  const reportPath = join(output, 'appimage-license-audit.txt');
  writeFileSync(reportPath, report);
  return { report, reportPath };
}

function main() {
  if (process.platform !== 'linux') throw new Error('Run the AppImage license audit on its Ubuntu build machine.');
  for (const tool of ['/usr/bin/readelf', '/usr/bin/dpkg-query']) {
    if (!existsSync(tool)) throw new Error(`Required Ubuntu tool not found: ${tool}`);
  }
  const output = resolve('src-tauri/target/release/bundle/appimage');
  const appDirs = readdirSync(output, { withFileTypes: true }).filter(entry => entry.isDirectory() && entry.name.endsWith('.AppDir'));
  if (appDirs.length !== 1) throw new Error('Expected exactly one generated AppDir. Build the AppImage first.');
  const appDir = join(output, appDirs[0].name);
  const audit = auditAppDir(appDir);
  const { report, reportPath } = writeAuditReport(output, appDir, audit);
  console.log(report);
  console.log(`Audit report: ${reportPath}`);
  if (audit.unresolved.length) throw new Error('AppImage license audit is incomplete. Review every unresolved library before distribution.');
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
