# Model management and project overrides Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete downloaded models from Settings, show the live and enhance engines in the meeting info bubble, and let a project (or a one-off picker) override the transcript language and the enhance model.

**Architecture:** One new Rust module `audio/project_settings.rs` owns `<root>/<project>/project.json` and the resolution rule (explicit value, then project, then preferences). The live transcript honors a recording-scoped language override held in `lib.rs`; the enhance path resolves inside `start_retranscription` so the job record carries what ran. `metadata.json` gains `language` under `transcription.live` and `transcription.enhanced`. The frontend gets a shared `lib/languages.ts` (options and bubble labels), a delete button in the models table, a per-project table in Settings, and an "Enhance with..." dialog on the meeting page.

**Tech Stack:** Rust (Tauri 2, serde_json, anyhow, tempfile for tests), Next.js 14 + React 18 + TypeScript, shadcn/radix `Dialog` and `Select`, lucide icons, Node's built-in test runner for `src/lib/*.test.ts`.

Spec: [docs/superpowers/specs/2026-10-30-model-management-and-project-overrides-design.md](../specs/2026-10-30-model-management-and-project-overrides-design.md). Read it first.

## Global Constraints

- Working directory for Rust commands: `frontend/src-tauri`. Tests: `cargo test --lib <module path>`. Type check: `cargo check`.
- Working directory for frontend commands: `frontend`. Tests: `pnpm test` (runs `node --test src/lib/*.test.ts`, Node 26, TypeScript stripped natively). Type check and lint: `pnpm exec tsc --noEmit && pnpm lint`.
- Files under `src/lib/` that a test imports must be self-contained: no relative imports (the tsconfig has no `allowImportingTsExtensions`, Node needs the `.ts` extension). Test files import with the `.ts` extension, as `src/lib/dateBuckets.test.ts` does.
- Additive changes only. Keep the existing Tauri command names, including `whisper_delete_corrupted_model` and `parakeet_delete_corrupted_model`.
- `metadata.json` v2 keys stay readable by the current wiki scripts. New keys are optional: `transcription.live.language`, `transcription.enhanced.language`.
- `project.json` keys: `language`, `enhance_provider`, `enhance_model`, all optional. `language` values: `auto`, `auto-translate`, or a code from the Settings language list (`en`, `fr`, `de`, `es`, `it`, `nl`, `pt`).
- Resolution order, per field: explicit argument, then `project.json`, then the recording preferences. Provider and model are taken as a pair from the same level.
- Copy, verbatim: delete confirmation `Delete <label> (<size>)? You can download it again later.`; disabled delete tooltip `Pick another model first`; engine error `Model in use: stop the recording or wait for the enhance job to finish`; Settings hint `Language applies live and to the enhance run. Live translation needs Whisper as the Live model.`
- Commit after every task with the message given in its last step. Branch: `enhance/project-overrides` created from `main` after merging `docs/model-management-and-project-overrides`.

## File structure

Rust (`frontend/src-tauri/src/`):

- Create `audio/project_settings.rs`: `ProjectSettings`, `EnhanceSettings`, `read`, `write`, `resolve_language`, `resolve_enhance`, three Tauri commands.
- Modify `audio/mod.rs`: declare the module.
- Modify `lib.rs`: `RECORDING_LANGUAGE` static, `set_recording_language`, `get_language_preference_internal` precedence, command registration.
- Modify `audio/recording_commands.rs`: set the override at both record starts and on `set_recording_project`, clear it in `stop_recording`, record the live language.
- Modify `audio/recording_saver.rs` and `audio/recording_manager.rs`: `live_transcription` carries the language.
- Modify `audio/enhance_status.rs`: `language` in `write_enhance_status` and `mark_interrupted`.
- Modify `audio/retranscription.rs`: resolution in `start_retranscription` and the command, `RetranscriptionStarted` fields, `set_enhance_status` parameter.
- Modify `audio/audio_cleanup.rs`: `extras_of` with `transcription`.
- Modify `whisper_engine/whisper_engine.rs` and `parakeet_engine/parakeet_engine.rs`: in-use guard in `delete_model`.

Frontend (`frontend/src/`):

- Create `lib/languages.ts` (+ `lib/languages.test.ts`): language list, select options, setting labels, bubble row labels.
- Create `lib/models.ts`: `listReadyTranscriptionModels`.
- Create `components/ProjectOverrides.tsx`: per-project table.
- Create `components/MeetingDetails/EnhanceWithDialog.tsx`: one-off picker.
- Modify `components/ModelsSettings.tsx`: delete button, in-use rule, per-project row, import `LANGUAGES` from the lib.
- Modify `components/MeetingStatusTags.tsx`, `components/MeetingDetails/TranscriptPanel.tsx`: bubble rows and reload.
- Modify `components/MeetingDetails/TranscriptButtonGroup.tsx`: "Enhance with..." item.
- Modify `lib/autoEnhance.ts`: override argument, toast from the returned values.

Docs: main spec section 5 notes, `CLAUDE.md` one bullet.

---

### Task 0: Branch

**Files:** none.

- [ ] **Step 1: Merge the spec branch and create the feature branch**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git checkout main
git merge --no-ff docs/model-management-and-project-overrides -m "Merge branch 'docs/model-management-and-project-overrides': spec for model delete, bubble models, project overrides"
git checkout -b enhance/project-overrides
```

Expected: branch `enhance/project-overrides` on top of main with the spec present at `docs/superpowers/specs/2026-10-30-model-management-and-project-overrides-design.md`.

---

### Task 1: `project_settings.rs` module and commands

**Files:**
- Create: `frontend/src-tauri/src/audio/project_settings.rs`
- Modify: `frontend/src-tauri/src/audio/mod.rs` (after the `pub mod claude_mcp;` line)
- Modify: `frontend/src-tauri/src/lib.rs` (invoke handler, after `audio::library::list_projects_command,`)

**Interfaces:**
- Consumes: `super::library::{project_of, is_valid_project_name}`, `super::recording_preferences::{RecordingPreferences, recordings_root, load_recording_preferences}`. `RecordingPreferences` implements `Default` and has pub fields `enhance_language: Option<String>`, `enhance_provider: String`, `enhance_model: String`.
- Produces:
  - `pub struct ProjectSettings { language: Option<String>, enhance_provider: Option<String>, enhance_model: Option<String> }` (Serialize, Deserialize, Default, PartialEq, Clone, Debug)
  - `pub struct EnhanceSettings { language: Option<String>, provider: String, model: String }` (same derives)
  - `pub fn read(root: &Path, project: &str) -> ProjectSettings`
  - `pub fn write(root: &Path, project: &str, settings: &ProjectSettings) -> anyhow::Result<()>`
  - `pub fn resolve_language(root: &Path, project: &str) -> Option<String>`
  - `pub fn resolve_enhance(root: &Path, folder: &Path, prefs: &RecordingPreferences, language: Option<String>, provider: Option<String>, model: Option<String>) -> EnhanceSettings`
  - Tauri commands `get_project_settings(project)`, `set_project_settings(project, settings)`, `resolve_enhance_settings(meeting_folder_path)`.

- [ ] **Step 1: Write the module with its tests**

Create `frontend/src-tauri/src/audio/project_settings.rs`:

```rust
//! Per-project settings in `<root>/<project>/project.json` (spec 2026-10-30 section 3).
//! Every key is optional; a missing key means the global setting. Unknown keys survive a
//! write. An unreadable or invalid file counts as empty and never blocks a recording.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Runtime};

use super::recording_preferences::RecordingPreferences;

pub const FILE_NAME: &str = "project.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    /// `auto`, `auto-translate` or a language code; drives the live transcript and the enhance run
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enhance_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enhance_model: Option<String>,
}

/// What one enhance run uses once every level has been applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnhanceSettings {
    pub language: Option<String>,
    pub provider: String,
    pub model: String,
}

fn file_path(root: &Path, project: &str) -> PathBuf {
    root.join(project).join(FILE_NAME)
}

/// The file as a JSON object; `{}` when missing, unreadable or not an object (a warning is logged).
fn read_raw(root: &Path, project: &str) -> serde_json::Map<String, serde_json::Value> {
    let p = file_path(root, project);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Default::default(),
        Err(e) => {
            log::warn!("project settings: cannot read {}: {}", p.display(), e);
            return Default::default();
        }
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(m)) => m,
        Ok(_) => {
            log::warn!("project settings: {} is not a JSON object; ignored", p.display());
            Default::default()
        }
        Err(e) => {
            log::warn!("project settings: {} is not valid JSON ({}); ignored", p.display(), e);
            Default::default()
        }
    }
}

pub fn read(root: &Path, project: &str) -> ProjectSettings {
    serde_json::from_value(serde_json::Value::Object(read_raw(root, project))).unwrap_or_default()
}

/// Write the three keys and keep every other key. Creates the project folder when missing.
pub fn write(root: &Path, project: &str, settings: &ProjectSettings) -> Result<()> {
    if !super::library::is_valid_project_name(project) {
        return Err(anyhow!("Invalid project name '{}'", project));
    }
    let dir = root.join(project);
    std::fs::create_dir_all(&dir)?;
    let mut obj = read_raw(root, project);
    for (key, value) in [
        ("language", &settings.language),
        ("enhance_provider", &settings.enhance_provider),
        ("enhance_model", &settings.enhance_model),
    ] {
        match value {
            Some(v) => {
                obj.insert(key.to_string(), serde_json::Value::String(v.clone()));
            }
            None => {
                obj.remove(key);
            }
        }
    }
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(obj))?;
    let tmp = dir.join(format!(".{}.{}.tmp", FILE_NAME, uuid::Uuid::new_v4()));
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, dir.join(FILE_NAME)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e
    })?;
    Ok(())
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.is_empty())
}

/// The project's language override, if any.
pub fn resolve_language(root: &Path, project: &str) -> Option<String> {
    non_empty(read(root, project).language)
}

