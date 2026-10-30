# Personal meeting recorder: design

Renamed to Noesis on 2026-10-28, see 2026-10-28-noesis-and-meeting-list-design.md.

Date: 2026-10-27. Status: draft for review. Fork of Meetily v0.4.1.

## 1. Goal

Turn Meetily into a dumb, accurate recorder that drops well-organized transcripts in per-project folders, and let Claude in the OpenKnowledge wikis consume them without symlinks or manual steps.

The owner's flow today:

1. Zoom call starts. Owner clicks record in Meetily. Live transcript runs on Parakeet (fast, rough).
2. Call ends. Owner clicks stop, then opens the meeting and clicks "Enhance" with Whisper large-v3. Waits.
3. Owner renames the folder under `~/Movies/noesis-recordings` with a case or candidate slug.
4. In a wiki repo (DEX, Flowers, recruiting), each of which symlinks the recordings folder, the owner asks Claude to process the recording. The wiki skill reads `transcripts.json`, reflows it, and writes the wiki documents.

Pain points, with evidence from the wikis:

- Enhance is a manual click on every meeting.
- Recording keeps running after the call. The Flowers transcript of 2026-10-26 carries 23 stray segments of television audio captured 70 minutes after the call ended.
- Folders are flat and untagged until renamed by hand. Two of 33 folders are tagged today.
- Every wiki needs a symlink and a README paragraph on how to repair it.
- Transcripts have no speaker labels. Both the DEX and recruiting skills say "infer who is speaking from context and flag uncertainty".

Non-goals:

- Live transcript quality. Live stays fast and rough.
- In-app summaries, notes, LLM providers, licensing, analytics. Left as they are, not removed (see section 9). Superseded for the UI by [the lean UI reskin](2026-10-27-lean-ui-reskin-design.md).
- Teams or Google Meet detection. Zoom is 95% of calls.
- Multi-speaker diarization beyond "me" and "them". Named as a later upgrade in section 4.5.

## 2. What exists today

All Rust paths are under `frontend/src-tauri/src/`, UI paths under `frontend/src/`.

- `audio/` is the live audio system. `audio_v2/` and `lib_old_complex.rs` are dead code.
- Recording writes `~/Movies/noesis-recordings/<title>_<UTC stamp>/` with `audio.mp4` (mono AAC 48 kHz), `transcripts.json` (rewritten on every segment) and `metadata.json`. Path logic: `audio/recording_saver.rs:224-229`, `audio/audio_processing.rs:35-58`. The `save_folder` preference is stored but never read.
- Mic and system audio are separate until `audio/pipeline.rs:849-851`, then mixed. Only the mix is recorded and transcribed. The `transcripts.speaker` column exists (migration `20251110000001`) and is never written.
- The SQLite meeting row is created by the frontend after stop (`hooks/useRecordingStop.ts:236-260` to `api_save_transcript`). That is why `metadata.json` has `meeting_id: null`.
- "Enhance" is `audio/retranscription.rs`: decode `audio.mp4`, VAD over the whole file, transcribe segments sequentially with the picked model, `DELETE` then `INSERT` the transcript rows, rewrite `transcripts.json`, stamp `retranscribed_at`. Manual only, one job at a time via a global flag. Events: `retranscription-progress`, `retranscription-complete`, `retranscription-error`.
- Live engine selection is `transcript_settings` (provider, model). Default Parakeet `parakeet-tdt-0.6b-v3-int8`. Whisper `ggml-large-v3.bin` is already downloaded on this machine.
- No call detection of any kind. No CLI, HTTP or MCP surface. `sysinfo` is already a dependency.
- Tray can start, pause and stop a recording (`tray.rs`). Stop goes through `recording_commands::stop_recording` then emits `recording-stop-complete`, which the frontend uses to save.
- Notifications module has `show_transcription_complete`.
- Export is clipboard only. `save_transcript(file_path, content)` exists in `lib.rs:288` with no caller.
- 43 Rust files carry `#[cfg(test)]`. The frontend has no tests.

## 3. Target flow

