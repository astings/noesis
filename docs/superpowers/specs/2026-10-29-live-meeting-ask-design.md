# Live meeting ask: design

Date: 2026-10-29. Status: approved in brainstorming.

## 1. Goal

During a call, ask a local model what has been discussed so far and get an answer in seconds, inside Noesis. The same box works on a finished meeting. Separately, Claude Code can read the transcript of a meeting that is still being recorded through the `noesis` MCP server.

Typical questions, from the owner: "what was said about X", "what did I miss in the last few minutes", "recap of the whole meeting so far".

Model: the summary model already configured for titles and tags, today the built-in `llama-helper` sidecar running Qwen 3.5 4B (Q4_K_M, 32k context). Open source, already on disk, no new install. The longest transcript recorded so far is about 12k tokens for a 53-minute meeting, so a whole meeting fits with room. Expected latency on the owner's M4 Max: 2 to 3 seconds for a short meeting, 5 to 10 seconds for an hour-long one. The sidecar protocol returns whole answers, so there is no streaming.

Non-goals: chat memory across questions, saving answers to disk, a global hotkey over Zoom, a second LLM provider, streaming. Each is named below with the path to add it if wanted.

## 2. What exists today

Rust paths under `frontend/src-tauri/src/`, UI paths under `frontend/src/`, MCP under `mcp/src/`.

- Live segments are emitted as `transcript-update` by `audio/transcription/worker.rs` and accumulated in the recording manager; `recording_commands::get_transcript_history` returns them as `TranscriptSegment { text, audio_start_time, audio_end_time, speaker, .. }`. The frontend mirrors them in `contexts/TranscriptContext.tsx`.
- The recorder writes `metadata.json` at record start with `status: "recording"`, `meeting_id`, `project` and `title`, and rewrites `transcripts.json` atomically after every segment (`audio/recording_saver.rs`).
- A finished meeting's segments are in SQLite `transcripts` (`transcript`, `audio_start_time`, `speaker`), read by `audio/meeting_tagging.rs::run_inner` with `SELECT ... ORDER BY audio_start_time, timestamp`.
- The model call path is `summary::llm_config::load(pool)` then `summary::llm_client::generate_summary(...)`, which dispatches to Ollama, Claude, OpenAI, Groq, OpenRouter, a custom OpenAI endpoint or the built-in sidecar. The configured provider is `builtin-ai`, model `qwen3.5:4b`. The sidecar keeps the model resident with an idle timeout and serialises requests. `summary::processor::rough_token_count` exists.
- The MCP server lists any folder with a `metadata.json`, so a recording meeting already appears in `list_meetings`, and `readTranscript` already renders from `transcripts.json` when `transcript.md` is absent. `qualityOf` returns `live` for it, which hides that it is in progress. `assignMeeting` refuses `status === 'recording'`.
- UI primitives: `components/ui/{input,button,textarea}.tsx`, shadcn tokens, `sonner` toasts. No Cmd shortcuts are bound anywhere. `react-markdown` was removed in the lean UI reskin.

## 3. Design

### 3.1 Rust command `ask_meeting`

New module `audio/ask.rs`, registered in `lib.rs`. About 80 lines plus tests.

```
#[tauri::command]
async fn ask_meeting(app, question: String, meeting_id: Option<String>, last_minutes: Option<u32>)
  -> Result<AskAnswer, String>

struct AskAnswer { answer: String, from_seconds: f64, to_seconds: f64, segments: usize }
```

Steps:

1. **Source.** `meeting_id` absent: the live recording's segments from the recording manager. Present: SQLite rows for that meeting, the tagging query. The live meeting has no SQLite row until stop, so the two cases do not overlap; a `meeting_id` that happens to be live is not special-cased.
2. **Window.** With `last_minutes = n`, keep segments whose `audio_start_time >= newest_end - 60 n`, where `newest_end` is the largest `audio_end_time`. Without it, keep all.
3. **Render.** One line per segment, `[mm:ss] Name: text`. Name is the `owner_name` preference for `me`, `Others` for `them`, none for `both` or unlabelled, the `transcript.md` rules.
4. **Budget.** If `rough_token_count` of the rendered text exceeds 24 000, drop the oldest lines until it fits. 24k leaves room for the 4 096-token answer and the prompt inside the 32k context. `from_seconds` is the start of the first line kept, so the UI can show what was covered.
5. **Call.** `llm_config::load(pool)`, then `generate_summary` with the system prompt below and the user prompt `<lines>\n\nQuestion: <question>`. Wrapped in `tokio::time::timeout(90 s)`; on timeout the cancellation token is cancelled so the sidecar drops the request, and the error reads "The model took too long". A static busy flag refuses a second concurrent ask with "An answer is already being generated". Empty source: "No transcript yet."

System prompt (fixed, not a preference):

