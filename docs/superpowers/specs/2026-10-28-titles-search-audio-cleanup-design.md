# Auto titles, tags, search and audio cleanup: design

Phase 9 of the personal meeting recorder (see `2026-10-27-personal-meeting-recorder-design.md`). Three capabilities, all post-enhance, all local-first:

1. After enhance, an LLM pass gives an untitled meeting a real title and gives every meeting a handful of tags.
2. A keyword search over title, tags and transcript text, ranked by relevance, with timestamped hits, in the app sidebar and as an MCP tool, so an agent can find where something was discussed.
3. Audio is deleted 24 hours after a successful enhance, to reclaim disk (today 1.4 GB across 52 meetings, almost all of it audio).

Each one follows the hook pattern of section 8 of `2026-10-28-noesis-and-meeting-list-design.md`: an automatic run behind a setting, plus a manual trigger on the meeting page and in the sidebar right-click menu, plus a status tag.

## 1. Auto title and tags

### When

At the end of a successful enhance with `segments_count > 0`, when `auto_title_tags` is on (default on). It runs before the post-enhance hook dispatch, so the hook and wiki automation see the final title and folder.

The enhance job holds `RetranscriptionGuard` until `start_retranscription` returns, and `library::assign` will not rename a folder while that guard is held. The tagging step therefore runs after the guard is dropped: the guard is scoped to the transcription work, and the post-processing (tagging, then hook dispatch) runs after it.

### Engine

The summary provider configured in the app (built-in model, Ollama, Claude, Groq, OpenRouter, OpenAI or custom OpenAI-compatible). It is local when that provider is built-in or Ollama. The config loading now inline in `summary/service.rs:354-430` (provider, model, API key, Ollama endpoint, custom OpenAI config) is extracted into one helper, `summary::llm_config::load(pool) -> Result<LlmConfig, String>`, which both the summary service and the new step call. The call itself is the existing `llm_client::generate_summary` with our own system and user prompt.

### Prompt and parsing

- Input: the transcript text from the database `transcripts` rows of the meeting (present for every meeting, legacy included), speaker names included, without timestamps. When it exceeds 24 000 characters, keep the first 16 000 and the last 8 000 with a `[...]` marker, which fits the built-in model's context.
- System prompt asks for JSON only: `{"title": "<3 to 8 words, no date, no quotes>", "tags": ["<1 to 3 words>", ...]}`, 3 to 6 tags naming topics, companies, people's roles or decisions, in the transcript's language.
- Parsing is lenient: take the first `{` to the last `}` of the reply, parse with serde_json. A reply that does not parse, an empty title, or zero tags after normalization is a failure.
- Tag normalization: lowercase, trim, spaces and underscores to `-`, strip characters other than letters, digits and `-`, drop empties, dedupe, keep at most 6.
- Title: trimmed, surrounding quotes stripped, at most 80 characters.

### Writes

- `metadata.json` gains `tags: [string]` and `tagged_at: ISO8601`. New keys, the v2 contract otherwise unchanged.
- `transcript.md` frontmatter gains `tags: [a, b, c]` (YAML flow list) after `speakers`. `render_transcript_md` reads tags from metadata, so every later rewrite keeps them. After tagging, `transcript.md` is re-rendered.
- Title: in the automatic run, applied only when the current title is empty or `library::is_auto_title()` is true. A title the owner typed, in the UI or through MCP `assign_meeting`, is never overwritten. In the manual run, the owner asked explicitly, so the title is always applied. Both go through `library::assign(pool, root, id, None, Some(title))`, which renames the folder and updates the index.
- The search index is refreshed for the meeting (section 2).

### Manual trigger and status

- Command `run_meeting_tagging(meeting_id) -> Result<String, String>`: refuses a live meeting and a meeting with no transcript rows; otherwise runs the same step with the title always applied, and returns "Title and tags updated".
- Meeting page: a "Title & tags" button next to Enhance and Run hook (lucide `Tags`). Sidebar right-click menu: "Regenerate title & tags".
- Settings, Recordings: switch "Name and tag meetings after enhance" (help: "Uses your summary model. Off: run it from the meeting page or the right-click menu."), plus a button "Tag meetings without tags" that runs the step, sequentially, over every meeting with transcript rows and no `tags` in metadata, and reports the count. The backfill applies titles under the automatic rule (only untitled meetings).
- Status: in memory, `running` and `failed` per meeting id, `done` when metadata has `tags`. Command `list_tagging_statuses()`, event `tagging-status-changed {meeting_id, status}`, mirroring `hook_status.rs`. `MeetingStatusTags` gains a third icon (lucide `Tags`): "Tagged", "Tagging", "Tagging failed". Toast on failure: "Title and tags failed" with the error.
- Tags display: small chips under the title in the meeting page header, read-only.

### Errors

Every failure is logged and leaves the meeting as it was. Enhance stays `done`. No summary model configured: "No summary model configured. Set one in Settings, Summary."

## 2. Search

### Index unit

