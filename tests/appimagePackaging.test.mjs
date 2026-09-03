import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, symlinkSync, rmSync, readFileSync, readdirSync, existsSync, readlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { packagingEnvironment, verifyAppDir, repackWithoutWayland, queryWithLibraryAliases, noticeForQuery, verifyRecoveredNotices } from '../scripts/build-appimage.mjs';

test('copyright lookup retries the equivalent usr library path', () => {
  const original = '/lib/x86_64-linux-gnu/librsvg-2.so.2';
  const alias = `/usr${original}`;
  const calls = [];
  const result = queryWithLibraryAliases(['-S', original], args => {
    calls.push(args);
    return args[1] === alias
      ? { status: 0, stdout: `librsvg2-2:amd64: ${alias}\n` }
      : { status: 1, stdout: '', stderr: 'no path found' };
  }, () => alias);
  assert.equal(result.status, 0);
  assert.equal(result.recoveredPath, alias);
  assert.equal(noticeForQuery(result.stdout), '/usr/share/doc/librsvg2-2/copyright');
  assert.deepEqual(calls, [['-S', original], ['-S', alias]]);
});

test('copyright lookup supports reverse aliases and fully resolved symlinks', () => {
  const path = '/usr/lib/libexample.so.1';
  const resolved = '/usr/lib/libexample.so.1.2';
  const result = queryWithLibraryAliases(['-S', path], args => ({
    status: args[1] === '/lib/libexample.so.1.2' ? 0 : 1,
    stdout: 'example: /lib/libexample.so.1.2\n',
  }), () => resolved);
  assert.equal(result.recoveredPath, '/lib/libexample.so.1.2');
});

test('copyright lookup preserves success, other commands, and tool failures', () => {
  for (const [args, status] of [[['-S', '/lib/a.so'], 0], [['-L', 'package'], 1],
    [['-S', '/appdir/usr/lib/a.so'], 1], [['-S', '/lib/a.so'], 2], [['-S', '/lib/a.so'], null]]) {
    const original = { status, stdout: 'output', stderr: 'error' };
    let count = 0;
    assert.equal(queryWithLibraryAliases(args, () => { count++; return original; }, () => assert.fail()), original);
    assert.equal(count, 1);
  }
});

test('copyright lookup will not use an alias pointing at a different file', () => {
  const original = { status: 1, stdout: '', stderr: 'original error' };
  let count = 0;
  const result = queryWithLibraryAliases(['-S', '/lib/a.so'], () => { count++; return original; }, path => path);
  assert.equal(result, original);
  assert.equal(count, 1);
});

test('copyright lookup preserves errors when paths do not exist or aliases fail', () => {
  const failure = { status: 1, stdout: '', stderr: 'original error' };
  assert.equal(queryWithLibraryAliases(['-S', '/lib/a.so'], () => failure, () => { throw new Error('missing'); }), failure);
  assert.equal(queryWithLibraryAliases(['-S', '/lib/a.so'], () => failure, () => '/usr/lib/a.so'), failure);
});

test('copyright ownership parser refuses ambiguous and unsafe output', () => {
  assert.equal(noticeForQuery('libfoo: /usr/lib/libfoo.so\n'), '/usr/share/doc/libfoo/copyright');
  for (const output of ['first, second: /usr/lib/foo', '../escape: /usr/lib/foo',
    'first: /usr/lib/foo\nsecond: /usr/lib/foo', '']) {
    assert.throws(() => noticeForQuery(output), /ownership/);
  }
});

test('recovered notices must be present and byte-identical in the AppDir', () => {
  const source = '/usr/share/doc/librsvg2-2/copyright';
  const calls = [];
  verifyRecoveredNotices('/bundle/app.AppDir', [source, source], path => {
    calls.push(path);
    return Buffer.from('notice');
  });
  assert.deepEqual(calls, [source, `/bundle/app.AppDir${source}`]);
  assert.throws(() => verifyRecoveredNotices('/bundle', [source], path => Buffer.from(path)), /differs/);
  assert.throws(() => verifyRecoveredNotices('/bundle', [source], () => { throw new Error('missing file'); }), /missing file/);
  assert.throws(() => verifyRecoveredNotices('/bundle', ['/usr/share/doc/../copyright']), /Invalid/);
});

test('AppImage exclusion preserves the caller environment without modifying it', () => {
  const original = { PATH: '/test', LINUXDEPLOY_EXCLUDED_LIBRARIES: 'existing.so*' };
  const result = packagingEnvironment(original);
  assert.equal(result.LINUXDEPLOY_EXCLUDED_LIBRARIES, 'existing.so*;libwayland-client.so*');
  assert.equal(original.LINUXDEPLOY_EXCLUDED_LIBRARIES, 'existing.so*');
  assert.equal(result.PATH, original.PATH);
  assert.equal(result.LD_PRELOAD, undefined);
  assert.deepEqual(packagingEnvironment(result), result);
});

