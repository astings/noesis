#!/usr/bin/env node
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import { recordingsRoot, parseBound } from './paths.ts';
import { listProjects, scanMeetings, findMeeting, readTranscript, assignMeeting } from './library.ts';
import { searchMeetings } from './search.ts';

const root = recordingsRoot();
const server = new McpServer({ name: 'noesis', version: '0.1.0' });

const text = (value: unknown) => ({ content: [{ type: 'text' as const, text: typeof value === 'string' ? value : JSON.stringify(value, null, 2) }] });
const fail = (message: string) => ({ isError: true as const, content: [{ type: 'text' as const, text: message }] });

server.registerTool(
  'list_projects',
  { description: `Project folders under the Noesis recordings root (${root}). "_inbox" holds untagged and legacy meetings.`, inputSchema: {} },
  async () => {
    const all = scanMeetings(root);
    return text(listProjects(root).map((name) => {
      const mine = all.filter((m) => m.project === name);
      return { name, meeting_count: mine.length, last_meeting_at: mine[0]?.started_at ?? null };
    }));
  },
);

server.registerTool(
  'list_meetings',
  {
    description: 'Meetings recorded by Noesis, newest first. Filter by project folder and/or ISO date range. quality is live, enhanced, enhancing or failed.',
    inputSchema: {
      project: z.string().optional().describe('Project folder name, e.g. "dex". "_inbox" for untagged and legacy meetings.'),
      since: z.string().optional().describe('ISO 8601: YYYY-MM-DD (whole local day) or a datetime; only meetings started at or after it.'),
      until: z.string().optional().describe('ISO 8601: YYYY-MM-DD (whole local day) or a datetime; only meetings started at or before it.'),
      tag: z.string().optional().describe('Only meetings whose tags include this tag, e.g. "onboarding".'),
      limit: z.number().int().positive().max(200).default(20).describe('Maximum number of meetings, default 20'),
    },
  },
  async ({ project, tag, since, until, limit }) => {
    const sinceT = since ? parseBound(since, false) : undefined;
    const untilT = until ? parseBound(until, true) : undefined;
    if ((since && Number.isNaN(sinceT)) || (until && Number.isNaN(untilT))) return fail('since/until must be ISO 8601: YYYY-MM-DD (whole local day) or a datetime');
    const rows = scanMeetings(root).filter((m) => {
      if (project && m.project !== project) return false;
      if (tag && !m.tags.includes(tag)) return false;
      const t = Date.parse(m.started_at);
      if (sinceT !== undefined && !(t >= sinceT)) return false;
      if (untilT !== undefined && !(t <= untilT)) return false;
      return true;
    }).slice(0, limit);
    return text(rows);
  },
);

server.registerTool(
  'search_meetings',
  {
    description: 'Keyword search (not semantic) over meeting titles, tags and transcript text under the Noesis recordings root. Ranked best first; each hit has start_seconds and end_seconds. Try synonyms and the other language if nothing matches, then call get_transcript with start_seconds/end_seconds around a hit to read the context.',
    inputSchema: {
      query: z.string().describe('Words to find; all must appear, the last one matches as a prefix.'),
      project: z.string().optional().describe('Project folder name, e.g. "dex". "_inbox" for untagged and legacy meetings.'),
      tags: z.array(z.string()).optional().describe('Only meetings that carry all of these tags.'),
      since: z.string().optional().describe('ISO 8601: YYYY-MM-DD (whole local day) or a datetime; only meetings started at or after it.'),
      until: z.string().optional().describe('ISO 8601: YYYY-MM-DD (whole local day) or a datetime; only meetings started at or before it.'),
      limit: z.number().int().positive().max(50).default(10).describe('Maximum number of meetings, default 10'),
    },
  },
  async (args) => {
    if ((args.since && Number.isNaN(parseBound(args.since, false))) || (args.until && Number.isNaN(parseBound(args.until, true)))) {
      return fail('since/until must be ISO 8601: YYYY-MM-DD (whole local day) or a datetime');
    }
    return text(searchMeetings(root, args));
  },
);

server.registerTool(
  'get_transcript',
  {
    description: 'Transcript of one meeting. "md" is transcript.md (rendered from transcripts.json for legacy folders), "text" strips timestamps, "json" is the raw segments. Optional time range in seconds. While a meeting is being enhanced, only format "json" (live segments) is available.',
    inputSchema: {
      meeting_id: z.string().optional(),
      path: z.string().optional().describe('Absolute folder path or folder name, when the meeting has no id (legacy).'),
      format: z.enum(['md', 'text', 'json']).default('md').describe('"md" is transcript.md (default), "text" strips timestamps, "json" is the raw segments'),
      start_seconds: z.number().nonnegative().optional(),
      end_seconds: z.number().nonnegative().optional(),
    },
  },
  async ({ meeting_id, path: folder, format, start_seconds, end_seconds }) => {
    if (!meeting_id && !folder) return fail('Give meeting_id or path');
    const info = findMeeting(root, { meeting_id, path: folder });
    if (!info) return fail(`No meeting found for ${meeting_id ?? folder}`);
    if (info.quality === 'enhancing' && format !== 'json') {
      return fail('Transcript is still being enhanced; try again in a few minutes, or request format "json" now for the live segments.');
    }
    return text(readTranscript(info, format, start_seconds, end_seconds));
  },
);

server.registerTool(
  'assign_meeting',
  {
    description: 'Move a meeting to a project folder and/or set its title. Renames the folder; the Noesis app picks the change up on its next refresh. A legacy flat folder is only moved when a project is given. Legacy flat folders have no id: after a move or rename the Noesis sidebar keeps their old title and path; only these tools see the meeting under its project.',
    inputSchema: {
      meeting_id: z.string().optional(),
      path: z.string().optional().describe('For legacy meetings without an id: absolute folder path or folder name.'),
      project: z.string().optional(),
      title: z.string().optional(),
    },
  },
  async ({ meeting_id, path: folder, project, title }) => {
    if (!meeting_id && !folder) return fail('Give meeting_id or path');
    if (project === undefined && title === undefined) return fail('Give project and/or title');
    const info = findMeeting(root, { meeting_id, path: folder });
    if (!info) return fail(`No meeting found for ${meeting_id ?? folder}`);
    try {
      return text({ path: assignMeeting(root, info, { project, title }) });
    } catch (error) {
      return fail(error instanceof Error ? error.message : String(error));
    }
  },
);

await server.connect(new StdioServerTransport());