A block of 8 consecutive transcript segments, with the start time of its first segment and the end time of its last. Hits point to a moment in the meeting, not only to the meeting. Each block row also carries the meeting's title and tags, so a meeting-level match ranks with its best block.

### Query rules (shared by the app and MCP)

- The user's text is split on whitespace. Each term is wrapped in double quotes (inner quotes doubled) so FTS5 syntax characters are literal. Terms are joined with AND. The last term gets a `*` prefix match, for search-as-you-type.
- Ranking: `bm25(search_index, 0, 0, 0, 5.0, 3.0, 1.0)` over (meeting_id, start, end, title, tags, text), so title weighs most, tags next, text least. Lower is better, as FTS5 returns it.
- Results grouped per meeting: the meeting's score is its best block; up to 3 hits per meeting, each with `snippet()` of the text column (about 24 tokens, `…` ellipsis, matched terms wrapped in `**`).
- Tokenizer: `unicode61 remove_diacritics 2`, so "reunion" finds "réunion".

### App

- Step zero of the plan: confirm the SQLite bundled by sqlx is built with FTS5 (`CREATE VIRTUAL TABLE ... USING fts5` in a test). If it is not, the app side ranks the same blocks in Rust with a plain term-frequency score and the rest of this section stands.
- Migration: `CREATE VIRTUAL TABLE search_index USING fts5(meeting_id UNINDEXED, start_seconds UNINDEXED, end_seconds UNINDEXED, title, tags, text, tokenize='unicode61 remove_diacritics 2')`.
- New module `search_index.rs` (under `database/` or `audio/`, next to `library.rs`): `reindex_meeting(pool, meeting_id)` deletes the meeting's rows and inserts its blocks from the `transcripts` rows plus the title from `meetings` and tags from `metadata.json`; `rebuild(pool)` does all meetings in one transaction; `search(pool, query, limit)`.
- Refresh points: full rebuild at startup after reconcile, in the background; `reindex_meeting` after a recording is saved, after enhance completes, after tagging, after `assign` changes a title, and when a meeting is deleted (delete its rows). Full rebuild stays fast for thousands of meetings; past that, rebuild only meetings whose `updated_at` moved.
- `api_search_transcripts(query)` keeps its name and its current fields (`id`, `title`, `match_context`, `timestamp`) so the sidebar keeps working, and gains `tags` and `start_seconds`. It returns one result per meeting, best first. `match_context` becomes the best snippet.
- Sidebar: unchanged behaviour (filter the list to matching meetings, show the context line), now ranked and matching titles and tags. Clicking a hit opens the meeting as today. Jumping to the timestamp is out of scope.

### MCP

- New tool `search_meetings`:

```
search_meetings({query: string, project?: string, tags?: string[], since?: ISO date, until?: ISO date, limit?: number = 10})
  -> [{meeting_id, project, title, started_at, tags, path, score,
       hits: [{start_seconds, end_seconds, snippet}]}]
  Best meeting first. `tags` filters to meetings carrying all the given tags.
```

- Description tells the agent: keyword search, not semantic; try synonyms and the other language; then call `get_transcript` with `start_seconds`/`end_seconds` around a hit to read context.
- Implementation: per call, build an in-memory `node:sqlite` FTS5 table from the filesystem with the same schema and query rules: segments from `transcripts.json` (the same parser `get_transcript` uses for `format: "json"`), title and tags from `metadata.json`. The server still reads only the filesystem and needs no new dependency. `node:sqlite` needs Node 22.13 or later; the README states it. Per-call rebuild stays fast for a few thousand meetings; past that, cache per folder by `transcripts.json` mtime.
- `list_meetings` gains `tags` in each result and an optional `tag` filter. Section 5.4 of the main spec is extended accordingly.
- `assign_meeting` is unchanged.

## 3. Audio cleanup

### Rule

A meeting's audio is deleted when all of these hold:

1. `delete_audio_after_enhance` is on (default on).
2. `metadata.json` has `transcription.enhanced.status == "done"` and `transcription.enhanced.at` older than `audio_retention_hours` (default 24).
3. `transcript.md` exists in the folder, its frontmatter says `quality: enhanced`, and it has at least one segment line.
4. The folder is not the live recording folder and no enhance job is queued or running for the meeting.
5. `metadata.json` has no `audio_deleted_at` yet.

Meetings never enhanced, legacy flat folders without an enhanced status, and any folder that fails a check are left alone.

Note for the owner: the first run deletes the audio of every meeting enhanced more than 24 hours ago. That is the intended saving.

### What is deleted

The audio file `find_audio_file` would pick (the same candidate list retranscription uses), and a leftover `.checkpoints/` directory. `transcripts.json`, `transcript.md`, `metadata.json`, `speakers.json`, `hook.log` and `hook.done` stay. Then `metadata.json` gets `audio_deleted_at: ISO8601` and `audio_file: ""`, and the freed bytes are logged.

### Loop

