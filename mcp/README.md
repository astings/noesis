# noesis-mcp

MCP server over the Noesis recordings folder. Filesystem only; works with the app closed.

Root, first match wins: `NOESIS_RECORDINGS`, `MEETILY_RECORDINGS`, the app's "Recordings folder" setting (`~/Library/Application Support/com.noesis.app/recording_preferences.json`), `~/Movies/noesis-recordings` if it exists, `~/Movies/meetily-recordings` if it exists, else `~/Movies/noesis-recordings`.

Run without a checkout: `npx -y @noesis-app/noesis-mcp` (Node 22.18 or newer).

Register for Claude Code (all projects): `claude mcp add --scope user noesis -- npx -y @noesis-app/noesis-mcp`, or click "Register with Claude Code" (row "MCP server") in Noesis, Settings, Recordings, Claude Code.

From a checkout instead: `node src/index.ts`, registered with `claude mcp add --scope user noesis -- node /ABSOLUTE/PATH/noesis/mcp/src/index.ts`.

The published package is compiled to `dist/` by `npm run build` (`tsc`); the checkout runs the TypeScript sources directly.

If you used Meetily, remove the old entry: `claude mcp remove --scope user meetily`.

Tools: `list_projects`, `list_meetings`, `search_meetings`, `get_transcript`, `assign_meeting`. See the design spec, section 5.4.

`search_meetings` is keyword search (not semantic) over titles, tags and transcript text. It builds an in-memory SQLite FTS5 index per call with `node:sqlite`, which needs Node 22.13 or later (the 22.18 floor above covers it).

Legacy flat folders (recorded before projects existed) can be read and filed through the tools, but the Noesis app does not follow those moves; it keeps their old row.

Test: `npm test`.