```
Zoom meeting starts (CptHost appears)      optional auto-start, default on
        |
Owner clicks record (or auto-start)  ->  project picker, defaults to last used, "_inbox" if unsure
        |
Live: Parakeet segments + me/them label from mic vs system energy
        |
Zoom meeting ends (CptHost gone 20 s)  ->  auto-stop
        |
Stop: write transcript.md (quality: live), save SQLite row, then enqueue enhance
        |
Enhance: Whisper large-v3 on audio.mp4, segments split at speaker turns, labels applied
        |
Write transcript.md (quality: enhanced), update metadata.json, notify, run post-enhance hook if set
        |
Any wiki: Claude calls the noesis MCP tools  ->  list_meetings(project), get_transcript, assign_meeting
```

## 4. Design, one capability at a time

Each item states the change, the files it touches, and the lazier alternative that was considered.

### 4.1 Auto-enhance after stop

What: when a recording stops and its SQLite row is saved, start the existing retranscription with the configured enhance model. No click.

How:

- New preferences in `recording_preferences.json`: `auto_enhance: bool` (default true), `enhance_provider: "whisper"`, `enhance_model: "large-v3"`, `enhance_language: Option<String>` (default auto).
- Trigger point: `hooks/useRecordingStop.ts`, right after `storageService.saveMeeting` resolves and the meeting id is known. Invoke `start_retranscription_command` with the enhance settings. The tray stop path reaches the same hook through `RecordingPostProcessingProvider`. In phase 1 the id is the one returned by the save; phase 2 moves id generation to record start (4.2) without changing this trigger.
- Queue instead of refuse: replace the "already running" early return in `audio/retranscription.rs:20-46` with a `tokio::sync::Mutex<()>` so back-to-back meetings wait their turn. Keep `cancel`.
- Status: write `transcription.enhanced.status` to `metadata.json` (`queued`, `running`, `done`, `failed`) at each transition. Reuse the existing progress events. Sidebar shows a small badge per meeting from that status.
- Completion: call `show_transcription_complete` (exists). Then write `transcript.md` (section 5.3) and run the hook (section 4.7).
- Model lifecycle: unchanged. The job unloads Whisper when done; the next live start loads Parakeet.

Lazier alternative: a frontend-only "auto-click" of the Enhance dialog. Rejected because the queue and status have to live in Rust to survive window reloads.

### 4.2 Projects as folders

What: every meeting lives under a project folder. A project is a wiki: `dex`, `flowers`, `recruiting`. Unassigned meetings go to `_inbox`.

Layout:

```
~/Movies/noesis-recordings/
  _inbox/
    2026-10-27_09-05/                 title not set yet
  dex/
    2026-10-26_14-30 western-digital-onboarding/
      audio.mp4  transcripts.json  transcript.md  metadata.json  speakers.json
  recruiting/
    2026-10-23_10-16 paul-kersale/
  Meeting 23_10_26_10_16_51_2026-10-23_14-16/     legacy flat folder, read as _inbox, never moved
```

Rules:

- The project list is the set of first-level directories under the root that do not start with a dot. No database table. Creating a project is `mkdir`. The app offers "+ New project" which does exactly that.
- Folder name is `<local YYYY-MM-DD_HH-MM>` plus, when a title is set, a space and the slug of the title. Local time, date first, so Finder sorts chronologically. Replaces `audio/audio_processing.rs:35-58`.
- Setting or changing the title renames the folder. Changing the project moves the folder. Both update `meetings.folder_path` and `metadata.json`.
- The filesystem is the source of truth. The database is an index. On startup and on sidebar refresh, a reconcile scan walks `<root>/*/*/metadata.json` plus legacy `<root>/*/metadata.json`, and for every `meeting_id` found it updates `folder_path`, `title` and `project` in SQLite when they differ. This makes Finder renames, MCP moves and manual `mv` all safe. New module `audio/library.rs`, about 80 lines.
- `meeting_id` must be known at record start so it lands in `metadata.json`. Rust generates `meeting-<uuid>` in `start_recording_with_meeting_name`, writes it to metadata, returns it in `recording-started`, and the frontend passes it to `api_save_transcript` (new optional argument; the repository uses it instead of minting one).
- UI: a project dropdown next to the record button (remembers last choice, default `_inbox`), and a "Move to project" action plus title edit in meeting details. Both call one new command, `assign_meeting(meeting_id, project: Option<String>, title: Option<String>)`.
- Legacy flat folders stay where they are and are listed as project `_inbox`. No migration, so the existing wiki scripts keep working during the transition.