New module `audio/audio_cleanup.rs`. A pure `should_delete(meta, transcript_md, now, retention_hours) -> bool` holds the rule (unit tested). `run_once(pool, root, now)` walks the meetings table folders. A background task, spawned in `lib.rs` setup after the database is ready (the `call_detector` pattern), runs it at startup and then every hour, reloading preferences each tick.

### Manual trigger

- Command `delete_meeting_audio(meeting_id) -> Result<String, String>`: same checks except the retention window (rule 2 becomes status `done` only). Errors: "This meeting is being recorded right now.", "Enhance first: the audio is the only full copy of this meeting.", "Audio already removed." Returns "Audio deleted (<size> freed)".
- Meeting page: "Delete audio" button (lucide `Trash2`, destructive style) next to Enhance, behind a confirmation dialog ("Delete the audio of this meeting? The transcript stays. Enhance and playback will no longer be possible."). Sidebar right-click menu: "Delete audio", same dialog.
- Settings, Recordings: switch "Delete audio after enhance" and a number input "Keep audio for (hours)", minimum 1.

### Effect on the rest of the app

- Meeting page: when `audio_deleted_at` is set, the Enhance and Delete audio buttons are disabled with the tooltip "Audio removed after enhance". A command `meeting_audio_state(meeting_id) -> {has_audio, deleted_at}` feeds this.
- Retranscription on a folder without audio but with `audio_deleted_at` fails with "Audio was removed after enhance" instead of "No audio file found".
- MCP: `get_transcript` and `search_meetings` are unaffected. `list_meetings` is unchanged.

## 4. Preferences

Added to `recording_preferences.rs` with serde defaults, and to the frontend `RecordingPreferences` type:

| Key | Type | Default |
| --- | --- | --- |
| `auto_title_tags` | bool | true |
| `delete_audio_after_enhance` | bool | true |
| `audio_retention_hours` | u32 | 24 |

## 5. Contract changes

All additive. Consumers already must ignore unknown keys.

- `metadata.json` v2 gains `tags`, `tagged_at`, `audio_deleted_at`. `audio_file` becomes `""` after deletion, as it already is when `auto_save` is off.
- `transcript.md` frontmatter gains `tags: [..]`.
- MCP gains `search_meetings`; `list_meetings` gains the `tags` field and the `tag` filter.
- Main spec sections 5.2 to 5.4 are updated in the same branch.

## 6. Testing

- Rust unit tests: tag normalization; title cleaning; JSON extraction from a chatty reply; transcript truncation; block building from segments; FTS query escaping (quotes, `*`, `-`, `NEAR`); FTS5 availability, insert and ranking on an in-memory pool (title hit ranks above text hit); `should_delete` over each rule; `run_once` on a temp root deletes only the eligible folder's audio and `.checkpoints/` and writes `audio_deleted_at`; `render_transcript_md` with tags.
- MCP: `node --test` against the fixture root: `search_meetings` finds a term in a v2 folder and in a legacy folder, title match ranks first, `tags` filter, `start_seconds` of a hit; `list_meetings` returns tags.
- Manual acceptance on a two-minute Zoom call: after enhance the folder is renamed to a real title and the meeting page shows tags; sidebar search finds a word said in the call and a tag; `search_meetings` from a wiki returns the hit with a timestamp; with retention set to 1 hour, the audio disappears an hour after enhance and the Enhance button reads "Audio removed after enhance"; each manual button works.

## 7. Out of scope

- Editing tags by hand.
- Jumping to the hit's timestamp in the meeting page.
- Semantic search with embeddings. Named upgrade path: chunk embeddings from a local Ollama model (`nomic-embed-text`) in a per-meeting sidecar, queried by both the app and MCP. Built only if keyword search misses meetings the owner actually looks for.
- Compressing audio instead of deleting it.

## 8. Files

- New: `summary/llm_config.rs`, `audio/meeting_tagging.rs` (prompt, parsing, normalization, the step, the command, status), `audio/search_index.rs`, `audio/audio_cleanup.rs`, one migration, `mcp/src/search.ts`, `mcp/test/search.test.ts`.
- Edited: `summary/service.rs` (use the helper), `audio/retranscription.rs` (guard scope, call tagging, clearer missing-audio error), `audio/transcript_md.rs` (tags), `audio/library.rs` (reindex after assign), `audio/recording_preferences.rs`, `lib.rs` (commands, startup rebuild, cleanup loop), `database/repositories/transcript.rs` (`search_transcripts` uses the index), `mcp/src/index.ts`, `mcp/src/library.ts` (tags), `mcp/README.md`, `frontend/src/components/RecordingSettings.tsx`, `MeetingDetails/TranscriptButtonGroup.tsx`, `Sidebar/index.tsx`, `Sidebar/SidebarProvider.tsx`, `MeetingStatusTags.tsx`, the meeting page header.

## 9. Estimate

About 1.5 to 2 Claude Code sessions, plus the owner's Zoom smoke test. Branch `phase-9/search-and-cleanup`.
