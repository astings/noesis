import { test } from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {
  INBOX, recordingsRoot, isValidProjectName, slug, isAutoTitle, stampOfFolder, meetingFolderName, formatMmss, toLocalIso, parseBound,
} from '../src/paths.ts';

test('project names are single normal components', () => {
  for (const ok of ['dex', INBOX, 'flowers-2026', 'Recruiting']) assert.equal(isValidProjectName(ok), true, ok);
  for (const bad of ['', '  ', '.', '..', '../x', '/tmp/x', 'a/b', 'a\\b', '.hidden']) assert.equal(isValidProjectName(bad), false, bad);
});

test('slug lowercases, collapses and caps at 60', () => {
  assert.equal(slug('Western Digital: onboarding (P3)'), 'western-digital-onboarding-p3');
  assert.equal(slug('  --Hello--  '), 'hello');
  assert.equal(slug(''), '');
  assert.equal(slug('x'.repeat(100)).length, 60);
});

test('auto titles are recognized', () => {
  assert.equal(isAutoTitle('Meeting 27_10_26_12_03_07'), true);
  assert.equal(isAutoTitle('Meeting 2026-10-27_12-03-07'), true);
  assert.equal(isAutoTitle('Western Digital onboarding'), false);
});

test('folder names carry the stamp and an optional slug', () => {
  assert.equal(meetingFolderName('2026-10-27_12-03', null), '2026-10-27_12-03');
  assert.equal(meetingFolderName('2026-10-27_12-03', 'Meeting 27_10_26_12_03_07'), '2026-10-27_12-03');
  assert.equal(meetingFolderName('2026-10-27_12-03', 'Western Digital onboarding'), '2026-10-27_12-03 western-digital-onboarding');
  assert.equal(stampOfFolder('2026-10-27_12-03 western-digital-onboarding'), '2026-10-27_12-03');
  assert.equal(stampOfFolder('Meeting 27_10_26_12_03_07_2026-10-27_16-03'), null);
});

test('mmss and local iso', () => {
  assert.equal(formatMmss(65.4), '01:05');
  assert.equal(formatMmss(3600), '60:00');
  assert.match(toLocalIso('2026-10-27T16:03:09.210153+00:00'), /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:\d{2}$/);
  assert.equal(toLocalIso('not a date'), 'not a date');
});

test('parseBound: date-only covers the local day, datetimes pass through, junk is NaN', () => {
  assert.equal(parseBound('2026-10-27', false), new Date(2026, 9, 27).getTime());
  assert.equal(parseBound('2026-10-27', true), new Date(2026, 9, 27, 23, 59, 59, 999).getTime());
  assert.equal(parseBound('2026-10-27T10:00:00Z', false), Date.parse('2026-10-27T10:00:00Z'));
  assert.ok(Number.isNaN(parseBound('10/27/2026', false)));
});

test('recordingsRoot: env vars, then existing dirs, noesis by default', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'noesis-paths-'));
  const movies = (n: string) => path.join(home, 'Movies', n);
  assert.equal(recordingsRoot({ NOESIS_RECORDINGS: '/a', MEETILY_RECORDINGS: '/b' }, home), '/a');
  assert.equal(recordingsRoot({ MEETILY_RECORDINGS: '/b' }, home), '/b');
  assert.equal(recordingsRoot({}, home), movies('noesis-recordings'));
  fs.mkdirSync(movies('meetily-recordings'), { recursive: true });
  assert.equal(recordingsRoot({}, home), movies('meetily-recordings'));
  fs.mkdirSync(movies('noesis-recordings'));
  assert.equal(recordingsRoot({}, home), movies('noesis-recordings'));
  fs.rmSync(home, { recursive: true });
});