Lazier alternative: keep the flat folder and rely on title prefixes (what the owner does by hand). Rejected: project folders make `list_meetings(project)` trivial and give a clean mental model in Finder.

### 4.3 The transcript contract

What: every meeting folder carries a `transcript.md` that any agent can read without knowing Meetily. See section 5.3 for the exact format. It is written at stop (quality `live`) and rewritten after enhance (quality `enhanced`). `transcripts.json` stays for backward compatibility with the current wiki skills.

How: one pure function `render_transcript_md(meta, segments) -> String` in `audio/transcript_md.rs`, called from `recording_saver.rs` at stop and from `retranscription.rs` at completion. Unit tested on a fixture.

### 4.4 Speaker labels: me and them

What: each segment gets `speaker: "me" | "them" | "both"`. "Me" is the microphone, "them" is system audio (the Zoom participants). This resolves the recruiting flow (interviewer vs candidate) and the DEX and Flowers one-to-one calls, which are most of the meetings.

How:

- Live: at `pipeline.rs:849-851`, where `mic_window` and `sys_window` are still separate, compute the RMS of each 50 ms window and append `(t, mic_rms, sys_rms)` to a timeline. The VAD segment that covers a time span is labelled by the dominant source over that span: `me` if mic energy is above 70% of the total, `them` if below 30%, otherwise `both`. The label travels on `TranscriptUpdate` and `TranscriptSegment` and fills the existing `transcripts.speaker` column.
- Persist the timeline as `speakers.json` next to the audio (one entry per 50 ms window, two small integers each, about 150 KB per hour). Written by `recording_saver.rs` at stop.
- Enhance: after VAD and before transcription, split every speech segment at speaker turns read from `speakers.json`. A turn is a change in dominant source that holds for at least one second. Then transcribe each piece and label it. This is the main quality win: Whisper segments stop straddling two speakers.
- Names: `speakers.json` is anonymous. `metadata.json` carries `speakers: {me: "<owner name>", them: "Others"}`. The owner's name is a preference. Rendering uses the names.

Lazier alternative kept in mind: record stereo (left mic, right system) and derive energy from the file at enhance time. It is the better primitive if finer diarization is ever wanted, since the recording would keep the information. Rejected for now because it changes the audio format, playback and the FFmpeg concat path. The timeline sidecar touches three files and leaves `audio.mp4` as it is.

Deferred: splitting "them" into named speakers (pyannote segmentation plus embeddings via `sherpa-rs`, run on the system share only). Not planned until the me/them labels prove insufficient.

### 4.5 Zoom call detection: auto-stop, optional auto-start

Generalised by [generic settings for distribution](2026-10-28-generic-settings-for-distribution-design.md), section 2: any process reading the microphone counts as a call on macOS 14; `CptHost` is the macOS 13 fallback; auto-start defaults off.

What: stop recording when the Zoom meeting ends. Start when one begins.

How:

- Signal: Zoom on macOS runs the helper process `CptHost` only while a meeting is in progress. New module `call_detector.rs` polls `sysinfo` every 5 seconds for a process named `CptHost` and keeps a two-state machine: `in_call` and `idle`. macOS only (`#[cfg(target_os = "macos")]`).
- Auto-stop: when `in_call` becomes `idle` and stays so for 20 seconds while recording, and `auto_stop_on_call_end` is true (default true), call the tray stop path (`recording_commands::stop_recording`, then emit `recording-stop-complete`) so the frontend saves exactly as it does for a tray stop. `metadata.json` records `ended_by: "auto-stop"`.
- Auto-start: when `idle` becomes `in_call` and `auto_start_on_call` is true (default true) and nothing is recording, trigger the existing tray start path, which records into the project last chosen on the home screen with the default devices.
- Both flags live in `recording_preferences.json` and get two switches in Recording settings.
- The state machine is a pure function over a sequence of samples, unit tested. The `CptHost` assumption is verified as step zero of the implementation plan: `pgrep -x CptHost` must return nothing while Zoom is open but not in a meeting.

Fallback if `CptHost` proves unreliable: system audio silence for more than N minutes while recording. Not built unless needed.

