import test from 'node:test';
import assert from 'node:assert/strict';
import { initialTheme, oppositeTheme, saveTheme, THEME_KEY } from '../src/theme.ts';

test('saved theme wins over the system preference', () => {
  assert.equal(initialTheme(() => 'light', () => true), 'light');
  assert.equal(initialTheme(() => 'dark', () => false), 'dark');
});
test('first launch or invalid preference follows system', () => {
  for (const value of [null, '', 'invalid']) {
    assert.equal(initialTheme(() => value, () => true), 'dark');
    assert.equal(initialTheme(() => value, () => false), 'light');
  }
});
test('unavailable storage and system detection do not break startup', () => {
  const fail = () => { throw new Error('unavailable'); };
  assert.equal(initialTheme(fail, () => true), 'dark');
  assert.equal(initialTheme(fail, fail), 'light');
});
test('toggle and persistence round trip', () => {
  let saved = null;
  assert.equal(THEME_KEY, 'rustynotes.theme');
  assert.equal(saveTheme(oppositeTheme('light'), value => { saved = value; }), true);
  assert.equal(initialTheme(() => saved, () => false), 'dark');
  assert.equal(oppositeTheme('dark'), 'light');
  assert.equal(saveTheme('light', () => { throw new Error('full'); }), false);
});
