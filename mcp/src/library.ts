import fs from 'node:fs';
import path from 'node:path';
import {
  INBOX, isValidProjectName, localStamp, stampOfFolder, meetingFolderName, formatMmss, toLocalIso,
} from './paths.ts';

export type Quality = 'live' | 'enhanced' | 'enhancing' | 'failed';

export type MeetingInfo = {
  meeting_id: string | null;
  project: string;
  title: string;
  started_at: string;
  duration_seconds: number | null;
  quality: Quality;
  path: string;
  legacy: boolean;
};

type Json = Record<string, unknown>;

function readJson(file: string): Json | null {
  try { return JSON.parse(fs.readFileSync(file, 'utf8')) as Json; } catch { return null; }
}

function writeJsonAtomic(file: string, value: unknown): void {
  const tmp = path.join(path.dirname(file), `.metadata.json.${process.pid}.tmp`);
  fs.writeFileSync(tmp, JSON.stringify(value, null, 2));
  fs.renameSync(tmp, file);
}

function isDir(p: string): boolean {
  try { return fs.statSync(p).isDirectory(); } catch { return false; }
}

function hasMetadata(dir: string): boolean {
  return fs.existsSync(path.join(dir, 'metadata.json'));
}

/** Project folders: not hidden, no metadata.json. Inbox first, then sorted. */
export function listProjects(root: string): string[] {
  let names: string[] = [];
  try {
    names = fs.readdirSync(root)
      .filter((n) => !n.startsWith('.') && n !== INBOX && isDir(path.join(root, n)) && !hasMetadata(path.join(root, n)))
      .sort((a, b) => a.localeCompare(b));
  } catch { /* missing root: only the inbox */ }
  return [INBOX, ...names];
}

function qualityOf(meta: Json): Quality {
  const enhanced = (meta.transcription as Json | undefined)?.enhanced as Json | undefined;
  const status = enhanced?.status;
  if (status === 'done') return 'enhanced';
  if (status === 'running' || status === 'queued') return 'enhancing';
  if (status === 'failed') return 'failed';
  return meta.retranscribed_at ? 'enhanced' : 'live';
}

function infoFrom(dir: string, legacy: boolean): MeetingInfo | null {
  const meta = readJson(path.join(dir, 'metadata.json'));
  if (!meta) return null;
  const id = typeof meta.meeting_id === 'string' && meta.meeting_id ? meta.meeting_id : null;
  const title = (typeof meta.title === 'string' && meta.title.trim()) ? meta.title
    : (typeof meta.meeting_name === 'string' && meta.meeting_name.trim()) ? meta.meeting_name
    : path.basename(dir);
  const project = legacy ? INBOX : path.basename(path.dirname(dir));
  return {
    meeting_id: id,
    project,
    title,
    started_at: typeof meta.created_at === 'string' ? meta.created_at : '',
    duration_seconds: typeof meta.duration_seconds === 'number' ? meta.duration_seconds : null,
    quality: qualityOf(meta),
    path: dir,
    legacy,
  };
}

/** Every meeting under the root: <root>/<project>/<meeting>/ plus legacy <root>/<meeting>/. Newest first. */
export function scanMeetings(root: string): MeetingInfo[] {
  const out: MeetingInfo[] = [];
  let entries: string[] = [];
  try { entries = fs.readdirSync(root); } catch { return out; }
  for (const name of entries) {
    if (name.startsWith('.')) continue;
    const dir = path.join(root, name);
    if (!isDir(dir)) continue;
    if (hasMetadata(dir)) {
      const info = infoFrom(dir, true);
      if (info) out.push(info);
      continue;
    }
    let subs: string[] = [];
    try { subs = fs.readdirSync(dir); } catch { continue; }
    for (const sub of subs) {
      const meetingDir = path.join(dir, sub);
      if (sub.startsWith('.') || !isDir(meetingDir) || !hasMetadata(meetingDir)) continue;
      const info = infoFrom(meetingDir, false);
      if (info) out.push(info);
    }
  }
  return out.sort((a, b) => b.started_at.localeCompare(a.started_at));
}

export function findMeeting(root: string, ref: { meeting_id?: string; path?: string }): MeetingInfo | null {
  const all = scanMeetings(root);
  if (ref.meeting_id) return all.find((m) => m.meeting_id === ref.meeting_id) ?? null;
  if (ref.path) {
    const wanted = path.resolve(root, ref.path);
    return all.find((m) => path.resolve(m.path) === wanted) ?? all.find((m) => path.basename(m.path) === ref.path) ?? null;
  }
  return null;
}

type Segment = { start: number; text: string; speaker: string | null; raw: Json };

function readSegments(info: MeetingInfo): Segment[] {
  const data = readJson(path.join(info.path, 'transcripts.json'));
  const list = Array.isArray(data?.segments) ? (data!.segments as Json[]) : [];
  return list
    .map((s) => ({ start: Number(s.audio_start_time ?? 0), text: String(s.text ?? ''), speaker: typeof s.speaker === 'string' ? s.speaker : null, raw: s, seq: Number(s.sequence_id ?? 0) }))
    .sort((a, b) => a.seq - b.seq);
}

