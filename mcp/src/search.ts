import { DatabaseSync } from 'node:sqlite';
import { scanMeetings, readSegments, type MeetingInfo } from './library.ts';
import { parseBound } from './paths.ts';

export type SearchOpts = { query: string; project?: string; tags?: string[]; since?: string; until?: string; limit?: number };
export type SearchResult = Pick<MeetingInfo, 'meeting_id' | 'project' | 'title' | 'started_at' | 'tags' | 'path'>
  & { score: number; title_match: boolean; matched_tags: string[]; hits: { start_seconds: number; end_seconds: number; snippet: string; text_match: boolean }[] };

export function ftsQuery(input: string): string | null {
  const terms = input.split(/\s+/).filter(Boolean).map((t) => `"${t.replaceAll('"', '""')}"`);
  return terms.length ? `${terms.join(' AND ')}*` : null;
}

export function buildBlocks(segs: { start: number; end: number; text: string }[], size = 8) {
  const out: { start: number; end: number; text: string }[] = [];
  for (let i = 0; i < segs.length; i += size) {
    const c = segs.slice(i, i + size);
    out.push({ start: c[0].start, end: c[c.length - 1].end, text: c.map((s) => s.text.trim()).join('\n') });
  }
  return out;
}

// ponytail: index rebuilt in memory per call; cache per folder by transcripts.json mtime past a few thousand meetings
export function searchMeetings(root: string, o: SearchOpts): SearchResult[] {
  const q = ftsQuery(o.query);
  if (!q) return [];
  const sinceT = o.since ? parseBound(o.since, false) : undefined;
  const untilT = o.until ? parseBound(o.until, true) : undefined;
  const meetings = scanMeetings(root).filter((m) => {
    const t = Date.parse(m.started_at);
    return (!o.project || m.project === o.project)
      && (sinceT === undefined || t >= sinceT)
      && (untilT === undefined || t <= untilT)
      && (o.tags ?? []).every((tag) => m.tags.includes(tag));
  });
  const db = new DatabaseSync(':memory:');
  db.exec("CREATE VIRTUAL TABLE s USING fts5(idx UNINDEXED, start_seconds UNINDEXED, end_seconds UNINDEXED, title, tags, text, tokenize='unicode61 remove_diacritics 2')");
  const ins = db.prepare('INSERT INTO s VALUES (?, ?, ?, ?, ?, ?)');
  meetings.forEach((m, i) => {
    const segs = readSegments(m).map((s) => ({
      start: s.start,
      end: typeof s.raw.audio_end_time === 'number' ? s.raw.audio_end_time : s.start,
      text: s.text,
    }));
    const blocks = buildBlocks(segs);
    for (const b of blocks.length ? blocks : [{ start: 0, end: 0, text: '' }]) ins.run(i, b.start, b.end, m.title, m.tags.join(' '), b.text);
  });
  let rows: { idx: number; start_seconds: number; end_seconds: number; snip: string; score: number; title_snip: string; tags_snip: string }[] = [];
  try {
    rows = db.prepare("SELECT idx, start_seconds, end_seconds, snippet(s, 5, '**', '**', '…', 24) AS snip, bm25(s, 0, 0, 0, 5.0, 3.0, 1.0) AS score, snippet(s, 3, '**', '**', '', 64) AS title_snip, snippet(s, 4, '**', '**', '', 64) AS tags_snip FROM s WHERE s MATCH ? ORDER BY score LIMIT 500").all(q) as typeof rows;
  } catch { return []; } finally { db.close(); }
  const out = new Map<number, SearchResult>();
  for (const r of rows) {
    const hit = { start_seconds: r.start_seconds, end_seconds: r.end_seconds, snippet: r.snip, text_match: r.snip.includes('**') };
    const cur = out.get(r.idx);
    if (cur) { if (cur.hits.length < 3) cur.hits.push(hit); continue; }
    const { meeting_id, project, title, started_at, tags, path } = meetings[r.idx];
    // same on every block of a meeting, so the best row says whether title and tags matched (as audio/search_index.rs)
    const matched_tags = r.tags_snip.split(/\s+/).filter((t) => t.includes('**')).map((t) => t.replaceAll('**', ''));
    out.set(r.idx, { meeting_id, project, title, started_at, tags, path, score: r.score, title_match: r.title_snip.includes('**'), matched_tags, hits: [hit] });
  }
  return [...out.values()].slice(0, o.limit ?? 10);
}