### 4.6 MCP server

Distribution: published as `@noesis-app/noesis-mcp`, registered with `npx -y` or the Settings button (generic settings spec, section 5). The root also follows the app's `save_folder` preference (section 4).

What: a small Model Context Protocol server that exposes the recordings root so every wiki can list and read meetings without a symlink. Registered once at user scope, so it is present in every Claude Code project.

How:

- Standalone stdio server, TypeScript, one file, in a new top-level `mcp/` directory using `@modelcontextprotocol/sdk`. It reads the filesystem only. It does not need the app to be running. Root comes from `NOESIS_RECORDINGS`, then `MEETILY_RECORDINGS`, else `~/Movies/noesis-recordings` (or the old `~/Movies/meetily-recordings` if only that exists).
- Tools (section 5.4 has the schemas): `list_projects`, `list_meetings`, `get_transcript`, `assign_meeting`.
- `get_transcript` returns `transcript.md` when present, and renders the same format from `transcripts.json` for legacy folders, so the 33 existing meetings are readable on day one.
- `assign_meeting` moves or renames the folder and rewrites `metadata.json`. The app picks the change up through the reconcile scan (4.2).
- Registration: `claude mcp add --scope user noesis -- node <repo>/mcp/src/index.ts`. Codex and Cursor get the equivalent entry in their user-level config. Wiki `.mcp.json` files do not change.
- Wiki side, one change per wiki: the skills replace "glob the symlink" with "call `list_meetings` and `get_transcript`", and `scripts/match_recordings.py` is either kept pointing at the root path or replaced by `list_meetings(since)`. The symlinks can be deleted once the skills are updated.

Why not inside the Tauri app: the app may be closed, and a stdio server has a per-client lifecycle. Why not Rust: more code, slower iteration, and nothing here needs Rust. If shipping inside the `.app` bundle ever matters, port the four tools to a Rust binary in the same workspace.

Lazier alternative: no server, just `transcript.md` plus the root path written in each wiki's `CLAUDE.md`. Rejected because it still hardcodes a path in every wiki and gives Claude no `assign` or `list` without shell work.

### 4.7 Post-enhance hook

> Removed 2026-10-29: the hook was dropped as too niche. This section is history; see CLAUDE.md.

What: let the app kick off a wiki workflow when a transcript is ready.

How: one preference `post_enhance_command: String` (empty means off). When an enhance job finishes, run it through `sh -c` detached, with `NOESIS_FOLDER`, `NOESIS_PROJECT`, `NOESIS_TITLE` and `NOESIS_MEETING_ID` in the environment (the `MEETILY_*` equivalents are still set), output appended to `hook.log` in the meeting folder. About 15 lines in `retranscription.rs`. The log is the only record of the hook; `metadata.json` does not track it.

The wiki automation itself is a user script outside the app. The reference script maps project to wiki directory and runs `claude -p "Process the new recording at $NOESIS_FOLDER"` in that directory. Recommendation: the wiki skills should only draft (transcript doc plus a draft log entry) when invoked this way, and leave scoring, tracker and Blueprint writes for a reviewed session. That is a wiki skill decision, not an app decision.

Lazier alternative: a `launchd` folder watcher outside the app. Rejected: the in-app hook fires exactly once with the right arguments and is fewer moving parts.

### 4.8 Wiki automation settings

> Removed 2026-10-29: the hook was dropped as too niche. This section is history; see CLAUDE.md.

Superseded for the settings shape by [generic settings for distribution](2026-10-28-generic-settings-for-distribution-design.md), section 1: the card is "Post-processing", the prompt is a template, the tool lists are editable and `skill` became per-project instructions.

What: the post-enhance hook without a script. A "Wiki automation" card in Recording settings holds a switch, a table of projects, and a cost cap; the app composes and runs the Claude Code call itself.

