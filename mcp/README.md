# meetily-mcp

MCP server over the Meetily recordings folder. Filesystem only; works with the app closed.

Run: `node src/index.ts` (Node 22.18 or newer; no build step). Root: `MEETILY_RECORDINGS` or `~/Movies/meetily-recordings`.

Register for Claude Code (all projects): `claude mcp add --scope user meetily -- node /ABSOLUTE/PATH/ok-meetily/mcp/src/index.ts`

Tools: `list_projects`, `list_meetings`, `get_transcript`, `assign_meeting`. See the design spec, section 5.4.

Legacy flat folders (recorded before projects existed) can be read and filed through the tools, but the Meetily app does not follow those moves; it keeps their old row.

Test: `npm test`.
