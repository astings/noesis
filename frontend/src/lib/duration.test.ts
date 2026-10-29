// Run with `pnpm test` (Node's built-in runner, types stripped natively).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { formatDuration } from './duration.ts';

test('minutes under an hour', () => {
  assert.equal(formatDuration(2555.2), '43m');
  assert.equal(formatDuration(59), '1m');
  assert.equal(formatDuration(10), '0m');
});

test('hours with zero-padded minutes', () => {
  assert.equal(formatDuration(3600), '1h00');
  assert.equal(formatDuration(3900), '1h05');
});

test('missing or invalid gives empty string', () => {
  assert.equal(formatDuration(undefined), '');
  assert.equal(formatDuration(null), '');
  assert.equal(formatDuration(-5), '');
  assert.equal(formatDuration(NaN), '');
});
