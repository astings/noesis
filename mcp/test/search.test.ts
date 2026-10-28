import { test, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { scanMeetings } from '../src/library.ts';
import { ftsQuery, searchMeetings } from '../src/search.ts';

let root = '';

const meta = (id: string, project: string, title: string, tags?: string[]) => ({
  version: '2.0', meeting_id: id, project, title, created_at: '2026-10-27T16:03:09+00:00', status: 'completed', ...(tags ? { tags } : {}),
});
const segs = (texts: string[]) => ({ version: '1.0', segments: texts.map((text, i) => ({
  id: `s${i}`, sequence_id: i, text, audio_start_time: i * 10, audio_end_time: i * 10 + 5,
})) });
const write = (dir: string, files: Record<string, unknown>) => {
  fs.mkdirSync(dir, { recursive: true });
  for (const [name, content] of Object.entries(files)) fs.writeFileSync(path.join(dir, name), JSON.stringify(content));
};

beforeEach(() => {
  root = fs.mkdtempSync(path.join(os.tmpdir(), 'noesis-mcp-search-'));
  const texts = Array.from({ length: 10 }, (_, i) => (i === 9 ? 'the budget is approved' : `filler line ${i}`));
  write(path.join(root, 'dex', '2026-10-27_12-03 wd'), {
    'metadata.json': meta('a', 'dex', 'WD onboarding', ['onboarding', 'western-digital']),
    'transcripts.json': segs(texts),
  });
  write(path.join(root, 'flowers', '2026-10-26_09-00 review'), {
    'metadata.json': { ...meta('b', 'flowers', 'Budget review'), created_at: '2026-10-26T09:00:00+00:00' },
    'transcripts.json': segs(['nothing relevant here']),
  });
  write(path.join(root, 'Meeting legacy'), {
    'metadata.json': { version: '1.0', meeting_name: 'Old one', created_at: '2026-09-30T19:02:05+00:00' },
    'transcripts.json': segs(['legacy budget line']),
  });
});

afterEach(() => fs.rmSync(root, { recursive: true, force: true }));

test('ftsQuery quotes, ANDs and prefixes', () => {
  assert.equal(ftsQuery('  '), null);
  assert.equal(ftsQuery('budget'), '"budget"*');
  assert.equal(ftsQuery('a "b"'), '"a" AND """b"""*');
});

test('title hit ranks first, text hits carry block times, legacy is searched', () => {
  const r = searchMeetings(root, { query: 'budget' });
  assert.equal(r[0].title, 'Budget review');
  const wd = r.find((m) => m.title === 'WD onboarding')!;
  assert.equal(wd.hits[0].start_seconds, 80);
  assert.match(wd.hits[0].snippet, /\*\*budget\*\*/);
  assert.ok(r.some((m) => m.project === '_inbox'));
  assert.ok(r[0].title_match && !r[0].hits[0].text_match);
  assert.ok(!wd.title_match && wd.hits[0].text_match);
});

test('tag hit names the matched tag and is not a text match', () => {
  const [m] = searchMeetings(root, { query: 'western' });
  assert.deepEqual(m.matched_tags, ['western-digital']);
  assert.ok(!m.title_match && !m.hits[0].text_match);
});

test('filters: tags, project, since', () => {
  assert.deepEqual(searchMeetings(root, { query: 'budget', tags: ['onboarding'] }).map((m) => m.title), ['WD onboarding']);
  assert.ok(searchMeetings(root, { query: 'budget', project: 'flowers' }).every((m) => m.project === 'flowers'));
  assert.equal(searchMeetings(root, { query: 'budget', since: '2099-01-01' }).length, 0);
});

test('list_meetings info carries tags', () => {
  const m = scanMeetings(root).find((x) => x.title === 'WD onboarding')!;
  assert.deepEqual(m.tags, ['onboarding', 'western-digital']);
});
