import test from 'node:test';
import assert from 'node:assert/strict';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { MarkdownPreview, MAX_PREVIEW_CHARACTERS } from '../src/MarkdownPreview.ts';

const render = content => renderToStaticMarkup(createElement(MarkdownPreview, { content }));

test('preview renders headings emphasis lists quotes code and GFM tables', () => {
  const html = render('# Heading\n\n**bold** *italic* ~~old~~\n\n- one\n- two\n\n1. first\n\n> quote\n\n`inline`\n\n```rust\nlet n = 1;\n```\n\n| A | B |\n|---|---|\n| x | y |');
  for (const tag of ['h1', 'strong', 'em', 'del', 'ul', 'ol', 'blockquote', 'pre', 'code', 'table']) {
    assert.ok(html.includes(`<${tag}`), tag);
  }
});
test('preview task checkboxes are read-only', () => {
  const html = render('- [x] done\n- [ ] todo');
  assert.equal((html.match(/disabled=""/g) || []).length, 2);
  assert.equal((html.match(/checked=""/g) || []).length, 1);
});
test('links and images never create navigations fetches or resource hints', () => {
  const html = render('[site](https://example.com) ![remote](https://example.com/tracker.png) ![local](file:///etc/passwd) ![data](data:image/png;base64,AAAA)\n\nhttps://example.com');
  assert.ok(html.includes('remote — not loaded'));
  assert.doesNotMatch(html, /<(a|img|link|iframe|video|audio)\b|\s(?:href|src|srcset)=/i);
});
test('raw HTML scripts and handlers cannot become executable DOM', () => {
  const html = render('<script>alert(1)</script>\n\n<img src="https://example.com" onerror="alert(2)">\n\n<iframe src="https://example.com"></iframe>\n\n[sneaky](javascript:alert%281%29)\n\n<style>body{display:none}</style>');
  assert.doesNotMatch(html, /<(script|img|iframe|style)\b|onerror=|href=/i);
});
test('plain text stays text and original Markdown is unchanged', () => {
  const content = '日本語 🌍 **bold**\r\n\n`<script>example</script>`';
  const before = content;
  assert.ok(render(content).includes('&lt;script&gt;'));
  assert.equal(content, before);
});
test('empty and oversized notes have safe non-destructive fallbacks', () => {
  assert.ok(render('  ').includes('This note is empty'));
  assert.ok(render('x'.repeat(MAX_PREVIEW_CHARACTERS + 1)).includes('too large to preview'));
});
