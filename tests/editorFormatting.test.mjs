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

test('link insertion refuses edits made after selecting the link text', async () => {
  const { applyPendingLink } = await import('../src/editorFormatting.ts');
  const link = { content: 'hello world', start: 0, end: 5, text: 'hello', url: 'https://example.com' };
  assert.equal(applyPendingLink('NEW hello world', link), null);
  assert.equal(applyPendingLink('hello NEW world', link), null);
  assert.equal(applyPendingLink(link.content, link), '[hello](https://example.com) world');
});

test('extracted formatting preserves surrounding text and supports toggling lists', async () => {
  const { formatHeading, formatBulletedList, formatNumberedList, formatChecklist } = await import('../src/editorFormatting.ts');
  assert.equal(formatHeading('before\nTitle\nafter', 9).content, 'before\n# Title\nafter');
  assert.equal(formatHeading('###### Title', 8).content, 'Title');
  for (const [format, expected] of [[formatBulletedList, '- one\n\n- two'], [formatNumberedList, '1. one\n\n2. two'], [formatChecklist, '- [ ] one\n\n- [ ] two']]) {
    const original = 'one\n\ntwo';
    const result = format(original, 0, original.length);
    assert.equal(result.content, expected);
    assert.equal(format(result.content, 0, result.content.length).content, original);
    assert.equal(format('', 0, 0), null);
  }
});