function fixture(t) {
  const path = mkdtempSync(join(tmpdir(), 'rustynotes-packaging-test-'));
  t.after(() => rmSync(path, { recursive: true, force: true }));
  mkdirSync(join(path, 'usr/lib'), { recursive: true });
  writeFileSync(join(path, 'AppRun'), 'test fixture');
  return path;
}

test('verification accepts unrelated bundled libraries', t => {
  const path = fixture(t);
  writeFileSync(join(path, 'usr/lib/libwebkit2gtk-4.1.so.0'), 'fixture');
  assert.doesNotThrow(() => verifyAppDir(path));
});

test('verification rejects a bundled Wayland client version', t => {
  const path = fixture(t);
  writeFileSync(join(path, 'usr/lib/libwayland-client.so.0.22.0'), 'fixture');
  assert.throws(() => verifyAppDir(path), /Wayland client is still bundled/);
});

test('verification also rejects a Wayland client symlink', t => {
  const path = fixture(t);
  symlinkSync('missing-target', join(path, 'usr/lib/libwayland-client.so.0'));
  assert.throws(() => verifyAppDir(path), /Wayland client is still bundled/);
});

test('verification rejects an incomplete AppDir', t => {
  const path = fixture(t);
  assert.throws(() => verifyAppDir(join(path, 'usr')), /missing AppRun/);
});

function repackFixture(t) {
  const root = fixture(t);
  const directory = join(root, 'appimage/rustynotes.AppDir');
  mkdirSync(join(directory, 'usr/lib'), { recursive: true });
  writeFileSync(join(directory, 'AppRun'), 'launcher');
  writeFileSync(join(directory, 'usr/lib/libwayland-client.so.0.22.0'), 'original library');
  symlinkSync('libwayland-client.so.0.22.0', join(directory, 'usr/lib/libwayland-client.so.0'));
  writeFileSync(join(directory, 'usr/lib/unrelated.so'), 'keep');
  const image = join(root, 'appimage/test.AppImage');
  const tool = join(root, 'linuxdeploy');
  writeFileSync(image, 'original image');
  writeFileSync(tool, 'mock tool');
  return { root, directory, image, tool, arch: 'x86_64', env: { PATH: '/test' } };
}

test('repack preserves originals and invokes an explicit exclusion without GTK plugin', t => {
  const f = repackFixture(t);
  repackWithoutWayland({ ...f, run(tool, args, options) {
    assert.equal(tool, f.tool);
    assert.ok(args.includes('--exclude-library=libwayland-client.so*'));
    assert.ok(!args.includes('--plugin'));
    assert.equal(options.env.OUTPUT, f.image);
    assert.equal(options.env.ARCH, 'x86_64');
    assert.equal(options.env.APPIMAGE_EXTRACT_AND_RUN, '1');
    assert.equal(options.env.PATH, '/test');
    verifyAppDir(f.directory);
    assert.ok(!existsSync(f.image));
    writeFileSync(f.image, 'rebuilt image');
    return { status: 0 };
  } });
  const backup = join(f.root, readdirSync(f.root).find(name => name.startsWith('appimage-backup-')));
  assert.equal(readFileSync(join(backup, 'test.AppImage'), 'utf8'), 'original image');
  assert.equal(readFileSync(join(backup, 'libraries/usr/lib/libwayland-client.so.0.22.0'), 'utf8'), 'original library');
  assert.equal(readlinkSync(join(backup, 'libraries/usr/lib/libwayland-client.so.0')), 'libwayland-client.so.0.22.0');
  assert.equal(readFileSync(join(f.directory, 'usr/lib/unrelated.so'), 'utf8'), 'keep');
  assert.equal(readFileSync(join(f.directory, 'AppRun'), 'utf8'), 'launcher');
});

test('repack fails closed if linuxdeploy reintroduces the client', t => {
  const f = repackFixture(t);
  assert.throws(() => repackWithoutWayland({ ...f, run() {
    writeFileSync(join(f.directory, 'usr/lib/libwayland-client.so.0'), 'reintroduced');
    writeFileSync(f.image, 'bad image');
    return { status: 0 };
  } }), /Wayland client is still bundled/);
});

test('repack reports failed command and missing output', t => {
  const f = repackFixture(t);
  assert.throws(() => repackWithoutWayland({ ...f, run: () => ({ status: 1 }) }), /Repack failed/);
  const other = repackFixture(t);
  assert.throws(() => repackWithoutWayland({ ...other, run: () => ({ status: 0 }) }), /did not produce/);
});

test('clean AppDir does not invoke repacking', t => {
  repackWithoutWayland({ directory: fixture(t), run() { assert.fail('unexpected repack'); } });
});
