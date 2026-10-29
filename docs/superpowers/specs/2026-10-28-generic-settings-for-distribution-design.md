# Generic settings for distribution

Date: 2026-10-28. Status: approved in brainstorming (option C).

## Problem

Noesis works for one person on one Mac. Several behaviours are tied to that person's flow (Claude Code plus OpenKnowledge wikis, Zoom, English and French, three named wikis) or to that Mac (fixed recordings root, silent Meetily migration, a repo checkout for the MCP server). Before the app is packaged for other people, those have to become settings with safe defaults, or derive from the machine they run on.

Audience (option C): the recorder is the product. Anyone on a Mac who wants accurate local transcripts in per-project folders. The agent hook stays as an advanced card that most people never open; Claude Code remains its only built-in agent.

Out of scope: Windows and Linux (every fork feature is already `cfg(target_os = "macos")`), a Developer ID and notarization (next step, separate), other built-in agents, in-app summaries.

## What is specific today

| Area | Where | Tied to |
|---|---|---|
| Wiki automation | `audio/wiki_automation.rs`: `ALLOWED_TOOLS`, `DENIED_TOOLS`, `build_prompt`, the `skill` field, `locate_claude` | Claude Code, OpenKnowledge, DEX vocabulary ("trackers", "Blueprint", "draft log entry") |
| Call detection | `call_detector.rs` (`CptHost`), Settings group "Zoom", `calendar_lookup.rs` tie-break on `zoom.us` | Zoom |
| Defaults | `recording_preferences.rs`: languages `en, fr`, delete audio after 24 h on, auto-start on | the owner |
| Recordings root | 14 runtime calls to `get_default_recordings_folder()`; the `save_folder` preference only drives "Open folder" | `~/Movies/noesis-recordings` |
| Migration | `migration::migrate_dirs()` at `lib.rs:501` renames any Meetily data dir it finds | a Mac that used to run Meetily |
| MCP server | `mcp/README.md`: absolute path into a checkout, Node 22.18 | this checkout |
| Example script | `scripts/post-enhance.sh` maps `dex`, `recruiting`, `flo` to `$HOME/Code/...` | the owner's wikis |

## Design

### 1. Post-processing card (replaces "Wiki automation")

The card in Settings, Recordings is renamed "Post-processing" and reads: "After each enhance, run Claude Code in the project's folder with the prompt below." Same spawn, same `hook.log`, same `hook.done`, same one-run-per-meeting rule, same job row.

Preferences (`recording_preferences.json`). Keys are renamed with `#[serde(alias = "...")]` on the old name, so an existing file loads unchanged and the next save writes the new names. The module `audio/wiki_automation.rs` becomes `audio/post_processing.rs` (`git mv`), and `WikiTarget` becomes `PostProcessingTarget`.

| New key | Old key | Default |
|---|---|---|
| `post_processing: bool` | `wiki_automation` | false |
| `post_processing_targets: Vec<PostProcessingTarget>` | `wiki_targets` | empty |
| `post_processing_budget_usd: f64` | `wiki_budget_usd` | 5 |
| `agent_path: String` | new | empty, meaning auto-detect through `locate_claude()` |
| `agent_prompt_template: String` | new | empty, meaning the built-in template |
| `agent_allowed_tools: Vec<String>` | new | `["mcp__open-knowledge", "mcp__noesis"]` |
| `agent_denied_tools: Vec<String>` | new | `["mcp__open-knowledge__delete", "mcp__open-knowledge__move", "mcp__noesis__assign_meeting"]` |

`PostProcessingTarget { project, directory, instructions }`. The legacy `skill` field is still deserialized (`#[serde(default, skip_serializing)]`); `load_recording_preferences` turns a non-empty `skill` with empty `instructions` into `instructions = "Use the <skill> skill."` and clears `skill`. The first save persists that.

Built-in prompt template, with `{folder}`, `{project}`, `{title}`, `{meeting_id}` and `{instructions}` substituted and runs of spaces collapsed:

> Process the meeting recording at {folder} (project {project}, meeting id {meeting_id}, title: {title}). {instructions} Draft only: leave everything uncommitted for review. This run is unattended: do not ask questions; if this meeting is already covered, stop without changes.

Unknown placeholders are left as typed. A template that renders to an empty string falls back to the built-in one.

Command line: `<agent_path or located claude> -p <prompt> --permission-mode acceptEdits [--allowedTools <list>] [--disallowedTools <list>] --max-budget-usd <cap>`. An empty allowed list omits `--allowedTools`, which in `-p` mode means Claude gets file edits and nothing else. An empty denied list omits `--disallowedTools`.

UI (`components/PostProcessingSettings.tsx`, renamed from `WikiAutomationSettings.tsx`):

- Switch "Run Claude Code after enhance" with the detected or configured path underneath, as today.
- "Claude Code path": text field, placeholder shows the auto-detected path; empty means auto.
- "Projects": one row per project with `ProjectSelect`, directory plus Browse, and an "Instructions" text field (placeholder: "Optional. Added to the prompt, e.g. Use the dex-case-tracking skill.").
- "Prompt template": textarea, same pattern as "Prompt for titles and tags", with a "Reset to default" link and the placeholder list in the hint.
- "Allowed tools" and "Denied tools": one text field each, space or comma separated.
- "Cost cap per run": unchanged.
- "Register with Claude Code" (row "MCP server"): see section 5.

