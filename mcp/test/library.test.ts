import { test, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { listProjects, scanMeetings, findMeeting, readTranscript, assignMeeting } from '../src/library.ts';

let root = '';

const v2Meta = (id: string, project: string, title: string, status?: string) => ({
  version: '2.0', meeting_id: id, project, title, meeting_name: 'Meeting 27_10_26_12_03_07',
  created_at: '2026-10-27T16:03:09+00:00', completed_at: '2026-10-27T16:04:09+00:00', duration_seconds: 60.5,
  status: 'completed', audio_file: 'audio.mp4', transcript_file: 'transcripts.json', transcript_md: 'transcript.md',
  transcription: status ? { live: { provider: 'parakeet', model: 'p' }, enhanced: { provider: 'whisper', model: 'large-v3', status } } : { live: { provider: 'parakeet', model: 'p' } },
});
const segments = { version: '1.0', total_segments: 2, segments: [
  { id: 's0', sequence_id: 0, text: 'Hello there.', audio_start_time: 0.0, audio_end_time: 2.0, duration: 2.0 },
  { id: 's1', sequence_id: 1, text: 'Second\nline.', audio_start_time: 65.4, audio_end_time: 70.0, duration: 4.6 },
] };

const write = (dir: string, files: Record<string, string>) => {
  fs.mkdirSync(dir, { recursive: true });
  for (const [name, content] of Object.entries(files)) fs.writeFileSync(path.join(dir, name), content);
};

beforeEach(() => {
  root = fs.mkdtempSync(path.join(os.tmpdir(), 'meetily-mcp-'));
  write(path.join(root, 'dex', '2026-10-27_12-03 wd'), {
    'metadata.json': JSON.stringify(v2Meta('meeting-a', 'dex', 'WD onboarding', 'done')),
    'transcripts.json': JSON.stringify(segments),
    'transcript.md': '---\ntitle: "WD onboarding"\n---\n[00:00] Hello there.\n',
  });
  write(path.join(root, '_inbox', '2026-10-27_13-00'), {
    'metadata.json': JSON.stringify(v2Meta('meeting-b', '_inbox', 'Meeting 27_10_26_13_00_00', 'running')),
    'transcripts.json': JSON.stringify(segments),
  });
  write(path.join(root, 'Meeting 30_09_26_15_02_04_2026-09-30_19-02'), {
    'metadata.json': JSON.stringify({ version: '1.0', meeting_id: null, meeting_name: 'Meeting 30_09_26_15_02_04', created_at: '2026-09-30T19:02:05+00:00', completed_at: '2026-09-30T19:27:54+00:00', duration_seconds: 1564.3, status: 'completed', retranscribed_at: '2026-09-30T19:31:30+00:00', audio_file: 'audio.mp4', transcript_file: 'transcripts.json' }),
    'transcripts.json': JSON.stringify(segments),
  });
  fs.mkdirSync(path.join(root, 'recruiting'));
  fs.mkdirSync(path.join(root, '.hidden'));
});

afterEach(() => fs.rmSync(root, { recursive: true, force: true }));

test('listProjects: inbox first, hidden and legacy folders excluded', () => {
  assert.deepEqual(listProjects(root), ['_inbox', 'dex', 'recruiting']);
});

test('scanMeetings: v2 and legacy, newest first, quality mapped', () => {
  const all = scanMeetings(root);
  assert.deepEqual(all.map((m) => m.meeting_id), ['meeting-b', 'meeting-a', null]);
  const a = all.find((m) => m.meeting_id === 'meeting-a')!;
  assert.equal(a.project, 'dex'); assert.equal(a.title, 'WD onboarding'); assert.equal(a.quality, 'enhanced'); assert.equal(a.legacy, false);
  const b = all.find((m) => m.meeting_id === 'meeting-b')!;
  assert.equal(b.quality, 'enhancing');
  const legacy = all[2];
  assert.equal(legacy.project, '_inbox'); assert.equal(legacy.legacy, true); assert.equal(legacy.quality, 'enhanced', 'retranscribed_at means enhanced');
  assert.equal(legacy.title, 'Meeting 30_09_26_15_02_04');
});

test('findMeeting by id, by absolute path, by folder name', () => {
  assert.equal(findMeeting(root, { meeting_id: 'meeting-a' })?.title, 'WD onboarding');
  assert.equal(findMeeting(root, { path: path.join(root, 'dex', '2026-10-27_12-03 wd') })?.meeting_id, 'meeting-a');
  assert.equal(findMeeting(root, { path: 'dex/2026-10-27_12-03 wd' })?.meeting_id, 'meeting-a');
  assert.equal(findMeeting(root, { path: 'Meeting 30_09_26_15_02_04_2026-09-30_19-02' })?.legacy, true);
  assert.equal(findMeeting(root, { meeting_id: 'nope' }), null);
});

test('readTranscript: md as written for v2, rendered for legacy, text and json', () => {
  const a = findMeeting(root, { meeting_id: 'meeting-a' })!;
  assert.equal(readTranscript(a, 'md'), '---\ntitle: "WD onboarding"\n---\n[00:00] Hello there.\n');
  const legacy = findMeeting(root, { path: 'Meeting 30_09_26_15_02_04_2026-09-30_19-02' })!;
  const md = readTranscript(legacy, 'md');
  assert.match(md, /^---\ntitle: "Meeting 30_09_26_15_02_04"\nproject: _inbox\nmeeting_id: \nstarted_at: 2026-09-30T/);
  assert.match(md, /quality: enhanced\nmodel: unknown\n---\n\[00:00\] Hello there\.\n\[01:05\] Second line\.\n$/);
  assert.equal(readTranscript(legacy, 'text'), 'Hello there.\nSecond line.\n');
  const json = JSON.parse(readTranscript(legacy, 'json', 60, 100));
  assert.equal(json.length, 1); assert.equal(json[0].sequence_id, 1);
  assert.equal(readTranscript(legacy, 'text', 60), 'Second line.\n');
});

test('assignMeeting moves a v2 meeting and rewrites metadata', () => {
  const a = findMeeting(root, { meeting_id: 'meeting-a' })!;
  const newPath = assignMeeting(root, a, { project: 'recruiting', title: 'Paul Kersale' });
  assert.equal(newPath, path.join(root, 'recruiting', '2026-10-27_12-03 paul-kersale'));
  assert.equal(fs.existsSync(a.path), false);
  const meta = JSON.parse(fs.readFileSync(path.join(newPath, 'metadata.json'), 'utf8'));
  assert.equal(meta.project, 'recruiting'); assert.equal(meta.title, 'Paul Kersale'); assert.equal(meta.meeting_id, 'meeting-a');
  assert.equal(meta.transcription.enhanced.status, 'done', 'other keys survive');
});

test('assignMeeting: legacy title edit stays in place, explicit project migrates it', () => {
  const legacy = findMeeting(root, { path: 'Meeting 30_09_26_15_02_04_2026-09-30_19-02' })!;
  assert.equal(assignMeeting(root, legacy, { title: 'DTE kickoff' }), legacy.path);
  assert.equal(JSON.parse(fs.readFileSync(path.join(legacy.path, 'metadata.json'), 'utf8')).title, 'DTE kickoff');
  const moved = assignMeeting(root, findMeeting(root, { path: legacy.path })!, { project: 'dex' });
  assert.match(moved, /\/dex\/2026-09-30_\d{2}-\d{2} dte-kickoff$/);
});

test('assignMeeting rejects bad projects and refuses a running job', () => {
  const a = findMeeting(root, { meeting_id: 'meeting-a' })!;
  assert.throws(() => assignMeeting(root, a, { project: '../x' }), /Invalid project/);
  const b = findMeeting(root, { meeting_id: 'meeting-b' })!;
  assert.throws(() => assignMeeting(root, b, { project: 'dex' }), /being enhanced/);
  assert.throws(() => assignMeeting(root, b, { title: 'Quick sync' }), /being enhanced/);
});

test('assignMeeting refuses a meeting that is still recording', () => {
  const dir = path.join(root, 'dex', '2026-10-27_14-00 live');
  write(dir, { 'metadata.json': JSON.stringify({ ...v2Meta('meeting-c', 'dex', 'Live'), status: 'recording' }) });
  const info = findMeeting(root, { meeting_id: 'meeting-c' })!;
  const before = fs.readFileSync(path.join(dir, 'metadata.json'), 'utf8');
  assert.throws(() => assignMeeting(root, info, { title: 'x' }), /being recorded/);
  assert.equal(fs.readFileSync(path.join(dir, 'metadata.json'), 'utf8'), before);
});

test('scanMeetings skips an unreadable project folder', (t) => {
  if (process.getuid?.() === 0) return t.skip('root can read everything');
  const locked = path.join(root, 'locked');
  fs.mkdirSync(locked);
  fs.chmodSync(locked, 0o000);
  try {
    assert.deepEqual(scanMeetings(root).map((m) => m.meeting_id), ['meeting-b', 'meeting-a', null]);
  } finally {
    fs.chmodSync(locked, 0o755);
  }
});
