# Model management and project overrides

Date: 2026-10-30. Status: approved design, not yet implemented.

Three small adjustments to the personal meeting recorder
([main spec](2026-10-27-personal-meeting-recorder-design.md)):

1. Delete a downloaded model from Settings.
2. The meeting info bubble names the model that transcribed the meeting, live and enhanced.
3. Per-project settings for the transcript language and the enhance model, written to a
   `project.json` in the project folder, plus a one-off "Enhance with..." picker on the meeting page.

The language override applies to the live transcript and to the enhanced one. The driving case:
Spanish meetings on one case, English transcripts for the wiki.

## 1. Delete a model

### What exists

- `whisper_delete_corrupted_model`, `parakeet_delete_corrupted_model` and `builtin_ai_delete_model`
  are registered Tauri commands. All three already delete a downloaded ("Available") model despite
  the names (`whisper_engine.rs:908`, `parakeet_engine.rs:572`, summary `model_manager.rs:823`).
- `components/ModelsSettings.tsx` lists the models of the three engines in one table; the
  `DownloadButton` cell shows "Downloaded" for a ready model and offers nothing else.

### Change

- UI: the "Downloaded" cell gains a trash icon button on its right. Click opens the existing
  `ConfirmationModal` with the text `Delete <label> (<size>)? You can download it again later.`
  On confirm the engine's delete command runs, then `refresh()`; the row flips back to the download
  button. Errors show as a toast.
- The trash is disabled, tooltip "Pick another model first", when the row is a current pick:
  transcription rows when the row is the Live pick (`transcriptModelConfig`) or the Enhance pick
  (`prefs.enhance_provider` and `prefs.enhance_model`); built-in rows when the row is the summary
  model (`summary.model` with `provider === 'builtin-ai'`) or the Ask model (`prefs.ask_model` with
  `prefs.ask_provider === 'builtin-ai'`).
- Rust: one guard in `WhisperEngine::delete_model` and `ParakeetEngine::delete_model`: when
  `get_current_model()` equals the name, return
  `Model in use: stop the recording or wait for the enhance job to finish`. The built-in delete keeps
  its behavior; the UI guard covers it.
- Command names stay as they are. Renaming would touch `lib/whisper.ts`, `lib/parakeet.ts` and the
  two legacy managers for no gain.

## 2. Info bubble names the models

### What exists

- `metadata.json` v2 already carries `transcription.live {provider, model}` (written at record
  start, `recording_saver.rs:369`) and `transcription.enhanced {provider, model, status, at, error}`
  (`enhance_status.rs:write_enhance_status`).
- `components/MeetingStatusTags.tsx` (`MeetingInfoButton`) receives only the enhance status string
  from the sidebar and the tagging status, and renders "Enhanced: Done".
- `components/MeetingDetails/TranscriptPanel.tsx` loads `get_meeting_extras` (tags,
  `audio_deleted_at`, `has_audio`) per meeting and reloads it on `tagging-status-changed`.

### Change

- `audio_cleanup::get_meeting_extras` returns one more field, `transcription`: the raw
  `transcription` object of `metadata.json`, or null when absent.
- `TranscriptPanel` passes it to `MeetingInfoButton` and also reloads the extras on
  `enhance-status-changed` for the current meeting, so the model appears when a job finishes.
- A pure helper `src/lib/languages.ts` formats the rows:
  - `liveLabel(t)`: `Parakeet parakeet-tdt-0.6b-v3-int8`, or `Unknown` when `transcription.live` is
    missing (meetings recorded before the key existed).
  - `enhancedLabel(status, t)`: `Done · Whisper large-v3 · to English`. The status word comes from
    the sidebar as today ("Not run" when none). The model part is `<Provider> <model>` from
    `transcription.enhanced` when present. The language suffix appears only when a language was
    forced: `to English` for `auto-translate`, the language name for a code (`Spanish`), nothing for
    `auto` or absent.
  - Provider names are capitalized (`whisper` becomes `Whisper`, `parakeet` becomes `Parakeet`).
  - The file also holds the shared language list and select options, so it stays import-free for the Node test runner.
- The bubble rows become Live, Enhanced, Title and tags. Width grows from `w-56` to `w-72`.

## 3. Per-project settings and the one-off picker

### What exists