The custom shell command of spec 4.7 ("Custom command", Advanced group) stays as the escape hatch for any other agent. `dispatch_with` in `post_enhance_hook.rs` keeps its logic; only the names change.

### 2. Calls, not Zoom

Signal: another process is reading the microphone. On macOS 14 and later, CoreAudio exposes process objects: `kAudioHardwarePropertyProcessObjectList` on the system object, then `kAudioProcessPropertyIsRunningInput` and `kAudioProcessPropertyPID` per object. `in_call` is true when any object with a PID other than Noesis's own reports running input. This covers Zoom, Teams, Google Meet in a browser, Webex and FaceTime with no per-app table. `coreaudio-sys` (already a transitive dependency through cpal) becomes a direct one; the four-character constants are defined locally if the crate's bindings predate macOS 14.

The `pgrep -x CptHost` check stays a second signal on every macOS version: a call is on when either signal says so, and on macOS 13 (or after a CoreAudio error) it is the only one. The detector logs once which signals it is using.

The state machine in `call_detector::step` is unchanged: 5 second poll, 20 second end debounce. `zoom_meeting_active()` stays as the fallback; the new function is `mic_in_use_by_others() -> Option<bool>`.

Step zero of the plan verifies the signal the way `CptHost` was verified: with a Teams or Meet call live, the process list shows that app's PID with running input; with no call and Noesis idle, no foreign PID does; while Noesis records alone, its own PID is the only one and is excluded.

Known false positives, accepted: dictation, Voice Memos, a browser tab holding the microphone. Auto-start defaults off (section 3), so the common case is auto-stop, where a false positive only delays the stop.

Settings group "Zoom" becomes "Calls": "Start recording when a call begins", "Stop recording when the call ends", "Name the recording after the calendar event". The hint says a call is any app using the microphone.

Calendar: `Candidate.zoom` becomes `has_link`, true when the location, notes or URL contain `zoom.us`, `teams.microsoft.com`, `meet.google.com`, `webex.com` or `facetime.apple.com`. The tie-break ("a link wins, then the latest start") is otherwise unchanged.

`metadata.json` `ended_by: "auto-stop"` is unchanged.

### 3. Defaults derived or made safe

Only `Default` and the `#[serde(default = ...)]` functions change. Stored values on existing installs are untouched, because the store saves the whole struct.

| Preference | Today | New default |
|---|---|---|
| `transcription_languages` | `["en", "fr"]` | ISO 639-1 codes from `NSLocale.preferredLanguages`, deduplicated, kept only if Whisper knows them; `["en"]` when that yields nothing. Non-macOS: `["en"]`. |
| `owner_name` | `"Me"` | `NSFullUserName()` when non-empty, else `"Me"`. The UI label "Me" for the speaker is unchanged; this is the name written to `metadata.json` and `transcript.md`. |
| `auto_start_on_call` | true | false. Recording consent rules differ by country; a stranger opts in. |
| `delete_audio_after_enhance` | true | false. Deleting the source audio is destructive; a stranger opts in. |

`objc2-foundation` gains the `NSLocale` and `NSPathUtilities` features (the second holds `NSFullUserName`).

### 4. The recordings root becomes real

New `pub fn recordings_root<R: Runtime>(app: &AppHandle<R>) -> PathBuf` in `recording_preferences.rs`: reads `save_folder` from the `recording_preferences.json` store synchronously and returns it when non-empty, else `get_default_recordings_folder()`. It replaces the 14 runtime call sites: `lib.rs` (startup sweep and reconcile), `audio/library.rs` (`list_projects`, root for the reconcile command), `audio/retranscription.rs`, `audio/recording_saver.rs` (where new folders land), `audio/meeting_tagging.rs`, `api/api.rs` (five sites), `summary/service.rs`, and the "Open folder" command. `migration.rs` keeps the default: it describes where the data lived before any preference existed.

Changing the folder in Settings moves nothing. New recordings land under the new root, `list_projects` reads the new root, and the reconcile scan walks it. Meetings recorded under the old root stay in SQLite with their absolute `folder_path` and still open. The hint under the setting says: "Existing recordings stay where they are."

MCP server (`mcp/src/paths.ts`): root order becomes `NOESIS_RECORDINGS`, `MEETILY_RECORDINGS`, then `save_folder` read from `~/Library/Application Support/com.noesis.app/recording_preferences.json` when that file parses and the value is non-empty, then today's default and legacy folders. Other platforms keep the environment variable route.

### 5. Distribution hygiene

