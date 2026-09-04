import test from 'node:test';
import assert from 'node:assert/strict';
import { localChangeIds, localChangeLabel } from '../src/localChanges.ts';

test('local changes includes failed unsaved notes even if their saved snapshot matches', () => {
  const result = localChangeIds([{ id: 1, kind: 'local_only' }, { id: 2, kind: null }], [2, 3]);
  assert.deepEqual([...result], [1, 2, 3]);
});
test('empty local changes does not manufacture pending uploads', () => {
  assert.equal(localChangeIds([], []).size, 0);
});
test('review and recovery states are not labelled ready to upload', () => {
  assert.equal(localChangeLabel('conflict_saved'), 'Conflict — review first');
  assert.equal(localChangeLabel('recovery_needed'), 'Upload recovery needed');
  assert.equal(localChangeLabel(null), 'Sync status needs review');
});