- Live language: a process-wide preference (`lib.rs:129`, default `auto-translate`, set from the
  Settings select through `set_language_preference`). The live worker reads it per chunk
  (`transcription/worker.rs:446` and `:525`). Values: `auto` (keep the language, narrowed to
  `transcription_languages`), `auto-translate` (Whisper translates to English), or a code.
- Enhance: `start_retranscription_command(meeting_id, folder, language, model, provider)` already
  takes the three values per call. `lib/autoEnhance.ts` always passes the global preferences
  (`enhance_language`, `enhance_model`, `enhance_provider`). `enhance_language` exists in Rust and
  has no UI; it is always null.
- Parakeet ignores the language argument. It detects the language on its own and cannot translate.
- Projects are folders under the recordings root. `library::list_projects` lists directories
  without a `metadata.json`; `library::meeting_folders` and the MCP server (`mcp/src/library.ts`)
  only look at directories inside a project folder, so a file there is inert.

### 3.1 Data: `project.json`

Optional file `<root>/<project>/project.json`:

```json
{
  "language": "auto-translate",
  "enhance_provider": "whisper",
  "enhance_model": "large-v3"
}
```

- Every key is optional. A missing key means "use the global setting".
- `language` takes the same values as the live select: `auto`, `auto-translate`, or one of the codes
  in the Settings language list (`en`, `fr`, `de`, `es`, `it`, `nl`, `pt`). It drives both the live
  transcript and the enhance run.
- `enhance_provider` is `whisper` or `parakeet`; `enhance_model` is a model name of that engine.
  Both or neither are meaningful; the UI always writes them together.
- Unknown keys are preserved on write (read-modify-write, same pattern as `metadata_file.rs`).
- `_inbox` can have one too.
- An unreadable or invalid file logs a warning and counts as empty. It never blocks a recording
  start or an enhance run.

### 3.2 Module `audio/project_settings.rs`

```rust
#[derive(Default, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub language: Option<String>,
    pub enhance_provider: Option<String>,
    pub enhance_model: Option<String>,
}

pub fn read(root: &Path, project: &str) -> ProjectSettings;            // empty when missing or invalid
pub fn write(root: &Path, project: &str, s: &ProjectSettings) -> Result<()>; // temp file + rename, keeps unknown keys
pub fn resolve_language(root: &Path, project: &str) -> Option<String>;  // project.json language only
pub struct EnhanceSettings { pub language: Option<String>, pub provider: String, pub model: String }
pub fn resolve_enhance(root: &Path, folder: &Path, prefs: &RecordingPreferences,
                       language: Option<String>, provider: Option<String>, model: Option<String>) -> EnhanceSettings;
```

`resolve_enhance` order, per field: the explicit argument, then `project.json`
(project from `library::project_of(root, folder)`), then the preferences (`enhance_language`,
`enhance_provider`, `enhance_model`). Provider and model are taken as a pair from the same level, so
a project that sets `enhance_model` without `enhance_provider` is treated as unset.

Tauri commands, registered in `lib.rs`:

- `get_project_settings(project) -> ProjectSettings`
- `set_project_settings(project, settings) -> ()`; the project name is validated with
  `library::is_valid_project_name`, the folder is created when missing.
- `resolve_enhance_settings(meeting_folder_path) -> EnhanceSettings` with no explicit arguments,
  for the picker's "Default" hint.

### 3.3 Live transcript

- `lib.rs` gains a second static next to `LANGUAGE_PREFERENCE`:
  `RECORDING_LANGUAGE: Mutex<Option<String>>`. `get_language_preference_internal()` returns it when
  set, else the global preference. Nothing else in the live path changes.
- Set in both record-start paths of `recording_commands.rs` (`start_recording_with_meeting_name`
  and the device-specific start around line 517) once `manager.start_recording` has returned Ok, from
  `project_settings::resolve_language(root, project)`. Updated in `set_recording_project` from the
  normalized new project (the folder moves later, the language applies now). Cleared in
  `stop_recording` once the transcription worker has drained the last chunks (after the force flush), so the tail of the live transcript keeps the project language. The tray, the UI and auto-stop all go through it.
- `metadata.json` `transcription.live` gains `language`: the effective value at start, that is the
  project override or else the global preference. `set_live_transcription` carries a third element.
- With Parakeet as the Live model the override cannot translate; the live text stays in the spoken
  language and the enhance run does the translation when its engine is Whisper. The Settings hint
  states it.

### 3.4 Enhance