How: preferences gain `wiki_automation: bool` (default off), `wiki_targets: [{project, directory, skill}]` and `wiki_budget_usd: f64` (default 5). When an enhance job finishes with segments and the switch is on, the app looks up the meeting's project in `wiki_targets`. On a match it runs, from the wiki directory, `claude -p <prompt> --permission-mode acceptEdits --allowedTools mcp__open-knowledge mcp__noesis --disallowedTools mcp__open-knowledge__delete mcp__open-knowledge__move mcp__noesis__assign_meeting --max-budget-usd <cap>`, with the same detached spawn and `hook.log` as 4.7. The prompt names the skill when one is set ("Use the <skill> skill to process the new meeting recording at <folder> ...") and asks for a draft only. A successful run writes `hook.done` in the meeting folder; a later enhance skips the meeting while the marker exists (delete it to reprocess). The app finds `claude` in `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, then `PATH`, and the settings card shows where it was found or that it is missing. Projects without a row fall through to the custom command of 4.7 when one is set, so both can coexist.

Why in the app rather than the script: the daily UX is a table, not a shell file in a dev folder, and a repackaged build can ship the behaviour without asking recipients to install a script. The command field of 4.7 stays as the advanced escape hatch.

## 5. Contracts

### 5.1 Folder layout

See 4.2. Root, then project, then meeting. Legacy flat meeting folders at the root are read as `_inbox`.

### 5.2 metadata.json version 2

```json
{
  "version": "2.0",
  "meeting_id": "meeting-7a1f...",
  "project": "dex",
  "title": "Western Digital onboarding",
  "created_at": "2026-10-26T18:30:05Z",
  "completed_at": "2026-10-26T19:12:40Z",
  "duration_seconds": 2555.2,
  "ended_by": "auto-stop",
  "devices": {"microphone": "Alex AirPods", "system_audio": "Alex AirPods"},
  "sample_rate": 48000,
  "status": "completed",
  "audio_file": "audio.mp4",
  "transcript_file": "transcripts.json",
  "transcript_md": "transcript.md",
  "speakers_file": "speakers.json",
  "speakers": {"me": "Alexandre", "them": "Others"},
  "tags": ["onboarding", "western-digital"],
  "tagged_at": "2026-10-26T19:32:10Z",
  "audio_deleted_at": "2026-10-27T19:40:00Z",
  "transcription": {
    "live": {"provider": "parakeet", "model": "parakeet-tdt-0.6b-v3-int8"},
    "enhanced": {"provider": "whisper", "model": "large-v3", "status": "done", "at": "2026-10-26T19:31:30Z", "error": null}
  },
  "retranscribed_at": "2026-10-26T19:31:30Z"
}
```

`tags` (lowercase-kebab slugs), `tagged_at` and `audio_deleted_at` are optional and set after enhance; `audio_deleted_at` means `audio.mp4` was removed and `audio_file` is `""`. `retranscribed_at`, `created_at`, `completed_at` and `status` keep their v1 meaning so the current wiki scripts keep working. `project` and `title` in metadata win over the database when they disagree; the reconcile scan updates the index and leaves folders where they are.

### 5.3 transcript.md

```
---
title: "Western Digital onboarding"
project: dex
meeting_id: meeting-7a1f...
started_at: 2026-10-26T14:30:05-04:00
duration_seconds: 2555
quality: enhanced
model: whisper large-v3
speakers: me=Alexandre, them=Others
tags: [onboarding, western-digital]
---
[00:00] Alexandre: Thank you. Hello, how are you?
[00:04] Others: Yes, it's been a long time.
[00:09] Alexandre: Did you recently get on this rotation?
```

One segment per line, `[mm:ss]` start time, speaker name when known, text verbatim. Segments labelled `both`, and legacy segments with no label, print without a name. Nothing is paraphrased or dropped. The frontmatter keys are stable; consumers must ignore unknown keys. The title is a quoted YAML string so colons and quotes in a title stay valid.

### 5.4 MCP tools

