import { test } from 'node:test';
import assert from 'node:assert/strict';
import { layoutDay } from './agenda.ts';

const at = (h: number, m = 0) => new Date(2026, 10, 11, h, m);

test('overlapping meetings split into lanes, separate ones stay full width', () => {
  const out = layoutDay([
    { item: 'a', start: at(9), seconds: 3600 },
    { item: 'b', start: at(9, 30), seconds: 1800 },
    { item: 'c', start: at(14), seconds: 60 },
  ]);
  const by = Object.fromEntries(out.map((p) => [p.item, p]));
  assert.deepEqual([by.a.lane, by.a.lanes, by.b.lane, by.b.lanes], [0, 2, 1, 2]);
  assert.deepEqual([by.c.lane, by.c.lanes, by.c.startMin, by.c.endMin], [0, 1, 840, 860]);
});

test('a lane is reused once the earlier block ends', () => {
  const out = layoutDay([
    { item: 'a', start: at(9), seconds: 7200 },
    { item: 'b', start: at(9), seconds: 1800 },
    { item: 'c', start: at(10) },
  ]);
  const c = out.find((p) => p.item === 'c')!;
  assert.deepEqual([c.lane, c.lanes], [1, 2]);
});