- `start_retranscription` (the async function, not the command) resolves the three values through
  `project_settings::resolve_enhance` before `jobs::add`, so the job record and a later Retry
  replay exactly what ran. Null arguments mean "resolve"; explicit arguments win.
- `write_enhance_status` gains a `language: Option<&str>` parameter and writes
  `transcription.enhanced.language`. `mark_interrupted` keeps the key as it keeps provider and model.
- `RetranscriptionStarted` gains `provider`, `model` and `language` so the toast names what runs.
  The command resolves once up front to fill them and passes the resolved values on.
- `lib/autoEnhance.ts`: `startEnhance(meetingId, folderPath, override?)` passes
  `override.language`, `override.provider` and `override.model`, null when the caller picked nothing.
  `startAutoEnhanceIfEnabled` still checks `auto_enhance` and `auto_save` and passes no override.
  The toast description uses the returned values.
- Today a null model means "the live model" (`retranscription.rs:895`). The preferences always
  carry an enhance provider and model, so after resolution that fallback is unreachable. It is left
  in place, untouched.

### 3.5 UI

Settings > Models > Audio transcription gains a "Per project" row under the model table:

- One row per project from `list_projects_command` (`_inbox` first).
- Column Language: a select with `Default`, `Keep the language`, `Translate to English`, then the
  seven languages. `Default` shows the global value in parentheses, taken from the live select
  (`selectedLanguage`).
- Column Enhance model: a select with `Default (<global provider model>)` then every downloaded
  Whisper or Parakeet model.
- A change calls `set_project_settings` at once and shows a toast on failure. Picking `Default`
  removes the key (`language`), or both keys for the model column.
- Hint under the table: "Language applies live and to the enhance run. Live translation needs
  Whisper as the Live model."

Meeting page, Actions menu (`TranscriptButtonGroup.tsx`):

- "Enhance" stays and runs with the resolved settings (passes no override).
- New item "Enhance with..." below it, disabled like Enhance when the audio is gone. It opens a
  `Dialog` (`components/ui/dialog.tsx`) with the same two selects, both starting at `Default`, a
  one-line hint `Default: <language> · <Provider> <model>` from `resolve_enhance_settings`, and a
  Run button that calls `startEnhance` with the picked values only. One-off: nothing persists except
  what `metadata.json` records.

Skipped: a mid-recording language toggle. The global Settings select already applies live per chunk
and the project select on the home page now re-resolves the language.

## 4. Contracts (additive to main spec section 5)

- 5.1 folder layout: `<root>/<project>/project.json` is optional, keys `language`,
  `enhance_provider`, `enhance_model`, all optional.
- 5.2 `metadata.json` v2: `transcription.live.language` and `transcription.enhanced.language` are
  optional strings. Absent means the global setting of the time applied. Consumers keep ignoring
  unknown keys.
- 5.3 `transcript.md` is unchanged.
- The MCP server does not read `project.json` for now.

## 5. Error handling

- Delete: an engine error (model in use, file system) is returned to the UI and shown as a toast.
  A file already missing on disk is not an error; the status becomes Missing.
- `project.json` unreadable or invalid: warning in the log, treated as empty.
- "Enhance with..." lists downloaded models only, so a missing model cannot be picked.
- Recording start never fails because of project settings.

## 6. Tests

Rust, next to the code:

- `project_settings`: read of a missing file is empty; partial keys; unknown keys survive a write;
  `resolve_enhance` honors argument, then project, then preferences; a project with
  `enhance_model` but no `enhance_provider` falls through.
- `lib.rs`: `get_language_preference_internal` returns the recording override when set and the
  global otherwise.
- `enhance_status`: `write_enhance_status` writes `language`; `mark_interrupted` keeps it.
- `audio_cleanup`: `get_meeting_extras` returns the `transcription` object, null when absent.
- Engines: `delete_model` refuses the currently loaded model.

Frontend, `node --test src/lib/*.test.ts` as the existing tests:

- `languages.test.ts`: live label, enhanced label with and without model, each language
  suffix, `Unknown` for an old meeting.

Manual pass before merge: delete and re-download a model; record in a project set to translate and
check the live text and `transcription.live.language`; run "Enhance with..." and check the bubble.

## 7. Out of scope

- Per-meeting persisted overrides. The project carries the default, the picker covers exceptions.
- Recording the detected language. Only the requested value is recorded.
- Exposing `project.json` through the MCP server.
- Renaming the `*_delete_corrupted_model` commands.