```
list_projects() -> [{name, meeting_count, last_meeting_at}]

list_meetings({project?: string, tag?: string, since?: ISO date, until?: ISO date, limit?: number = 20})
  -> [{meeting_id, project, title, started_at, duration_seconds, quality: "recording"|"live"|"enhanced"|"enhancing"|"failed", path, tags: string[]}]
  Sorted newest first. `_inbox` includes legacy root-level folders.
  quality "enhancing" covers both queued and running jobs. "recording" means the meeting is being recorded right now (see 2026-10-29-live-meeting-ask-design.md).

search_meetings({query: string, project?: string, tags?: string[], since?: ISO date, until?: ISO date, limit?: number = 10})
  -> [{meeting_id, project, title, started_at, tags, path, score,
       hits: [{start_seconds, end_seconds, snippet}]}]
  Keyword search (not semantic) over titles, tags and transcript text; best meeting first. `tags` filters to meetings carrying all the given tags. Read context with get_transcript(start_seconds, end_seconds).

get_transcript({meeting_id?: string, path?: string, format?: "md"|"text"|"json" = "md", start_seconds?: number, end_seconds?: number})
  -> string. "md" is transcript.md (rendered from transcripts.json for legacy folders), "text" strips timestamps, "json" is the raw segments.

assign_meeting({meeting_id?: string, path?: string, project?: string, title?: string}) -> {path}
  Moves and/or renames the folder, rewrites metadata.json. Creates the project folder if missing.
  `path` (absolute folder or folder name) identifies a legacy meeting that has no id; a legacy meeting is moved only when a project is given. The app does not follow a move or rename of a legacy folder made through the tools; its sidebar row keeps the old title and path.
```

Errors are plain messages: unknown meeting, unknown project, transcript still enhancing.

## 6. Phasing

Each phase is one implementation plan and one branch. Estimates are Claude Code sessions, assuming the owner tests on a real Zoom call at the end of each phase.

| Phase | Scope | Sections | Estimate |
| --- | --- | --- | --- |
| 1 | Auto-enhance with queue, status badge, notification | 4.1 | 1 session, about 2 hours |
| 2 | Projects as folders, meeting_id at start, reconcile scan, transcript.md, picker and move UI | 4.2, 4.3, 5.1 to 5.3 | 2 sessions |
| 3 | MCP server, user-scope registration, update the three wiki skills, remove symlinks | 4.6, 5.4 | 1 to 2 sessions |
| 4 | Zoom auto-stop and optional auto-start | 4.5 | 1 session |
| 5 | Me/them speaker labels live and at enhance, segment splitting at turns | 4.4 | 2 sessions |
| 6 | Post-enhance hook plus a reference wiki script | 4.7 | half a session |
| 7 | Wiki automation settings: project table, Claude Code detection, app-composed run | 4.8 | 1 session |

Phase 1 first because it removes a daily click with the smallest change. Phase 3 before 4 and 5 because it removes the symlinks, which is the second daily annoyance. Phases 4 to 6 are independent of each other.

## 7. Testing

- Rust: unit tests for folder naming and slugging, the reconcile scan on a temp directory, `render_transcript_md`, speaker labelling from a synthetic timeline, segment splitting at turns, and the call detector state machine over a sample sequence. Run with `cargo test` in `frontend/src-tauri`.
- MCP: one test file against a fixture root containing a v2 folder and a legacy folder, run with `node --test`.
- Manual acceptance per phase, on a two-minute Zoom test call: folder lands in the chosen project, recording stops within 30 seconds of leaving the call, enhance runs without a click and `transcript.md` flips to `quality: enhanced`, `list_meetings` from a wiki shows the meeting, labels read as me and them.

## 8. Decisions to confirm

Defaults chosen in this document. Changing any of them changes the plans.

1. Recordings root stays `~/Movies/noesis-recordings`, with one subfolder per project. Legacy flat folders are left in place and read as `_inbox`.
2. Speaker labels come from a live mic-vs-system energy timeline saved as `speakers.json`. `audio.mp4` stays mono. Stereo recording is the named upgrade path, not the default.
3. Zoom detection uses the `CptHost` helper process. Auto-stop and auto-start both default on.
4. The MCP server is a standalone Node script registered at user scope, not part of the Tauri app.
5. Summary, LLM provider, licensing and analytics code stays. Changes are additive to keep upstream merges feasible.

## 9. Upstream policy

This fork tracks `Zackriya-Solutions/meetily` loosely. New behaviour goes in new modules (`audio/library.rs`, `audio/transcript_md.rs`, `call_detector.rs`, `mcp/`) with small, clearly marked edits in existing files. Nothing is deleted for taste. Dead code (`audio_v2/`, `lib_old_complex.rs`, `audio/stt.rs`, `parallel_processor.rs`) is left alone until it blocks a change.
