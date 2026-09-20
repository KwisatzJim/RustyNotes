import test from 'node:test';
import assert from 'node:assert/strict';
import { insertMarkdownTable } from '../src/editorFormatting.ts';

test('table insertion keeps surrounding text and creates Markdown block spacing', () => {
  const result = insertMarkdownTable('beforeafter', 6, 6);
  assert.equal(result.content, 'before\n\n| Column 1 | Column 2 |\n| --- | --- |\n| Cell 1 | Cell 2 |\n\nafter');
  assert.equal(result.content.slice(result.selectionStart, result.selectionEnd), 'Column 1');
});

test('selected text becomes the first table cell without losing pipes or lines', () => {
  const result = insertMarkdownTable('A | B\nC', 0, 7);
  assert.ok(result.content.includes('| A \\| B C | Cell 2 |'));
  assert.doesNotMatch(result.content, /^A \| B/m);
});