/// Per field: the explicit argument, then project.json, then the preferences. Provider and
/// model are taken as a pair from the same level: a project that sets only one of them is
/// treated as unset for both.
pub fn resolve_enhance(
    root: &Path,
    folder: &Path,
    prefs: &RecordingPreferences,
    language: Option<String>,
    provider: Option<String>,
    model: Option<String>,
) -> EnhanceSettings {
    let project = super::library::project_of(root, folder);
    let ps = read(root, &project);
    let language = non_empty(language)
        .or_else(|| non_empty(ps.language.clone()))
        .or_else(|| non_empty(prefs.enhance_language.clone()));
    let pair = |p: Option<String>, m: Option<String>| match (non_empty(p), non_empty(m)) {
        (Some(p), Some(m)) => Some((p, m)),
        _ => None,
    };
    let (provider, model) = pair(provider, model)
        .or_else(|| pair(ps.enhance_provider.clone(), ps.enhance_model.clone()))
        .unwrap_or_else(|| (prefs.enhance_provider.clone(), prefs.enhance_model.clone()));
    EnhanceSettings { language, provider, model }
}

#[tauri::command]
pub async fn get_project_settings<R: Runtime>(app: AppHandle<R>, project: String) -> Result<ProjectSettings, String> {
    let root = super::recording_preferences::recordings_root(&app);
    Ok(read(&root, &project))
}

#[tauri::command]
pub async fn set_project_settings<R: Runtime>(app: AppHandle<R>, project: String, settings: ProjectSettings) -> Result<(), String> {
    let root = super::recording_preferences::recordings_root(&app);
    write(&root, &project, &settings).map_err(|e| e.to_string())
}

/// What an enhance run of this meeting uses when nothing is picked explicitly.
#[tauri::command]
pub async fn resolve_enhance_settings<R: Runtime>(app: AppHandle<R>, meeting_folder_path: String) -> Result<EnhanceSettings, String> {
    let root = super::recording_preferences::recordings_root(&app);
    let prefs = super::recording_preferences::load_recording_preferences(&app).await.unwrap_or_default();
    Ok(resolve_enhance(&root, Path::new(&meeting_folder_path), &prefs, None, None, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs() -> RecordingPreferences {
        RecordingPreferences {
            enhance_language: None,
            enhance_provider: "whisper".into(),
            enhance_model: "large-v3".into(),
            ..RecordingPreferences::default()
        }
    }

    fn settings(language: Option<&str>, provider: Option<&str>, model: Option<&str>) -> ProjectSettings {
        ProjectSettings {
            language: language.map(str::to_string),
            enhance_provider: provider.map(str::to_string),
            enhance_model: model.map(str::to_string),
        }
    }

    #[test]
    fn missing_file_reads_empty_and_resolves_to_preferences() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(read(root.path(), "dex"), ProjectSettings::default());
        assert_eq!(resolve_language(root.path(), "dex"), None);
        let folder = root.path().join("dex").join("2026-10-30_10-00 standup");
        let r = resolve_enhance(root.path(), &folder, &prefs(), None, None, None);
        assert_eq!(r, EnhanceSettings { language: None, provider: "whisper".into(), model: "large-v3".into() });
    }

    #[test]
    fn write_keeps_unknown_keys_and_removes_cleared_ones() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("dex");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE_NAME), r#"{"language":"es","note":"keep me"}"#).unwrap();

        write(root.path(), "dex", &settings(None, Some("parakeet"), Some("parakeet-tdt-0.6b-v3-int8"))).unwrap();

        let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join(FILE_NAME)).unwrap()).unwrap();
        assert_eq!(raw["note"], "keep me");
        assert!(raw.get("language").is_none(), "cleared key removed");
        assert_eq!(raw["enhance_provider"], "parakeet");
        assert_eq!(read(root.path(), "dex").enhance_model.as_deref(), Some("parakeet-tdt-0.6b-v3-int8"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temp file left behind");
    }

    #[test]
    fn write_creates_the_project_folder_and_rejects_bad_names() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "new-case", &settings(Some("auto-translate"), None, None)).unwrap();
        assert!(root.path().join("new-case").is_dir());
        assert_eq!(resolve_language(root.path(), "new-case").as_deref(), Some("auto-translate"));
        assert!(write(root.path(), "../x", &ProjectSettings::default()).is_err());
        assert!(write(root.path(), ".hidden", &ProjectSettings::default()).is_err());
    }

    #[test]
    fn invalid_json_counts_as_empty() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("dex");
        std::fs::create_dir_all(&dir).unwrap();
        for bad in ["{not json", "[1,2]", r#"{"language": 7}"#] {
            std::fs::write(dir.join(FILE_NAME), bad).unwrap();
            assert_eq!(read(root.path(), "dex"), ProjectSettings::default(), "{}", bad);
        }
    }

    #[test]
    fn resolve_order_is_argument_then_project_then_preferences() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "dex", &settings(Some("auto-translate"), Some("parakeet"), Some("p-v3"))).unwrap();
        let folder = root.path().join("dex").join("2026-10-30_10-00 standup");

        let project_wins = resolve_enhance(root.path(), &folder, &prefs(), None, None, None);
        assert_eq!(project_wins, EnhanceSettings { language: Some("auto-translate".into()), provider: "parakeet".into(), model: "p-v3".into() });

        let arg_wins = resolve_enhance(root.path(), &folder, &prefs(), Some("es".into()), Some("whisper".into()), Some("medium".into()));
        assert_eq!(arg_wins, EnhanceSettings { language: Some("es".into()), provider: "whisper".into(), model: "medium".into() });

        // an explicit provider without a model is not a pair: the project pair applies
        let half = resolve_enhance(root.path(), &folder, &prefs(), None, Some("whisper".into()), None);
        assert_eq!((half.provider.as_str(), half.model.as_str()), ("parakeet", "p-v3"));

        // a meeting in another project sees only the preferences
        let other = root.path().join("other").join("2026-10-30_11-00 x");
        let mut p = prefs();
        p.enhance_language = Some("fr".into());
        assert_eq!(
            resolve_enhance(root.path(), &other, &p, None, None, None),
            EnhanceSettings { language: Some("fr".into()), provider: "whisper".into(), model: "large-v3".into() }
        );
    }

    #[test]
    fn a_project_with_model_but_no_provider_falls_through() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "dex", &settings(None, None, Some("medium"))).unwrap();
        let folder = root.path().join("dex").join("2026-10-30_10-00 standup");
        let r = resolve_enhance(root.path(), &folder, &prefs(), None, None, None);
        assert_eq!((r.provider.as_str(), r.model.as_str()), ("whisper", "large-v3"));
    }
}
```

- [ ] **Step 2: Declare the module**

In `frontend/src-tauri/src/audio/mod.rs`, after the line `pub mod claude_mcp;`, add:

```rust
pub mod project_settings; // per-project language and enhance model (spec 2026-10-30)
```

- [ ] **Step 3: Run the tests, expect them to fail to compile only if something is missing, else pass**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend/src-tauri && cargo test --lib audio::project_settings
```

Expected: `test result: ok. 6 passed`. If `RecordingPreferences` does not implement `Default` or a field is private, fix the test helper to construct it the way `recording_preferences.rs` tests do (search `RecordingPreferences::default()` there).

- [ ] **Step 4: Register the commands**

In `frontend/src-tauri/src/lib.rs`, inside `tauri::generate_handler![`, right after the line `audio::library::list_projects_command,`, add:

```rust
            audio::project_settings::get_project_settings,
            audio::project_settings::set_project_settings,
            audio::project_settings::resolve_enhance_settings,
```

- [ ] **Step 5: Type check**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend/src-tauri && cargo check 2>&1 | tail -5
```

Expected: `Finished` with no errors (warnings about unused functions are fine until Task 3).

- [ ] **Step 6: Commit**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git add frontend/src-tauri/src/audio/project_settings.rs frontend/src-tauri/src/audio/mod.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(project): project.json settings module, resolution rule and commands"
```

---

### Task 2: Live language override

**Files:**
- Modify: `frontend/src-tauri/src/lib.rs` (around lines 450-454, `get_language_preference_internal`)
- Modify: `frontend/src-tauri/src/audio/recording_commands.rs` (both start paths around lines 395-425 and 600-625; `stop_recording` at 710; `set_recording_project` at 1301)
- Modify: `frontend/src-tauri/src/audio/recording_saver.rs` (lines 81, 100, 175-176, 369-371)
- Modify: `frontend/src-tauri/src/audio/recording_manager.rs` (line 556)

**Interfaces:**
- Consumes: `audio::project_settings::resolve_language(root, project)` from Task 1; `library::{normalize_project, INBOX}`; `recording_preferences::recordings_root(&app)`.
- Produces: `crate::set_recording_language(language: Option<String>)`; `get_language_preference_internal()` now returns the override first; `RecordingManager::set_live_transcription(Option<(String, String, String)>)` (provider, model, language); `metadata.json` `transcription.live.language`.

- [ ] **Step 1: Write the failing test and the override in `lib.rs`**

Replace, in `frontend/src-tauri/src/lib.rs`:

```rust
// Internal helper function to get language preference (for use within Rust code)
pub fn get_language_preference_internal() -> Option<String> {
    LANGUAGE_PREFERENCE.lock().ok().map(|lang| lang.clone())
}
```

with:

