import { test } from 'node:test';
import assert from 'node:assert/strict';
import { acknowledgeSave, createSaveQueue } from '../src/saveQueue.ts';

test('writes finish in submission order even when the first write is delayed', async () => {
  const enqueue = createSaveQueue();
  const events = [];
  let release;
  const waiting = new Promise(resolve => { release = resolve; });
  const first = enqueue(async () => { await waiting; events.push('first'); });
  const second = enqueue(async () => { events.push('second'); });
  await Promise.resolve();
  assert.deepEqual(events, []);
  release();
  await Promise.all([first, second]);
  assert.deepEqual(events, ['first', 'second']);
});

test('a failed write does not prevent the next write', async () => {
  const enqueue = createSaveQueue();
  await assert.rejects(enqueue(async () => { throw new Error('disk full'); }));
  assert.equal(await enqueue(async () => 'saved'), 'saved');
});

test('an old save response cannot overwrite newer text', () => {
  const submitted = { id: 1, content: 'old', modified_at: 1 };
  const current = { ...submitted, content: 'new typing' };
  assert.equal(acknowledgeSave(current, submitted, { ...submitted, modified_at: 2 }), current);
});

test('a matching response updates only the modification timestamp', () => {
  const submitted = { id: 1, content: 'text', modified_at: 1 };
  assert.deepEqual(acknowledgeSave(submitted, submitted, {
    ...submitted, content: 'unexpected server replacement', modified_at: 2,
  }), { ...submitted, modified_at: 2 });
});