function yamlString(s: string): string {
  return `"${s.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

/** spec 5.3, same bytes as the Rust renderer. */
export function renderTranscriptMd(info: MeetingInfo, model: string, segments: Array<{ start: number; text: string; speaker?: string | null }>, speakerNames?: { me: string; them: string }): string {
  let out = '---\n';
  out += `title: ${yamlString(info.title)}\n`;
  out += `project: ${info.project}\n`;
  out += `meeting_id: ${info.meeting_id ?? ''}\n`;
  out += `started_at: ${toLocalIso(info.started_at)}\n`;
  if (info.duration_seconds !== null) out += `duration_seconds: ${Math.round(info.duration_seconds)}\n`;
  out += `quality: ${info.quality}\n`;
  out += `model: ${model}\n`;
  if (speakerNames) out += `speakers: me=${speakerNames.me}, them=${speakerNames.them}\n`; // spec 4.4
  out += '---\n';
  for (const s of segments) {
    const line = s.text.split(/\s+/).filter(Boolean).join(' ');
    if (!line) continue;
    // spec 4.4: "both", null and unknown values stay bare
    const name = !speakerNames ? null : s.speaker === 'me' ? speakerNames.me : s.speaker === 'them' ? speakerNames.them : null;
    out += name ? `[${formatMmss(s.start)}] ${name}: ${line}\n` : `[${formatMmss(s.start)}] ${line}\n`;
  }
  return out;
}

function inRange(start: number, from?: number, to?: number): boolean {
  return (from === undefined || start >= from) && (to === undefined || start <= to);
}

function modelLabel(meta: Json | null): string {
  const t = meta?.transcription as Json | undefined;
  const enhanced = t?.enhanced as Json | undefined;
  const live = t?.live as Json | undefined;
  const pick = enhanced?.status === 'done' ? enhanced : live;
  return pick ? `${pick.provider ?? ''} ${pick.model ?? ''}`.trim() || 'unknown' : 'unknown';
}

export function readTranscript(info: MeetingInfo, format: 'md' | 'text' | 'json', startSeconds?: number, endSeconds?: number): string {
  const ranged = startSeconds !== undefined || endSeconds !== undefined;
  const mdPath = path.join(info.path, 'transcript.md');
  if (format === 'md' && !ranged && fs.existsSync(mdPath)) return fs.readFileSync(mdPath, 'utf8');
  const segments = readSegments(info).filter((s) => inRange(s.start, startSeconds, endSeconds));
  if (format === 'json') return JSON.stringify(segments.map((s) => s.raw), null, 2);
  if (format === 'text') return segments.map((s) => s.text.split(/\s+/).filter(Boolean).join(' ')).filter(Boolean).map((t) => `${t}\n`).join('');
  const meta = readJson(path.join(info.path, 'metadata.json'));
  // spec 4.4: names only when some segment is labelled, like the Rust saver
  const labelled = segments.some((s) => s.speaker === 'me' || s.speaker === 'them' || s.speaker === 'both');
  const sp = meta?.speakers as Json | undefined;
  const names = labelled
    ? { me: typeof sp?.me === 'string' ? sp.me : 'Me', them: typeof sp?.them === 'string' ? sp.them : 'Others' }
    : undefined;
  return renderTranscriptMd(info, modelLabel(meta), segments, names);
}

/**
 * Move and/or retitle a meeting on disk. Mirrors the Rust `library::assign` rules without SQLite:
 * the stamp is kept, the title decides the slug, the project decides the parent, a legacy folder is
 * never moved by a title change, and a running or queued enhance job blocks both moves and title changes (the app would revert the title).
 */
export function assignMeeting(root: string, info: MeetingInfo, changes: { project?: string; title?: string }): string {
  if (changes.project !== undefined && !isValidProjectName(changes.project)) {
    throw new Error(`Invalid project name: ${changes.project}`);
  }
  const metaPath = path.join(info.path, 'metadata.json');
  const meta = readJson(metaPath);
  if (!meta) throw new Error(`metadata.json missing or unreadable at ${info.path}`);
  if (meta.status === 'recording') throw new Error('This meeting is being recorded right now.');
  const newTitle = changes.title ?? info.title;
  const enhanced = (meta.transcription as Json | undefined)?.enhanced as Json | undefined;
  const running = enhanced?.status === 'running' || enhanced?.status === 'queued';
  const oldName = path.basename(info.path);
  const legacy = stampOfFolder(oldName) === null;

  const inPlace = () => {
    meta.title = newTitle;
    if (info.meeting_id) meta.meeting_id = info.meeting_id;
    writeJsonAtomic(metaPath, meta);
    return info.path;
  };

  if (running) throw new Error('This meeting is being enhanced right now. Try again when it finishes.');
  if (legacy && changes.project === undefined) return inPlace();

  const project = changes.project ?? info.project;
  const createdAt = typeof meta.created_at === 'string' ? new Date(meta.created_at) : new Date(NaN);
  const stamp = stampOfFolder(oldName) ?? localStamp(Number.isNaN(createdAt.getTime()) ? new Date() : createdAt);
  const base = meetingFolderName(stamp, newTitle);
  const projectDir = path.join(root, project);
  fs.mkdirSync(projectDir, { recursive: true });
  let target = path.join(projectDir, base);
  for (let n = 2; fs.existsSync(target) && path.resolve(target) !== path.resolve(info.path); n++) {
    target = path.join(projectDir, `${base}-${n}`);
  }
  if (path.resolve(target) !== path.resolve(info.path)) fs.renameSync(info.path, target);
  meta.project = project;
  meta.title = newTitle;
  if (info.meeting_id) meta.meeting_id = info.meeting_id;
  if (!meta.version) meta.version = '2.0';
  writeJsonAtomic(path.join(target, 'metadata.json'), meta);
  return target;
}