```rust
// spec 2026-10-30 section 3.3: the live recording's language, from the project's project.json.
// Set at record start and on a live project change, cleared at stop. None means the global preference.
static RECORDING_LANGUAGE: StdMutex<Option<String>> = StdMutex::new(None);

pub fn set_recording_language(language: Option<String>) {
    if let Ok(mut l) = RECORDING_LANGUAGE.lock() {
        log_info!("Recording language override: {:?}", language);
        *l = language;
    }
}

// Internal helper function to get language preference (for use within Rust code).
// The recording override wins over the global preference while a recording runs.
pub fn get_language_preference_internal() -> Option<String> {
    if let Some(l) = RECORDING_LANGUAGE.lock().ok().and_then(|g| g.clone()) {
        return Some(l);
    }
    LANGUAGE_PREFERENCE.lock().ok().map(|lang| lang.clone())
}

#[cfg(test)]
mod recording_language_tests {
    use super::*;

    #[test]
    fn override_wins_while_set_and_the_global_returns_after_clear() {
        let global = get_language_preference_internal();
        set_recording_language(Some("auto-translate-test".to_string()));
        assert_eq!(get_language_preference_internal().as_deref(), Some("auto-translate-test"));
        set_recording_language(None);
        assert_eq!(get_language_preference_internal(), global);
    }
}
```

- [ ] **Step 2: Run the test**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend/src-tauri && cargo test --lib recording_language_tests
```

Expected: `1 passed`.

- [ ] **Step 3: Carry the language in `live_transcription`**

In `frontend/src-tauri/src/audio/recording_saver.rs`:

Line 81, replace `live_transcription: Option<(String, String)>,` with:

```rust
    /// (provider, model, language) of the live engine; spec 5.2 and spec 2026-10-30 section 3.3
    live_transcription: Option<(String, String, String)>,
```

Lines 174-176, replace the setter with:

```rust
    /// spec 5.2: provider, model and effective language of the live engine, recorded under transcription.live
    pub fn set_live_transcription(&mut self, provider_model_language: Option<(String, String, String)>) {
        self.live_transcription = provider_model_language;
    }
```

Lines 369-371, replace the `transcription:` field with:

```rust
            transcription: self.live_transcription.as_ref().map(|(provider, model, language)| {
                serde_json::json!({ "live": { "provider": provider, "model": model, "language": language } })
            }),
```

In `frontend/src-tauri/src/audio/recording_manager.rs` line 556, replace the forwarder with:

```rust
    pub fn set_live_transcription(&mut self, provider_model_language: Option<(String, String, String)>) {
        self.recording_saver.set_live_transcription(provider_model_language);
    }
```

- [ ] **Step 4: Set the override at both record starts**

In `frontend/src-tauri/src/audio/recording_commands.rs`, in `start_recording_with_meeting_name` (around line 401), replace:

```rust
    manager.set_project(project.clone());
    manager.set_root(crate::audio::recording_preferences::recordings_root(&app));
    manager.set_owner_name(prefs_owner_name); // spec 4.4
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        match crate::database::repositories::setting::SettingsRepository::get_transcript_config(state.db_manager.pool()).await {
            Ok(Some(cfg)) => manager.set_live_transcription(Some((cfg.provider.clone(), cfg.model.clone()))),
            Ok(None) => manager.set_live_transcription(Some(("parakeet".to_string(), crate::config::DEFAULT_PARAKEET_MODEL.to_string()))),
            Err(e) => warn!("Could not read transcript config for metadata: {}", e),
        }
    }
```

with:

```rust
    manager.set_project(project.clone());
    let root = crate::audio::recording_preferences::recordings_root(&app);
    manager.set_root(root.clone());
    manager.set_owner_name(prefs_owner_name); // spec 4.4
    // spec 2026-10-30 section 3.3: the project's language drives the live transcript; cleared on
    // a failed start below and in stop_recording
    let project_name = super::library::normalize_project(project.as_deref().unwrap_or(super::library::INBOX));
    crate::set_recording_language(super::project_settings::resolve_language(&root, &project_name));
    let live_language = crate::get_language_preference_internal().unwrap_or_default();
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        match crate::database::repositories::setting::SettingsRepository::get_transcript_config(state.db_manager.pool()).await {
            Ok(Some(cfg)) => manager.set_live_transcription(Some((cfg.provider.clone(), cfg.model.clone(), live_language.clone()))),
            Ok(None) => manager.set_live_transcription(Some(("parakeet".to_string(), crate::config::DEFAULT_PARAKEET_MODEL.to_string(), live_language.clone()))),
            Err(e) => warn!("Could not read transcript config for metadata: {}", e),
        }
    }
```

Then, in the same function, replace:

```rust
    let transcription_receiver = manager
        .start_recording(microphone_device, system_device, auto_save)
        .await
        .map_err(|error| map_recording_start_error(&app, error))?;
```

with:

```rust
    let transcription_receiver = match manager.start_recording(microphone_device, system_device, auto_save).await {
        Ok(receiver) => receiver,
        Err(error) => {
            crate::set_recording_language(None);
            return Err(map_recording_start_error(&app, error));
        }
    };
```

Apply the same two replacements in the second start function (the one whose `project: Option<String>` parameter is declared around line 517 and whose `set_project` call is around line 605). Its start call uses `mic_device, system_device, auto_save` instead of `microphone_device, system_device, auto_save`; keep those names.

- [ ] **Step 5: Re-resolve on a live project change and clear at stop**

Replace the whole `set_recording_project` command (around line 1300) with:

```rust
/// Change the project of the live recording. The folder is not moved now (the saver and
/// checkpoints hold its path); `take_pending_project` hands the move to the row save.
/// The new project's language applies from now on (spec 2026-10-30 section 3.3).
/// Returns the normalized project name.
#[tauri::command]
pub async fn set_recording_project<R: Runtime>(app: AppHandle<R>, project: String) -> Result<String, String> {
    let guard = RECORDING_MANAGER.lock().unwrap();
    let Some(manager) = guard.as_ref() else { return Err("No recording in progress.".to_string()) };
    let Some(meeting_id) = manager.get_meeting_id() else { return Err("No recording in progress.".to_string()) };
    let started = manager.get_project().unwrap_or_else(|| super::library::INBOX.to_string());
    let normalized = super::library::normalize_project(&project);
    let pending = super::library::pending_move(&started, Some(&project));
    info!("Recording {} project change: {:?} -> pending {:?}", meeting_id, started, pending);
    *PENDING_PROJECT.lock().unwrap() = pending.map(|p| (meeting_id, p));
    let root = crate::audio::recording_preferences::recordings_root(&app);
    crate::set_recording_language(super::project_settings::resolve_language(&root, &normalized));
    Ok(normalized)
}
```

The frontend call `invoke('set_recording_project', { project })` is unchanged: Tauri injects `app`.

In `stop_recording` (around line 714), right after the opening `info!("🛑 Starting optimized recording shutdown ...");` statement and before the `IS_RECORDING` check, add:

```rust
    // spec 2026-10-30 section 3.3: the project language override ends with the recording
    crate::set_recording_language(None);
```

- [ ] **Step 6: Type check and run the audio tests**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend/src-tauri && cargo check 2>&1 | tail -5 && cargo test --lib audio::recording_saver audio::library recording_language_tests 2>&1 | tail -5
```

Expected: `Finished`, all tests pass. If `cargo check` reports another `set_live_transcription` caller, update it to the triple.

- [ ] **Step 7: Commit**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git add frontend/src-tauri/src/lib.rs frontend/src-tauri/src/audio/recording_commands.rs frontend/src-tauri/src/audio/recording_saver.rs frontend/src-tauri/src/audio/recording_manager.rs
git commit -m "feat(live): project language override for the live transcript; transcription.live.language in metadata"
```

---

### Task 3: Enhance resolution and `transcription.enhanced.language`

**Files:**
- Modify: `frontend/src-tauri/src/audio/enhance_status.rs` (`write_enhance_status` lines 23-50, `mark_interrupted` lines 70-99, tests)
- Modify: `frontend/src-tauri/src/audio/retranscription.rs` (`start_retranscription` lines 135-212, `set_enhance_status` lines 282-298, `RetranscriptionStarted` line 1063, `start_retranscription_command` lines 1070-1118)

**Interfaces:**
- Consumes: `audio::project_settings::{resolve_enhance, EnhanceSettings}` from Task 1.
- Produces: `write_enhance_status(folder, provider, model, language: Option<&str>, status, error)`; `RetranscriptionStarted { meeting_id, message, provider, model, language: Option<String> }` (the frontend reads `provider`, `model`, `language`).

- [ ] **Step 1: Write the failing test in `enhance_status.rs`**

Add inside `mod tests`:

```rust
    #[test]
    fn language_is_written_when_given_and_kept_by_mark_interrupted() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("proj").join("m");
        std::fs::create_dir_all(&dir).unwrap();

        write_enhance_status(&dir, "whisper", "large-v3", Some("auto-translate"), STATUS_RUNNING, None).unwrap();
        assert_eq!(read_root(&dir)["transcription"]["enhanced"]["language"], "auto-translate");

        assert_eq!(mark_interrupted(root.path()), 1);
        assert_eq!(read_root(&dir)["transcription"]["enhanced"]["language"], "auto-translate", "kept like provider and model");

        write_enhance_status(&dir, "whisper", "large-v3", None, STATUS_DONE, None).unwrap();
        assert!(read_root(&dir)["transcription"]["enhanced"]["language"].is_null());
    }
```

Update the existing test calls to the new arity by inserting `None,` after the model argument:
`write_enhance_status(dir.path(), "whisper", "large-v3", None, STATUS_QUEUED, None)`,
`... None, STATUS_FAILED, Some("boom"))`, `... None, STATUS_RUNNING, None)`.

- [ ] **Step 2: Run it, expect a compile error on the arity**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend/src-tauri && cargo test --lib audio::enhance_status 2>&1 | grep -E "error|passed|failed" | head
```

Expected: `error[E0061]: this function takes 5 arguments but 6 arguments were supplied`.

- [ ] **Step 3: Add the parameter**

Replace `write_enhance_status` (lines 20-50) with:

