import test from 'node:test';
import assert from 'node:assert/strict';
import { matchesNoteFilter, noteFilterLabel } from '../src/noteFilters.ts';

test('built-in filters cannot collide with category names', () => {
  const notes = [
    { category: 'Favorites', favorite: false },
    { category: 'All Notes', favorite: false },
    { category: 'Work', favorite: true },
  ];
  const filter = value => notes.filter(note => matchesNoteFilter(note, value));
  assert.deepEqual(filter({ kind: 'all' }), notes);
  assert.deepEqual(filter({ kind: 'favorites' }), [notes[2]]);
  for (const [index, name] of ['Favorites', 'All Notes'].entries()) {
    assert.deepEqual(filter({ kind: 'category', name }), [notes[index]]);
    assert.equal(noteFilterLabel({ kind: 'category', name }), name);
  }
});
