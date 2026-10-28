import { expect, test } from 'bun:test';

import { hitSegmentIndex } from '../../src/lib/hitSegment';

const segs = [
  { timestamp: 0, text: 'hello' },
  { timestamp: 531.48, text: 'block starts here' },
  { timestamp: 560, text: 'nothing yet' },
  { timestamp: 600, text: "we'll be over Budget, the project" },
  { timestamp: 650, text: 'budget again but later' },
  { timestamp: 800, text: 'next block budget' },
];

test('lands on the segment holding the words, not the block start', () => {
  expect(hitSegmentIndex(segs, 531.48, 726.26, 'budg')).toBe(3);
});
test('more matching terms wins over earlier', () => {
  expect(hitSegmentIndex(segs, 531.48, 726.26, 'budget later')).toBe(4);
});
test('accents and case ignored', () => {
  expect(hitSegmentIndex([{ timestamp: 0, text: 'x' }, { timestamp: 1, text: 'Réunion' }], 0, 2, 'reunion')).toBe(1);
});
test('no term in block: block start', () => {
  expect(hitSegmentIndex(segs, 531.48, 726.26, 'zebra')).toBe(1);
});
test('block gone: last segment before start', () => {
  expect(hitSegmentIndex(segs, 700, 710, 'budget')).toBe(4);
});
