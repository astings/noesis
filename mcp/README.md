# noesis-mcp

MCP server over the Noesis recordings folder. Filesystem only; works with the app closed.

Run: `node src/index.ts` (Node 22.18 or newer; no build step). Root, first match wins: `NOESIS_RECORDINGS`, `MEETILY_RECORDINGS`, `~/Movies/noesis-recordings` if it exists, `~/Movies/meetily-recordings` if it exists, else `~/Movies/noesis-recordings`.

Register for Claude Code (all projects): `claude mcp add --scope user noesis -- node /ABSOLUTE/PATH/ok-meetily/mcp/src/index.ts`

Remove the old entry: `claude mcp remove --scope user meetily`.

Tools: `list_projects`, `list_meetings`, `search_meetings`, `get_transcript`, `assign_meeting`. See the design spec, section 5.4.

`search_meetings` is keyword search (not semantic) over titles, tags and transcript text. It builds an in-memory SQLite FTS5 index per call with `node:sqlite`, which needs Node 22.13 or later (the 22.18 floor above covers it).

Legacy flat folders (recorded before projects existed) can be read and filed through the tools, but the Noesis app does not follow those moves; it keeps their old row.

Test: `npm test`.
