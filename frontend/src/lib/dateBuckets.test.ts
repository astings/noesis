// Run with `pnpm test` (Node's built-in runner, types stripped natively).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bucketOf, groupByDate, parseDate, rowLabel } from './dateBuckets.ts';

const at = (y: number, m: number, d: number, h = 12, min = 0) => new Date(y, m - 1, d, h, min);
const wed = at(2026, 10, 28); // Wednesday
const label = (d: Date | null, now = wed) => bucketOf(d, now).label;

test('day buckets split at local midnight', () => {
  assert.equal(label(at(2026, 10, 28, 0, 0)), 'Today');
  assert.equal(label(at(2026, 10, 27, 23, 59)), 'Yesterday');
});

test('weeks start on Monday', () => {
  assert.equal(label(at(2026, 10, 26)), 'This week');
  assert.equal(label(at(2026, 10, 25)), 'Last week');
  assert.equal(label(at(2026, 10, 19, 0, 0)), 'Last week');
  assert.equal(label(at(2026, 10, 18, 23, 59)), 'October 2026');
});

test('on a Monday, Sunday is Yesterday and Saturday is Last week', () => {
  const mon = at(2026, 10, 26, 9);
  assert.equal(label(at(2026, 10, 25), mon), 'Yesterday');
  assert.equal(label(at(2026, 10, 24), mon), 'Last week');
});

test('month buckets cross the year boundary', () => {
  assert.equal(label(at(2025, 12, 31)), 'December 2025');
  assert.equal(label(at(2026, 1, 1)), 'January 2026');
});

test('missing or invalid dates are Undated', () => {
  assert.equal(label(null), 'Undated');
  assert.equal(parseDate('not a date'), null);
  assert.equal(parseDate(undefined), null);
});

test('row labels per bucket kind', () => {
  const mon = at(2026, 10, 26, 11);
  assert.match(rowLabel(mon, bucketOf(mon, wed)), /^Mon \d/);
  const sep = at(2026, 9, 28);
  assert.equal(rowLabel(sep, bucketOf(sep, wed)), 'Sep 28');
  const today = at(2026, 10, 28, 9, 30);
  assert.match(rowLabel(today, bucketOf(today, wed)), /09.30/);
});

test('groupByDate orders buckets and rows, Undated always last', () => {
  const items = [
    { id: 'old', d: at(2026, 9, 1) },
    { id: 'none', d: null },
    { id: 'today-am', d: at(2026, 10, 28, 9) },
    { id: 'today-pm', d: at(2026, 10, 28, 11) },
  ];
  const ids = (order: 'newest' | 'oldest') =>
    groupByDate(items, (i) => i.d, order, wed).map((g) => `${g.bucket.label}:${g.items.map((i) => i.id).join(',')}`);
  assert.deepEqual(ids('newest'), ['Today:today-pm,today-am', 'September 2026:old', 'Undated:none']);
  assert.deepEqual(ids('oldest'), ['September 2026:old', 'Today:today-am,today-pm', 'Undated:none']);
});