```rust
/// Merge `transcription.enhanced = {provider, model, language, status, at, error}` into
/// metadata.json, creating the file when it is missing. Every other key is kept.
/// `language` is the requested value (`auto-translate`, a code) or None for auto
/// (spec 2026-10-30 section 3.4).
pub fn write_enhance_status(
    folder: &Path,
    provider: &str,
    model: &str,
    language: Option<&str>,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    super::metadata_file::update_metadata(folder, |obj| {
        let transcription = obj
            .entry("transcription")
            .or_insert_with(|| serde_json::json!({}));
        if !transcription.is_object() {
            *transcription = serde_json::json!({});
        }
        transcription.as_object_mut().unwrap().insert(
            "enhanced".to_string(),
            serde_json::json!({
                "provider": provider,
                "model": model,
                "language": language,
                "status": status,
                "at": chrono::Utc::now().to_rfc3339(),
                "error": error,
            }),
        );
        Ok(())
    })?;
    Ok(())
}
```

In `mark_interrupted`, replace:

```rust
            let provider = enhanced["provider"].as_str().unwrap_or("whisper");
            let model = enhanced["model"].as_str().unwrap_or("configured");
            match write_enhance_status(&folder, provider, model, STATUS_FAILED, Some(INTERRUPTED)) {
```

with:

```rust
            let provider = enhanced["provider"].as_str().unwrap_or("whisper");
            let model = enhanced["model"].as_str().unwrap_or("configured");
            let language = enhanced["language"].as_str();
            match write_enhance_status(&folder, provider, model, language, STATUS_FAILED, Some(INTERRUPTED)) {
```

- [ ] **Step 4: Thread the language through `retranscription.rs`**

Replace `set_enhance_status` (lines 282-298) with:

```rust
/// Write the status to metadata.json and tell the UI. Failures to write are
/// logged, never fatal: the transcript matters more than the badge.
fn set_enhance_status<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    folder: &Path,
    provider: &str,
    model: &str,
    language: Option<&str>,
    status: &str,
    error: Option<&str>,
) {
    if let Err(e) = enhance_status::write_enhance_status(folder, provider, model, language, status, error) {
        warn!("Could not write enhance status '{}' for {}: {}", status, meeting_id, e);
    }
    let _ = app.emit(
        "enhance-status-changed",
        serde_json::json!({ "meeting_id": meeting_id, "status": status }),
    );
}
```

In `start_retranscription`, replace the first three lines of the body:

```rust
    let queued_folder = PathBuf::from(&meeting_folder_path);
    let provider_name = provider.clone().unwrap_or_else(|| "whisper".to_string());
    let model_name = model.clone().unwrap_or_else(|| "configured".to_string());
```

with:

```rust
    let queued_folder = PathBuf::from(&meeting_folder_path);
    // spec 2026-10-30 section 3.4: explicit values win, then the project's project.json, then the
    // preferences. Resolved before jobs::add so the job record and a Retry replay what ran.
    let resolved = {
        let root = super::recording_preferences::recordings_root(&app);
        let prefs = super::recording_preferences::load_recording_preferences(&app).await.unwrap_or_default();
        super::project_settings::resolve_enhance(&root, &queued_folder, &prefs, language, provider, model)
    };
    let language = resolved.language;
    let provider = Some(resolved.provider);
    let model = Some(resolved.model);
    let language_label = language.clone();
    let provider_name = provider.clone().unwrap_or_else(|| "whisper".to_string());
    let model_name = model.clone().unwrap_or_else(|| "configured".to_string());
```

Then update every `set_enhance_status(...)` call in that function (five of them: queued, cancelled-while-queued, running, done, failed) by inserting `language_label.as_deref(),` right after the `&model_name,` argument. Example, the first one:

```rust
    set_enhance_status(&app, &meeting_id, &queued_folder, &provider_name, &model_name, language_label.as_deref(), STATUS_QUEUED, None);
```

`language` itself is moved into `run_retranscription(...)`; `language_label` is the clone the status writes use after that point.

Replace `RetranscriptionStarted` (line 1063) with:

```rust
/// Response when retranscription is started: what the run uses after resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionStarted {
    pub meeting_id: String,
    pub message: String,
    pub provider: String,
    pub model: String,
    pub language: Option<String>,
}
```

In `start_retranscription_command`, right after the opening comment `// spec 4.1: a job that arrives while another runs waits in ENHANCE_QUEUE.`, add:

```rust
    // Resolve once here too so the caller's toast names what will run (the spawned job resolves
    // the same explicit values to the same result).
    let resolved = {
        let root = crate::audio::recording_preferences::recordings_root(&app);
        let prefs = crate::audio::recording_preferences::load_recording_preferences(&app).await.unwrap_or_default();
        super::project_settings::resolve_enhance(&root, Path::new(&meeting_folder_path), &prefs, language, provider, model)
    };
    let language = resolved.language.clone();
    let provider = Some(resolved.provider.clone());
    let model = Some(resolved.model.clone());
```

and replace the return at the end of the command with:

```rust
    Ok(RetranscriptionStarted {
        meeting_id,
        message: "Retranscription started".to_string(),
        provider: resolved.provider,
        model: resolved.model,
        language: resolved.language,
    })
```

- [ ] **Step 5: Run the tests and type check**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend/src-tauri && cargo test --lib audio::enhance_status audio::retranscription 2>&1 | tail -5 && cargo check 2>&1 | tail -3
```

Expected: all pass, `Finished`. `grep -rn "write_enhance_status\|set_enhance_status" src` must show only `enhance_status.rs` and `retranscription.rs`; if another caller appears, give it `None` for the language.

- [ ] **Step 6: Commit**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git add frontend/src-tauri/src/audio/enhance_status.rs frontend/src-tauri/src/audio/retranscription.rs
git commit -m "feat(enhance): resolve language and model from project.json then preferences; record the language"
```

---

### Task 4: `get_meeting_extras` returns the transcription object

**Files:**
- Modify: `frontend/src-tauri/src/audio/audio_cleanup.rs` (lines 167-185 and `mod tests`)

**Interfaces:**
- Produces: `MeetingExtras.transcription: Option<serde_json::Value>`; `pub fn extras_of(folder: &Path) -> MeetingExtras`. The frontend reads `extras.transcription.live.{provider,model,language}` and `extras.transcription.enhanced.{provider,model,language,status}`.

- [ ] **Step 1: Write the failing test**

Add inside `mod tests` of `audio_cleanup.rs`:

```rust
    #[test]
    fn extras_carry_the_transcription_object_when_present() {
        let dir = tempfile::tempdir().unwrap();
        assert!(extras_of(dir.path()).transcription.is_none(), "no metadata.json");
        std::fs::write(
            dir.path().join("metadata.json"),
            json!({ "transcription": { "live": { "provider": "parakeet", "model": "p-v3", "language": "auto" } } }).to_string(),
        )
        .unwrap();
        let extras = extras_of(dir.path());
        let t = extras.transcription.unwrap();
        assert_eq!(t["live"]["model"], "p-v3");
        assert_eq!(t["live"]["language"], "auto");
        assert!(extras.tags.is_empty());
        assert!(!extras.has_audio);
    }
```

- [ ] **Step 2: Run it, expect `extras_of` not found**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend/src-tauri && cargo test --lib audio::audio_cleanup 2>&1 | grep -E "error|passed" | head -3
```

Expected: `error[E0425]: cannot find function `extras_of``.

- [ ] **Step 3: Implement**

Replace lines 167-185 (`MeetingExtras` and `get_meeting_extras`) with:

```rust
#[derive(Debug, Serialize)]
pub struct MeetingExtras {
    pub tags: Vec<String>,
    pub audio_deleted_at: Option<String>,
    pub has_audio: bool,
    /// spec 2026-10-30 section 2: `transcription` of metadata.json (live and enhanced engines), null when absent
    pub transcription: Option<serde_json::Value>,
}

/// The extras of one meeting folder; the command adds the database lookup.
pub fn extras_of(folder: &Path) -> MeetingExtras {
    let meta = read_meta(folder);
    MeetingExtras {
        tags: super::search_index::metadata_tags(folder),
        audio_deleted_at: meta["audio_deleted_at"].as_str().map(str::to_string),
        has_audio: super::retranscription::find_audio_file(folder).is_ok(),
        transcription: meta.get("transcription").cloned(),
    }
}

#[tauri::command]
pub async fn get_meeting_extras<R: Runtime>(app: AppHandle<R>, meeting_id: String) -> Result<MeetingExtras, String> {
    let folder = folder_of(&app, &meeting_id).await?.map(PathBuf::from).filter(|p| p.is_dir());
    let Some(folder) = folder else {
        return Ok(MeetingExtras { tags: vec![], audio_deleted_at: None, has_audio: false, transcription: None });
    };
    Ok(extras_of(&folder))
}
```

- [ ] **Step 4: Run the tests**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend/src-tauri && cargo test --lib audio::audio_cleanup 2>&1 | tail -3
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git add frontend/src-tauri/src/audio/audio_cleanup.rs
git commit -m "feat(meeting): get_meeting_extras returns the transcription engines"
```

---

### Task 5: Engines refuse to delete the loaded model

**Files:**
- Modify: `frontend/src-tauri/src/whisper_engine/whisper_engine.rs` (line 908, `delete_model`)
- Modify: `frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs` (line 572, `delete_model`)

**Interfaces:**
- Consumes: `WhisperEngine::get_current_model() -> Option<String>` (line 424), `ParakeetEngine::get_current_model() -> Option<String>` (line 533). Both files already import `anyhow!`.
- Produces: the error text `Model in use: stop the recording or wait for the enhance job to finish`.

No unit test: both engines need a models directory to construct. `cargo check` plus the manual pass in Task 12 cover it.

- [ ] **Step 1: Whisper guard**

In `whisper_engine.rs`, replace:

```rust
    pub async fn delete_model(&self, model_name: &str) -> Result<String> {
        log::info!("Attempting to delete model: {}", model_name);
```

with:

```rust
    pub async fn delete_model(&self, model_name: &str) -> Result<String> {
        log::info!("Attempting to delete model: {}", model_name);
        // spec 2026-10-30 section 1: never pull the file from under a running recording or enhance job
        if self.get_current_model().await.as_deref() == Some(model_name) {
            return Err(anyhow!("Model in use: stop the recording or wait for the enhance job to finish"));
        }
```

