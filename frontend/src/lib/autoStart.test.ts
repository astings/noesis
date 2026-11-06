// Run with `pnpm test` (Node's built-in runner, types stripped natively).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { canAutoStartNow } from './autoStart.ts';

test('auto-start waits while the previous recording is still being stopped or saved', () => {
  for (const busy of ['starting', 'recording', 'stopping', 'processing', 'saving', 'completed']) {
    assert.equal(canAutoStartNow(busy), false, busy);
  }
});

test('auto-start proceeds once idle, or after a failed save', () => {
  assert.equal(canAutoStartNow('idle'), true);
  assert.equal(canAutoStartNow('error'), true);
});