Migration asks first. Before `migrate_dirs()` at `lib.rs:501`, when the plan holds at least one rename and no `migration.skipped` marker exists in the Noesis app data dir, a blocking `rfd::MessageDialog` (the crate behind `tauri-plugin-dialog`, added as a direct dependency) shows: "Meetily data was found on this Mac. Move it to Noesis now? Nothing is deleted. If you skip, Noesis starts fresh and leaves the Meetily data where it is; you can move it by hand later." Buttons: "Move" and "Skip". Move runs the migration as today. Skip (or closing the dialog) writes the marker and leaves the data in place for good, because the app creates its own dirs at that launch and a later automatic move is no longer possible. A user without Meetily data never sees the dialog. The main window is created by setup after the dialog returns (`"create": false` in `tauri.conf.json`), so no webview call can create the new app data dir while the user reads the dialog.

MCP without a checkout. `mcp/package.json` is published as `@alexandrelaine/noesis-mcp` (`noesis-mcp` is taken on npm), public, with `"bin": { "noesis-mcp": "dist/index.js" }`: `tsc` compiles `src/` to `dist/` at `npm pack` because Node refuses to strip types from files under `node_modules`; the shebang survives compilation; the checkout keeps running `src/index.ts` directly; Node 22.18 or newer stays the floor. Registration is `claude mcp add --scope user noesis -- npx -y @alexandrelaine/noesis-mcp`. The Post-processing card gets an "MCP server" row with a "Register with Claude Code" button that runs that command through the located `claude` and shows success or the error text, plus a "Copy command" action for Codex and Cursor users. `mcp/README.md` documents both the npx route and the checkout route.

Example script. `scripts/post-enhance.sh` keeps its shape and loses the owner's rows: two example lines with `$HOME/path/to/wiki` placeholders.

Signing is out of scope here. `tauri.conf.json` keeps `signingIdentity: "-"` for local builds; `.github/workflows/build-macos.yml` already imports a Developer ID certificate from secrets.

Docs: `CLAUDE.md` (the fork paragraph mentions "Wiki automation") and spec `2026-10-27-personal-meeting-recorder-design.md` section 4.8 get a one-line pointer to this document.

## Contracts

- Folder layout, `metadata.json` v2, `transcript.md` and the MCP tool schemas (main spec section 5) do not change.
- `recording_preferences.json`: keys renamed as in section 1, read through aliases; new keys with defaults. A file written by today's build loads without loss.
- Environment of the hook (`NOESIS_*`, `MEETILY_*`) is unchanged.
- MCP root resolution gains one step (section 4) between the environment variables and the defaults.

## Phasing

Two plans, independent:

- Plan A, generic settings: sections 1, 3, 4 and the script cleanup. About a day.
- Plan B, calls and distribution: sections 2 and 5. About a day, plus the step-zero verification on a real Teams or Meet call.

## Testing

Rust (`cargo test` in `frontend/src-tauri`):

- Template rendering: all placeholders substituted, empty instructions leave no double space, unknown placeholder kept, empty render falls back.
- Preferences: a JSON with the old keys (`wiki_automation`, `wiki_targets` with `skill`, `wiki_budget_usd`) loads into the new fields and `skill` becomes `instructions`.
- `claude_args`: empty allowed or denied lists omit their flags.
- `recordings_root`: empty or missing `save_folder` falls back to the default.
- Calendar: `has_link` for each domain and for none.
- Call detector: a pure `choose_signal(process_objects: Option<bool>, cpthost: Option<bool>) -> Option<bool>` is true when either is true, and `None` only when neither answered.
- Language derivation: a pure function over a list of locale identifiers (`fr-FR`, `en-US`, `zz`) returns `["fr", "en"]`.

MCP (`npm test`): root resolution reads `save_folder` from an injected data dir path and ignores an unreadable or empty file.

Manual acceptance:

- Record from a Teams or Google Meet test call: recording starts (switch on) and stops within 30 seconds of leaving.
- Change the recordings folder, record a two-minute meeting: the folder lands under the new root, the sidebar lists it, `list_meetings` from a wiki sees it.
- Fresh user account with a copy of Meetily data: the dialog appears; "Skip" writes the marker and the second launch is silent.
- From a shell without the checkout on `PATH`: the Register button adds the server and `claude mcp list` shows `noesis`.
- The owner's existing settings load with the same behaviour as before: three project rows with instructions "Use the … skill." (the former DEX-specific sentence "do not score, do not update trackers or the Blueprint" is not migrated; the owner adds it to the row's instructions by hand before turning post-processing on), languages en and fr, auto-start on.
- Call signal on a real call: with a Zoom meeting live, then a Teams or Google Meet call in the browser, `cargo test --lib probe_process_objects -- --ignored --nocapture` shows that app's PID with running input; with no call it shows none; while Noesis records alone only Noesis's own PID does.
- Migration dialog left open: on a Mac with Meetily data, leave the dialog open 30 seconds, click Move, then confirm `~/Library/Application Support/com.noesis.app` holds the old Meetily database and the log says the dirs were moved, not "both exist".

## Decisions

1. macOS only. Windows and Linux builds are not a goal of this fork.
2. Generic call detection needs macOS 14; macOS 13 keeps Zoom-only detection.
3. Claude Code is the only built-in agent. Other agents go through the custom command.
4. Preference keys are renamed with serde aliases, not left under their old names.
5. npm scope `@alexandrelaine`.
6. Developer ID and notarization are the next step, outside this spec.