- [ ] **Step 2: Parakeet guard**

In `parakeet_engine.rs`, replace:

```rust
    /// Delete a corrupted model
    pub async fn delete_model(&self, model_name: &str) -> Result<String> {
        log::info!("Attempting to delete Parakeet model: {}", model_name);
```

with:

```rust
    /// Delete a corrupted or downloaded model
    pub async fn delete_model(&self, model_name: &str) -> Result<String> {
        log::info!("Attempting to delete Parakeet model: {}", model_name);
        // spec 2026-10-30 section 1: never pull the files from under a running recording or enhance job
        if self.get_current_model().await.as_deref() == Some(model_name) {
            return Err(anyhow!("Model in use: stop the recording or wait for the enhance job to finish"));
        }
```

- [ ] **Step 3: Type check**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend/src-tauri && cargo check 2>&1 | tail -3
```

Expected: `Finished`.

- [ ] **Step 4: Commit**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git add frontend/src-tauri/src/whisper_engine/whisper_engine.rs frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs
git commit -m "fix(models): refuse to delete the model currently loaded"
```

---

### Task 6: Frontend libs: `languages.ts`, `models.ts`

**Files:**
- Create: `frontend/src/lib/languages.ts`
- Create: `frontend/src/lib/languages.test.ts`
- Create: `frontend/src/lib/models.ts`
- Modify: `frontend/src/components/ModelsSettings.tsx` (remove the local `LANGUAGES` const at lines 39-47, import it)

**Interfaces:**
- Produces (`languages.ts`, self-contained): `LANGUAGES`, `LANGUAGE_OPTIONS`, `languageName(code)`, `languageSettingLabel(value)`, `languageSuffix(language)`, `liveLabel(t)`, `enhancedLabel(status, t)`, `type TranscriptionInfo`.
- Produces (`models.ts`): `interface TranscriptionModel { provider: 'whisper' | 'parakeet'; model: string; label: string }`, `listReadyTranscriptionModels(): Promise<TranscriptionModel[]>`.

- [ ] **Step 1: Write the failing test**

Create `frontend/src/lib/languages.test.ts`:

```ts
// Run with `pnpm test` (Node's built-in runner, types stripped natively).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { enhancedLabel, languageSettingLabel, languageSuffix, liveLabel } from './languages.ts';

test('live row names the engine and says Unknown for old meetings', () => {
  assert.equal(liveLabel({ live: { provider: 'parakeet', model: 'parakeet-tdt-0.6b-v3-int8' } }), 'Parakeet parakeet-tdt-0.6b-v3-int8');
  assert.equal(liveLabel({ live: { provider: 'whisper', model: 'small', language: 'auto-translate' } }), 'Whisper small · to English');
  assert.equal(liveLabel({ live: { provider: 'whisper', model: 'small', language: 'auto' } }), 'Whisper small');
  assert.equal(liveLabel(null), 'Unknown');
  assert.equal(liveLabel({}), 'Unknown');
});

test('enhanced row joins status, model and forced language', () => {
  const done = { enhanced: { provider: 'whisper', model: 'large-v3', language: 'auto-translate', status: 'done' } };
  assert.equal(enhancedLabel('done', done), 'Done · Whisper large-v3 · to English');
  assert.equal(enhancedLabel('failed', { enhanced: { provider: 'whisper', model: 'large-v3', language: 'es' } }), 'Failed · Whisper large-v3 · Spanish');
  assert.equal(enhancedLabel('running', { enhanced: { provider: 'parakeet', model: 'p', language: 'auto' } }), 'Running · Parakeet p');
  assert.equal(enhancedLabel(undefined, null), 'Not run');
  assert.equal(enhancedLabel('done', null), 'Done');
});

test('setting labels and suffixes', () => {
  assert.equal(languageSettingLabel(null), 'Keep the language');
  assert.equal(languageSettingLabel('auto'), 'Keep the language');
  assert.equal(languageSettingLabel('auto-translate'), 'Translate to English');
  assert.equal(languageSettingLabel('es'), 'Always Spanish');
  assert.equal(languageSuffix('xx'), 'xx', 'unknown code shown as is');
  assert.equal(languageSuffix(null), '');
});
```

- [ ] **Step 2: Run it, expect a module-not-found failure**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend && pnpm test 2>&1 | tail -5
```

Expected: `Cannot find module '.../languages.ts'`.

- [ ] **Step 3: Write `languages.ts`**

Create `frontend/src/lib/languages.ts` (no imports: Node tests load it directly):

```ts
// Language settings shared by Settings, the per-project table, the enhance picker and the
// meeting info bubble (spec 2026-10-30). Values: 'auto' keeps the spoken language,
// 'auto-translate' makes Whisper translate to English, a code forces that language.

export const LANGUAGES: { code: string; label: string }[] = [
  { code: 'en', label: 'English' },
  { code: 'fr', label: 'French' },
  { code: 'de', label: 'German' },
  { code: 'es', label: 'Spanish' },
  { code: 'it', label: 'Italian' },
  { code: 'nl', label: 'Dutch' },
  { code: 'pt', label: 'Portuguese' },
];

/** English name of a code, the code itself when unknown. */
export function languageName(code: string): string {
  return LANGUAGES.find((l) => l.code === code)?.label ?? code;
}

/** Label of a language setting value as the selects show it. */
export function languageSettingLabel(value: string | null | undefined): string {
  if (!value || value === 'auto') return 'Keep the language';
  if (value === 'auto-translate') return 'Translate to English';
  return `Always ${languageName(value)}`;
}

/** Options of the language selects: the two modes, then every language. */
export const LANGUAGE_OPTIONS: { value: string; label: string }[] = [
  { value: 'auto', label: 'Keep the language' },
  { value: 'auto-translate', label: 'Translate to English' },
  ...LANGUAGES.map((l) => ({ value: l.code, label: `Always ${l.label}` })),
];

/** `transcription` of metadata.json, as get_meeting_extras returns it. */
export interface TranscriptionInfo {
  live?: { provider?: string; model?: string; language?: string | null } | null;
  enhanced?: { provider?: string; model?: string; language?: string | null; status?: string } | null;
}

function providerName(provider?: string): string {
  return provider ? provider.charAt(0).toUpperCase() + provider.slice(1) : '';
}

function modelLabel(engine?: { provider?: string; model?: string } | null): string {
  if (!engine?.model) return '';
  return [providerName(engine.provider), engine.model].filter(Boolean).join(' ');
}

/** Suffix for a forced language: "to English" for translate, the language name for a code, "" otherwise. */
export function languageSuffix(language?: string | null): string {
  if (!language || language === 'auto') return '';
  if (language === 'auto-translate') return 'to English';
  return languageName(language);
}

/** Live row of the info bubble; "Unknown" for meetings recorded before the key existed. */
export function liveLabel(t?: TranscriptionInfo | null): string {
  const parts = [modelLabel(t?.live), languageSuffix(t?.live?.language)].filter(Boolean);
  return parts.length ? parts.join(' · ') : 'Unknown';
}

const ENHANCE_STATUS: Record<string, string> = { done: 'Done', queued: 'Queued', running: 'Running', failed: 'Failed' };

/** Enhanced row of the info bubble: status word, then the engine, then a forced language. */
export function enhancedLabel(status?: string, t?: TranscriptionInfo | null): string {
  const word = (status && ENHANCE_STATUS[status]) || 'Not run';
  return [word, modelLabel(t?.enhanced), languageSuffix(t?.enhanced?.language)].filter(Boolean).join(' · ');
}
```

- [ ] **Step 4: Run the tests**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend && pnpm test 2>&1 | tail -5
```

Expected: all tests pass (`fail 0`).

- [ ] **Step 5: Write `models.ts`**

Create `frontend/src/lib/models.ts`:

```ts
import { invoke } from '@tauri-apps/api/core';

/** A downloaded transcription model, as the enhance pickers list them. */
export interface TranscriptionModel { provider: 'whisper' | 'parakeet'; model: string; label: string }

// Whisper and Parakeet report 'Available' | 'Missing' | { Downloading } | { Error } | { Corrupted }.
const ready = (status: unknown) => status === 'Available';

/** Downloaded Whisper and Parakeet models, Parakeet first like the Settings table. */
export async function listReadyTranscriptionModels(): Promise<TranscriptionModel[]> {
  const [whisper, parakeet] = await Promise.all([
    invoke<{ name: string; status: unknown }[]>('whisper_get_available_models').catch(() => []),
    invoke<{ name: string; status: unknown }[]>('parakeet_get_available_models').catch(() => []),
  ]);
  return [
    ...parakeet.filter((m) => ready(m.status)).map((m): TranscriptionModel => ({ provider: 'parakeet', model: m.name, label: `Parakeet ${m.name}` })),
    ...whisper.filter((m) => ready(m.status)).map((m): TranscriptionModel => ({ provider: 'whisper', model: m.name, label: `Whisper ${m.name}` })),
  ];
}
```

- [ ] **Step 6: Use the shared list in `ModelsSettings.tsx`**

Delete the local constant (lines 39-47):

```ts
const LANGUAGES = [
  { code: 'en', label: 'English' },
  ...
  { code: 'pt', label: 'Portuguese' },
];
```

and add to the imports:

```ts
import { LANGUAGES } from '@/lib/languages';
```

- [ ] **Step 7: Type check and lint**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend && pnpm exec tsc --noEmit && pnpm lint 2>&1 | tail -3
```

Expected: no errors.

- [ ] **Step 8: Commit**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git add frontend/src/lib/languages.ts frontend/src/lib/languages.test.ts frontend/src/lib/models.ts frontend/src/components/ModelsSettings.tsx
git commit -m "feat(ui): shared language options and info bubble labels; ready model listing"
```

---

### Task 7: Info bubble shows the engines