> You answer questions about a meeting using only the transcript below. Quote or closely paraphrase the transcript; do not invent details. If the transcript does not cover the question, say so in one sentence. Speaker names are approximate. Answer in the language of the question. Be brief: a few sentences or at most five bullet points.

Not a job in the job queue, nothing written to disk, no new preference. The preset questions are sent by the UI as ordinary text: "Recap what has been discussed so far." and "What was said in the last 10 minutes?" (the latter with `last_minutes: 10`).

Known ceiling: `llama-helper` has no prompt cache, so each ask re-reads the whole prompt. Upgrade path: keep the KV cache across requests in `llama-helper/src/main.rs` so a growing transcript only pays for the new lines.

### 3.2 Ask panel

New `components/AskPanel.tsx`, props `meetingId?: string` and `disabled?: boolean`. About 120 lines. Mounted twice:

- Home, `app/page.tsx`: between `TranscriptPanel` and the recording controls, rendered while recording or while live segments exist.
- Meeting details, `app/meeting-details/page-content.tsx`: under the split view, with `meetingId={meeting.id}`.

States:

- **Idle.** One row: an `Input` with placeholder "Ask about this meeting… ⌘K" and two chip buttons, "Recap so far" and "Last 10 min". Chips are disabled until at least one segment exists (home: `transcripts.length`, details: `totalCount`). A `keydown` listener on `window` focuses the input on Cmd+K.
- **Asking.** Enter or a chip calls `ask_meeting`. Input and chips are disabled; the row shows a spinner and "Thinking…". No cancel button; the 90-second timeout is the cancel.
- **Answer.** A card above the row: a muted header with the coverage ("Last 10 min, 14:02 to 14:12" or "Whole meeting, 48 segments", built from `from_seconds`, `to_seconds` and `segments`), the answer as plain text with `whitespace-pre-wrap`, a Copy button, a close button. Only the latest answer is kept. Esc clears the card and blurs the input. The question stays in the input.
- **Error.** The Rust message shown inside the card, not a toast, so it stays readable mid-call. Two messages get a hint: the no-model error adds "pick one in Settings", the timeout adds "try Last 10 min".

Styling uses existing tokens (`bg-card`, `border-border`, `text-muted-foreground`), so dark mode needs nothing. No Markdown renderer; the prompt asks for short sentences or bullets, which read fine as text.

Alternatives not built: a list of past answers in the card (add a `history` array in component state), a chat with follow-ups (send the previous pair in the user prompt), streaming (needs a token-stream protocol in `llama-helper`, the Rust client and an event per chunk).

### 3.3 MCP: live meetings are visible

`mcp/src/library.ts`:

- `Quality` gains `'recording'`. `qualityOf` returns it first when `meta.status === 'recording'`. The `transcript.md` frontmatter rendered for a live meeting therefore reads `quality: recording`.
- No new tool or parameter. `assignMeeting` keeps refusing recording meetings.

`mcp/src/index.ts` descriptions:

- `list_meetings`: "quality is recording, live, enhanced, enhancing or failed. recording means the meeting is in progress right now; it is the newest, so it sorts first."
- `get_transcript`: "On a meeting being recorded, returns the transcript so far; call again for new lines."

Release: `mcp/package.json` to 0.3.0, `npm publish --otp <code>` by the owner. Registrations use `npx -y @noesis-app/noesis-mcp`, so new Claude Code sessions pick the version up without re-registering. `mcp/README.md` mentions the live case in one sentence.

## 4. Contracts

Main spec section 5.4, `list_meetings` and `transcript.md` frontmatter: `quality` gains the value `recording`, placed first in the list. Folder layout, `metadata.json` v2 and the other tool schemas do not change. Consumers that switch on `quality` must treat unknown values as `live`.

## 5. Testing

- Rust, `cargo test` in `frontend/src-tauri`: window selection over a synthetic segment list, budget trim keeps the newest lines and reports `from_seconds`, line rendering with `me`, `them`, `both` and unlabelled segments.
- MCP, `node --test` in `mcp/`: a fixture folder with `status: "recording"` and a two-segment `transcripts.json`; `listMeetings` reports quality `recording`; `readTranscript(md)` contains both lines and `quality: recording`; `assignMeeting` still refuses it (existing test).
- Manual, on a one-minute test recording: "Recap so far" answers within 10 seconds; "Last 10 min" shows the coverage header; Cmd+K focuses the input; a finished meeting answers a topic question from its details page; from a Claude Code session during the recording, `list_meetings` shows the top meeting as `recording` and a second `get_transcript` a minute later returns more lines.

## 6. Decisions

1. The model is whatever the summary model setting says; no separate "ask" model setting.
2. Single-shot questions, no memory, nothing persisted.
3. Whole answer at once, 90-second timeout, one ask at a time.
4. Presets are plain questions sent by the UI; "Last 10 min" is 10 minutes of audio time.
5. The MCP change is the quality value and two descriptions; reading live transcripts needed no new code.