**Files:**
- Modify: `frontend/src/components/MeetingStatusTags.tsx` (whole file)
- Modify: `frontend/src/components/MeetingDetails/TranscriptPanel.tsx` (lines 84-98 and 153-158)

**Interfaces:**
- Consumes: `liveLabel`, `enhancedLabel`, `TranscriptionInfo` from Task 6; `MeetingExtras.transcription` from Task 4.
- Produces: `MeetingInfoButton` prop `transcription?: TranscriptionInfo | null`.

- [ ] **Step 1: Rewrite `MeetingStatusTags.tsx`**

```tsx
import { Info } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { enhancedLabel, liveLabel, type TranscriptionInfo } from '@/lib/languages';

const TAGGING: Record<string, string> = { done: 'Done', running: 'Running', failed: 'Failed', cancelled: 'Cancelled' };

/** Info icon on the meeting page; hover shows the live and enhance engines, the statuses and the tags. */
export function MeetingInfoButton({
  tags,
  enhanceStatus,
  taggingStatus,
  transcription,
}: {
  tags: string[];
  enhanceStatus?: string;
  taggingStatus?: string;
  transcription?: TranscriptionInfo | null;
}) {
  const statuses = [enhanceStatus, taggingStatus];
  // the icon still signals work in progress and failures without opening the hover
  const tone = statuses.includes('failed') ? 'text-destructive' : statuses.includes('running') ? 'text-primary animate-pulse' : 'text-muted-foreground';
  const rows: [string, string][] = [
    ['Live', liveLabel(transcription)],
    ['Enhanced', enhancedLabel(enhanceStatus, transcription)],
    ['Title and tags', (taggingStatus && TAGGING[taggingStatus]) || 'Not run'],
  ];
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button type="button" aria-label="Meeting info" className={`flex h-8 w-8 items-center justify-center rounded-md hover:bg-accent/60 ${tone}`}>
          <Info className="h-4 w-4" />
        </button>
      </TooltipTrigger>
      <TooltipContent side="bottom" align="end" className="w-72 space-y-2 border border-border bg-popover p-3 text-popover-foreground">
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
          {rows.map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-muted-foreground">{k}</dt>
              <dd className={`break-words ${v.startsWith('Failed') ? 'text-destructive' : ''}`}>{v}</dd>
            </div>
          ))}
        </dl>
        <div className="flex flex-wrap gap-1 border-t border-border pt-2">
          {tags.length ? tags.map((tag) => <span key={tag} className="rounded border border-border px-1.5">{tag}</span>) : <span className="text-muted-foreground">No tags</span>}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}
```

- [ ] **Step 2: Load and pass the transcription in `TranscriptPanel.tsx`**

Add to the imports:

```ts
import type { TranscriptionInfo } from '@/lib/languages';
```

Replace lines 86-98 (the `extras` state and its effect) with:

```tsx
  type Extras = { tags: string[]; audio_deleted_at: string | null; has_audio: boolean; transcription?: TranscriptionInfo | null };
  const [extras, setExtras] = useState<Extras | null>(null);
  useEffect(() => {
    if (!meetingId) { setExtras(null); return; }
    const load = () => invoke<Extras>('get_meeting_extras', { meetingId }).then(setExtras).catch(() => setExtras(null));
    load();
    window.addEventListener(MEETING_EXTRAS_CHANGED, load);
    // tags change with tagging; the engines and language change when an enhance job moves
    const unlistens: (() => void)[] = [];
    let cancelled = false;
    for (const name of ['tagging-status-changed', 'enhance-status-changed']) {
      listen<{ meeting_id: string }>(name, (e) => { if (e.payload.meeting_id === meetingId) load(); })
        .then((fn) => { if (cancelled) fn(); else unlistens.push(fn); });
    }
    return () => { cancelled = true; unlistens.forEach((fn) => fn()); window.removeEventListener(MEETING_EXTRAS_CHANGED, load); };
  }, [meetingId]);
```

Replace the `<MeetingInfoButton ... />` usage (around line 154) with:

```tsx
          <MeetingInfoButton
            tags={extras?.tags ?? []}
            enhanceStatus={status?.enhanceStatus}
            taggingStatus={status?.taggingStatus}
            transcription={extras?.transcription}
          />
```

- [ ] **Step 3: Type check and lint**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend && pnpm exec tsc --noEmit && pnpm lint 2>&1 | tail -3
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git add frontend/src/components/MeetingStatusTags.tsx frontend/src/components/MeetingDetails/TranscriptPanel.tsx
git commit -m "feat(ui): info bubble names the live and enhance engines and a forced language"
```

---

### Task 8: Delete button in the models table

**Files:**
- Modify: `frontend/src/components/ModelsSettings.tsx` (imports; `DownloadButton` lines 98-125; `ModelTable` lines 136-167; the two `<ModelTable>` usages)

**Interfaces:**
- Consumes: Tauri commands `whisper_delete_corrupted_model`, `parakeet_delete_corrupted_model`, `builtin_ai_delete_model` (all take `modelName`); `ConfirmationModal` from `@/components/ConfirmationModel/confirmation-modal`.
- Produces: `ModelTable` prop `inUse?: (r: ModelRow) => string | undefined` (a reason disables the delete button).

- [ ] **Step 1: Imports and the delete command map**

Add `Trash2` to the lucide import and import the modal:

```ts
import { Check, Download, Loader2, Trash2 } from 'lucide-react';
import { ConfirmationModal } from '@/components/ConfirmationModel/confirmation-modal';
```

After `CANCEL_CMD`, add:

```ts
const DELETE_CMD: Record<Engine, string> = { whisper: 'whisper_delete_corrupted_model', parakeet: 'parakeet_delete_corrupted_model', builtin: 'builtin_ai_delete_model' };
```

- [ ] **Step 2: Replace `DownloadButton`**

```tsx
/** One cell: download, progress (click cancels), or downloaded with a delete button. */
function DownloadButton({ row, inUse, onChanged }: { row: ModelRow; inUse?: string; onChanged: () => void }) {
  const { state, engine, name } = row;
  const [confirm, setConfirm] = useState(false);
  if (state.kind === 'ready') {
    const remove = async () => {
      setConfirm(false);
      try {
        await invoke(DELETE_CMD[engine], { modelName: name });
        toast.success(`${row.label} deleted`);
      } catch (e) {
        toast.error('Could not delete the model', { description: String(e) });
      } finally {
        onChanged();
      }
    };
    return (
      <span className="inline-flex h-7 items-center gap-1 px-2 text-xs text-muted-foreground">
        <Check className="h-3.5 w-3.5" />Downloaded
        <button
          type="button"
          aria-label={`Delete ${row.label}`}
          title={inUse ?? 'Delete the model file'}
          disabled={!!inUse}
          className="ml-1 rounded p-0.5 hover:text-destructive disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:text-muted-foreground"
          onClick={() => setConfirm(true)}
        >
          <Trash2 className="h-3.5 w-3.5" />
        </button>
        <ConfirmationModal
          isOpen={confirm}
          text={`Delete ${row.label} (${formatSize(row.sizeMb)})? You can download it again later.`}
          onConfirm={remove}
          onCancel={() => setConfirm(false)}
        />
      </span>
    );
  }
  if (state.kind === 'downloading') {
    return (
      <Button size="sm" variant="ghost" className="h-7 w-[104px] text-xs" title="Cancel download"
        onClick={() => invoke(CANCEL_CMD[engine], { modelName: name }).catch((e) => toast.error(String(e))).finally(onChanged)}>
        <Loader2 className="animate-spin" />{Math.round(state.progress)}%
      </Button>
    );
  }
  return (
    <Button size="sm" variant="outline" className="h-7 w-[104px] text-xs"
      onClick={() => {
        // fire and forget: progress arrives through the download events
        invoke(DOWNLOAD_CMD[engine], { modelName: name }).catch((e) => { toast.error(String(e)); onChanged(); });
      }}>
      <Download />{state.kind === 'broken' ? 'Retry' : formatSize(row.sizeMb)}
    </Button>
  );
}
```

- [ ] **Step 3: Pass the in-use reason through `ModelTable`**

Change the `ModelTable` signature to:

```tsx
function ModelTable({ rows, columns, inUse, onChanged }: {
  rows: ModelRow[];
  columns: { label: string; checked: (r: ModelRow) => boolean; pick: (r: ModelRow) => void }[];
  inUse?: (r: ModelRow) => string | undefined;
  onChanged: () => void;
}) {
```

and the cell `<td className="py-1.5"><DownloadButton row={r} onChanged={onChanged} /></td>` to:

```tsx
            <td className="py-1.5"><DownloadButton row={r} inUse={inUse?.(r)} onChanged={onChanged} /></td>
```

- [ ] **Step 4: Define the rules and pass them**

In `ModelsSettings`, after the line `const readyBuiltin = builtinRows.filter((r) => r.state.kind === 'ready');`, add:

```tsx
  // spec 2026-10-30 section 1: a picked model cannot be deleted; pick another one first
  const IN_USE = 'Pick another model first';
  const transcriptionInUse = (r: ModelRow) => {
    const live = r.name === transcriptModelConfig.model && (r.engine === 'parakeet') === (transcriptModelConfig.provider === 'parakeet');
    const enhance = r.engine === prefs.enhance_provider && r.name === prefs.enhance_model;
    return live || enhance ? IN_USE : undefined;
  };
  const builtinInUse = (r: ModelRow) => {
    const summaryPick = !isExternal && r.name === summary.model;
    const askPick = prefs.ask_provider === 'builtin-ai' && r.name === prefs.ask_model;
    return summaryPick || askPick ? IN_USE : undefined;
  };
```

Add `inUse={transcriptionInUse}` to the transcription `<ModelTable rows={transcriptionRows} ...>` and `inUse={builtinInUse}` to the summarization `<ModelTable rows={builtinRows} ...>`.

- [ ] **Step 5: Type check and lint**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend && pnpm exec tsc --noEmit && pnpm lint 2>&1 | tail -3
```

Expected: no errors.

- [ ] **Step 6: Commit**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git add frontend/src/components/ModelsSettings.tsx
git commit -m "feat(settings): delete a downloaded model, except the picked ones"
```

---

### Task 9: Per-project table in Settings

**Files:**
- Create: `frontend/src/components/ProjectOverrides.tsx`
- Modify: `frontend/src/components/ModelsSettings.tsx` (import; a `SettingRow` after the transcription `<ModelTable>`)

**Interfaces:**
- Consumes: Tauri `list_projects_command() -> string[]`, `get_project_settings({ project }) -> ProjectSettings`, `set_project_settings({ project, settings })` from Task 1; `LANGUAGE_OPTIONS`, `languageSettingLabel` from Task 6; `TranscriptionModel` from Task 6; `INBOX` from `@/lib/projects`.
- Produces: `ProjectOverrides({ models, globalLanguage, globalProvider, globalModel })`; `interface ProjectSettings { language: string | null; enhance_provider: string | null; enhance_model: string | null }`.

- [ ] **Step 1: Write the component**

Create `frontend/src/components/ProjectOverrides.tsx`:

```tsx
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { INBOX } from '@/lib/projects';
import { LANGUAGE_OPTIONS, languageSettingLabel } from '@/lib/languages';
import type { TranscriptionModel } from '@/lib/models';

/** `<root>/<project>/project.json`; null means the global setting (spec 2026-10-30 section 3.1). */
export interface ProjectSettings { language: string | null; enhance_provider: string | null; enhance_model: string | null }

const EMPTY: ProjectSettings = { language: null, enhance_provider: null, enhance_model: null };
const DEFAULT = '__default__';
const key = (provider: string, model: string) => `${provider}:${model}`;
const providerName = (p: string) => p.charAt(0).toUpperCase() + p.slice(1);

/** One row per project; a change writes project.json at once. `models` are the downloaded transcription models. */
export function ProjectOverrides({ models, globalLanguage, globalProvider, globalModel }: {
  models: TranscriptionModel[];
  globalLanguage: string;
  globalProvider: string;
  globalModel: string;
}) {
  const [projects, setProjects] = useState<string[]>([]);
  const [settings, setSettings] = useState<Record<string, ProjectSettings>>({});

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const list = await invoke<string[]>('list_projects_command').catch(() => [INBOX]);
      const entries = await Promise.all(list.map(async (p) => [p, await invoke<ProjectSettings>('get_project_settings', { project: p }).catch(() => EMPTY)] as const));
      if (cancelled) return;
      setProjects(list);
      setSettings(Object.fromEntries(entries));
    })();
    return () => { cancelled = true; };
  }, []);

  const save = async (project: string, next: ProjectSettings) => {
    const prev = settings[project] ?? EMPTY;
    setSettings((s) => ({ ...s, [project]: next }));
    try {
      await invoke('set_project_settings', { project, settings: next });
    } catch (e) {
      setSettings((s) => ({ ...s, [project]: prev }));
      toast.error('Could not save the project settings', { description: String(e) });
    }
  };

  if (!projects.length) return null;
  return (
    <table className="w-full text-[13px]">
      <thead>
        <tr className="text-xs text-muted-foreground">
          <th className="py-2 text-left font-normal">Project</th>
          <th className="px-2 py-2 text-left font-normal">Language</th>
          <th className="px-2 py-2 text-left font-normal">Enhance model</th>
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {projects.map((p) => {
          const s = settings[p] ?? EMPTY;
          const modelValue = s.enhance_provider && s.enhance_model ? key(s.enhance_provider, s.enhance_model) : DEFAULT;
          const known = modelValue === DEFAULT || models.some((m) => key(m.provider, m.model) === modelValue);
          return (
            <tr key={p}>
              <td className="py-1.5">{p === INBOX ? 'Inbox' : p}</td>
              <td className="px-2 py-1.5">
                <Select value={s.language ?? DEFAULT} onValueChange={(v) => save(p, { ...s, language: v === DEFAULT ? null : v })}>
                  <SelectTrigger aria-label={`Language for ${p}`} className="h-8 w-52 text-[13px]"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value={DEFAULT}>Default ({languageSettingLabel(globalLanguage)})</SelectItem>
                    {LANGUAGE_OPTIONS.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}
                  </SelectContent>
                </Select>
              </td>
              <td className="px-2 py-1.5">
                <Select
                  value={modelValue}
                  onValueChange={(v) => {
                    const m = models.find((x) => key(x.provider, x.model) === v);
                    save(p, { ...s, enhance_provider: m?.provider ?? null, enhance_model: m?.model ?? null });
                  }}
                >
                  <SelectTrigger aria-label={`Enhance model for ${p}`} className="h-8 w-52 text-[13px]"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value={DEFAULT}>Default ({providerName(globalProvider)} {globalModel})</SelectItem>
                    {models.map((m) => <SelectItem key={key(m.provider, m.model)} value={key(m.provider, m.model)}>{m.label}</SelectItem>)}
                    {!known && <SelectItem value={modelValue} disabled>{modelValue.replace(':', ' ')} (not downloaded)</SelectItem>}
                  </SelectContent>
                </Select>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
```

- [ ] **Step 2: Mount it in `ModelsSettings.tsx`**

Add the imports:

```ts
import { ProjectOverrides } from '@/components/ProjectOverrides';
import type { TranscriptionModel } from '@/lib/models';
```

After the line `const readyBuiltin = builtinRows.filter((r) => r.state.kind === 'ready');`, add:

```tsx
  const readyTranscription: TranscriptionModel[] = transcriptionRows
    .filter((r) => r.state.kind === 'ready' && r.engine !== 'builtin')
    .map((r) => ({ provider: r.engine as 'whisper' | 'parakeet', model: r.name, label: r.label }));
```

Inside `<SettingsGroup title="Audio transcription">`, right after the closing `/>` of the transcription `<ModelTable ... />`, add:

```tsx
        <SettingRow
          label="Per project"
          hint="Language applies live and to the enhance run. Live translation needs Whisper as the Live model."
        >
          <ProjectOverrides models={readyTranscription} globalLanguage={selectedLanguage} globalProvider={prefs.enhance_provider} globalModel={prefs.enhance_model} />
        </SettingRow>
```

- [ ] **Step 3: Type check and lint**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend && pnpm exec tsc --noEmit && pnpm lint 2>&1 | tail -3
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git add frontend/src/components/ProjectOverrides.tsx frontend/src/components/ModelsSettings.tsx
git commit -m "feat(settings): per-project language and enhance model written to project.json"
```

---

### Task 10: Enhance override and the "Enhance with..." dialog

**Files:**
- Modify: `frontend/src/lib/autoEnhance.ts` (whole file)
- Create: `frontend/src/components/MeetingDetails/EnhanceWithDialog.tsx`
- Modify: `frontend/src/components/MeetingDetails/TranscriptButtonGroup.tsx` (imports, state, menu, modal area)

**Interfaces:**
- Consumes: `start_retranscription_command` returning `{ meeting_id, message, provider, model, language }` (Task 3); `resolve_enhance_settings({ meetingFolderPath }) -> { language, provider, model }` (Task 1); `listReadyTranscriptionModels`, `LANGUAGE_OPTIONS`, `languageSettingLabel`, `languageSuffix` (Task 6).
- Produces: `startEnhance(meetingId, folderPath, override?: EnhanceOverride)`; `EnhanceWithDialog({ open, onOpenChange, meetingId, folderPath })`.

- [ ] **Step 1: Rewrite `autoEnhance.ts`**

```ts
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import type { RecordingPreferences } from '@/components/RecordingSettings';
import { languageSuffix } from '@/lib/languages';

/** One-off choices for an enhance run; null or absent means "resolve" (project.json, then Settings). */
export interface EnhanceOverride { language?: string | null; provider?: string | null; model?: string | null }

interface Started { meeting_id: string; message: string; provider: string; model: string; language: string | null }

/**
 * spec 4.1: start the enhance (full-file re-transcription) job right after a
 * recording is saved, when the `auto_enhance` preference is on. Fire and
 * forget: progress and completion arrive as `retranscription-progress`,
 * `retranscription-complete` and `enhance-status-changed` events.
 */
export async function startAutoEnhanceIfEnabled(
  meetingId: string,
  folderPath: string | null,
): Promise<boolean> {
  if (!folderPath) {
    console.warn('[autoEnhance] no folder path for meeting, skipping', meetingId);
    return false;
  }

  let prefs: RecordingPreferences;
  try {
    prefs = await invoke<RecordingPreferences>('get_recording_preferences');
  } catch (error) {
    console.warn('[autoEnhance] could not read preferences, skipping:', error);
    return false;
  }
  // spec 4.1: no audio file is written when auto_save is off, so there is nothing to enhance
  if (!prefs.auto_enhance || !prefs.auto_save) return false;
  return startEnhance(meetingId, folderPath);
}

/**
 * Start the enhance job. Fields left null are resolved by Rust: the project's project.json,
 * then Settings (spec 2026-10-30 section 3.4). The toast names what actually runs.
 */
export async function startEnhance(
  meetingId: string,
  folderPath: string,
  override: EnhanceOverride = {},
): Promise<boolean> {
  try {
    const started = await invoke<Started>('start_retranscription_command', {
      meetingId,
      meetingFolderPath: folderPath,
      language: override.language ?? null,
      model: override.model ?? null,
      provider: override.provider ?? null,
    });
    const what = [`${started.provider} ${started.model}`, languageSuffix(started.language)].filter(Boolean).join(' · ');
    toast.info('Enhancing transcript in the background', {
      description: `${what}. The transcript updates when it finishes.`,
    });
    return true;
  } catch (error) {
    console.error('[autoEnhance] failed to start:', error);
    toast.error('Enhance did not start', {
      description: error instanceof Error ? error.message : String(error),
    });
    return false;
  }
}
```

Check callers: `grep -rn "startEnhance(" frontend/src`. `TranscriptButtonGroup.tsx` calls `startEnhance(meetingId, meetingFolderPath)` and needs no change. Any caller passing `prefs` as the third argument must drop it.

- [ ] **Step 2: Create the dialog**

Create `frontend/src/components/MeetingDetails/EnhanceWithDialog.tsx`:

```tsx
"use client";

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogTitle } from '@/components/ui/dialog';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Button } from '@/components/ui/button';
import { LANGUAGE_OPTIONS, languageSettingLabel } from '@/lib/languages';
import { listReadyTranscriptionModels, type TranscriptionModel } from '@/lib/models';
import { startEnhance } from '@/lib/autoEnhance';

const DEFAULT = '__default__';
const key = (provider: string, model: string) => `${provider}:${model}`;
interface Resolved { language: string | null; provider: string; model: string }

/**
 * spec 2026-10-30 section 3.5: one-off language and model for this enhance run. A select left
 * on Default passes null, so Rust resolves it from project.json and Settings. Nothing persists.
 */
export function EnhanceWithDialog({ open, onOpenChange, meetingId, folderPath }: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  meetingId: string;
  folderPath: string;
}) {
  const [resolved, setResolved] = useState<Resolved | null>(null);
  const [models, setModels] = useState<TranscriptionModel[]>([]);
  const [language, setLanguage] = useState(DEFAULT);
  const [model, setModel] = useState(DEFAULT);

  useEffect(() => {
    if (!open) return;
    setLanguage(DEFAULT);
    setModel(DEFAULT);
    invoke<Resolved>('resolve_enhance_settings', { meetingFolderPath: folderPath }).then(setResolved).catch(() => setResolved(null));
    listReadyTranscriptionModels().then(setModels);
  }, [open, folderPath]);

  const run = async () => {
    const picked = models.find((m) => key(m.provider, m.model) === model);
    onOpenChange(false);
    await startEnhance(meetingId, folderPath, {
      language: language === DEFAULT ? null : language,
      provider: picked?.provider ?? null,
      model: picked?.model ?? null,
    });
  };

  const hint = resolved
    ? `Default: ${languageSettingLabel(resolved.language)} · ${resolved.provider} ${resolved.model}`
    : 'Default: the project settings, then Settings';

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[420px]">
        <DialogTitle>Enhance with...</DialogTitle>
        <DialogDescription>{hint}</DialogDescription>
        <div className="grid gap-3">
          <Select value={language} onValueChange={setLanguage}>
            <SelectTrigger aria-label="Language"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value={DEFAULT}>Default language</SelectItem>
              {LANGUAGE_OPTIONS.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}
            </SelectContent>
          </Select>
          <Select value={model} onValueChange={setModel}>
            <SelectTrigger aria-label="Model"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value={DEFAULT}>Default model</SelectItem>
              {models.map((m) => <SelectItem key={key(m.provider, m.model)} value={key(m.provider, m.model)}>{m.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button onClick={run}>Run</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

- [ ] **Step 3: Add the menu item in `TranscriptButtonGroup.tsx`**

Imports: add `SlidersHorizontal` to the lucide import and:

```ts
import { EnhanceWithDialog } from './EnhanceWithDialog';
```

State, next to `const [showDeleteAudio, setShowDeleteAudio] = useState(false);`:

```ts
  const [showEnhanceWith, setShowEnhanceWith] = useState(false);
```

In the `<DropdownMenuContent>`, right after the Enhance `<DropdownMenuItem>`, add:

```tsx
              <DropdownMenuItem
                disabled={audioDeleted}
                title={audioGone}
                // deferred: the menu must finish closing before a Radix dialog takes focus
                onSelect={() => setTimeout(() => setShowEnhanceWith(true), 0)}
              >
                <SlidersHorizontal />
                Enhance with...
              </DropdownMenuItem>
```

Next to the `<ConfirmationModal ... />` at the end of the fragment, add:

```tsx
          <EnhanceWithDialog open={showEnhanceWith} onOpenChange={setShowEnhanceWith} meetingId={meetingId} folderPath={meetingFolderPath} />
```

- [ ] **Step 4: Type check and lint**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend && pnpm exec tsc --noEmit && pnpm lint 2>&1 | tail -3
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git add frontend/src/lib/autoEnhance.ts frontend/src/components/MeetingDetails/EnhanceWithDialog.tsx frontend/src/components/MeetingDetails/TranscriptButtonGroup.tsx
git commit -m "feat(meeting): Enhance with... one-off language and model; Enhance resolves project then Settings"
```

---

### Task 11: Contract notes and CLAUDE.md

**Files:**
- Modify: `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md` (sections 5.1 and 5.2)
- Modify: `docs/superpowers/specs/2026-10-30-model-management-and-project-overrides-design.md` (section 2 and 6: the helper file is `lib/languages.ts`)
- Modify: `CLAUDE.md` ("This fork" section)

- [ ] **Step 1: Main spec section 5**

In section 5.1, after `Legacy flat meeting folders at the root are read as `_inbox`.`, add:

```markdown
A project folder may hold an optional `project.json` with `language`, `enhance_provider` and `enhance_model`, all optional ([2026-10-30 spec](2026-10-30-model-management-and-project-overrides-design.md)).
```

In section 5.2, in the paragraph after the JSON block, append:

```markdown
`transcription.live.language` and `transcription.enhanced.language` are optional (2026-10-30 spec); absent means the global setting of the time applied.
```

- [ ] **Step 2: New spec: file name**

In `2026-10-30-model-management-and-project-overrides-design.md`, replace `src/lib/transcriptionLabel.ts` with `src/lib/languages.ts` in section 2 and `transcriptionLabel.test.ts` with `languages.test.ts` in section 6. Add to section 2, after the helper bullet: `The file also holds the shared language list and select options, so it stays import-free for the Node test runner.`

- [ ] **Step 3: CLAUDE.md**

In the "This fork" list, after the live meeting ask bullet, add:

```markdown
- Project overrides ([spec](docs/superpowers/specs/2026-10-30-model-management-and-project-overrides-design.md)): `<root>/<project>/project.json` carries optional `language`, `enhance_provider`, `enhance_model`; `audio/project_settings.rs` resolves explicit value, then project, then preferences, for the enhance run and for the live transcript (`set_recording_language` in `lib.rs`, set at record start and on a live project change, cleared at stop). `metadata.json` records the language under `transcription.live` and `transcription.enhanced`. Settings > Models has the per-project table and a delete button per downloaded model; the meeting page has "Enhance with...".
```

- [ ] **Step 4: Commit**

```bash
cd /Users/lainealexandre/Code/Perso/noesis
git add docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md docs/superpowers/specs/2026-10-30-model-management-and-project-overrides-design.md CLAUDE.md
git commit -m "docs: project.json and transcription language in the contracts; CLAUDE.md pointer"
```

---

### Task 12: Full verification and manual pass

**Files:** none.

- [ ] **Step 1: Run everything**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend/src-tauri && cargo test --lib 2>&1 | tail -3
cd /Users/lainealexandre/Code/Perso/noesis/frontend && pnpm test 2>&1 | tail -3 && pnpm exec tsc --noEmit && pnpm lint 2>&1 | tail -3
```

Expected: all Rust tests pass, all Node tests pass, no type or lint errors.

- [ ] **Step 2: Manual pass in the app**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend && ./clean_run.sh
```

Check, in order:

1. Settings > Models: a downloaded model that is not the Live or Enhance pick shows a trash icon. Click, confirm, the row flips to the download button. The Live pick's trash is disabled with "Pick another model first". Download the model again.
2. Settings > Models > Per project: set a project to "Translate to English" and a different Enhance model. `cat "<root>/<project>/project.json"` shows the two keys. Set both back to Default: the file has no keys left (an empty object).
3. Record a short meeting in that project (set to translate, Whisper as Live model), speak a sentence in French: the live transcript is in English. Stop. `metadata.json` has `transcription.live.language: "auto-translate"`. Change the project during a recording and check the live text follows.
4. Open the meeting, Actions > Enhance with..., leave both selects on Default: the hint names the project's language and model. Pick "Always Spanish" and a model, Run: the toast names them. When it finishes, hover the info icon: Live, Enhanced (with the model and "Spanish"), Title and tags.
5. Jobs panel: Retry a finished enhance job; the toast names the same model as the first run.

- [ ] **Step 3: Record the result**

Write a short note of anything that failed in the manual pass as a follow-up bullet in the spec's section 7, commit, then hand over to `superpowers:finishing-a-development-branch`.

---

## Self-review

- Spec section 1 (delete): Tasks 5 and 8. Section 2 (bubble): Tasks 4, 6, 7. Section 3.1 and 3.2 (data, module, commands): Task 1. Section 3.3 (live): Task 2. Section 3.4 (enhance): Tasks 3 and 10. Section 3.5 (UI): Tasks 9 and 10. Section 4 (contracts): Task 11. Section 6 (tests): each task carries its tests; the engine guards and the dialog are covered by the manual pass in Task 12.
- Names used across tasks: `set_recording_language` (Task 2, used in Task 2 only), `resolve_language` and `resolve_enhance` (Task 1, used in Tasks 2 and 3), `write_enhance_status` six-argument form (Task 3), `extras_of` and `MeetingExtras.transcription` (Task 4, read in Task 7), `TranscriptionInfo`, `liveLabel`, `enhancedLabel`, `languageSuffix`, `languageSettingLabel`, `LANGUAGE_OPTIONS` (Task 6, used in Tasks 7, 9, 10), `TranscriptionModel` and `listReadyTranscriptionModels` (Task 6, used in Tasks 9 and 10), `startEnhance(meetingId, folderPath, override?)` (Task 10), `RetranscriptionStarted.{provider,model,language}` (Task 3, read in Task 10).
