# Phase 1: Auto-Enhance After Stop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a recording stops and its SQLite row is saved, the app starts the existing full-file re-transcription ("Enhance") with the configured model, queues back-to-back jobs, records status in `metadata.json`, shows a badge in the sidebar and a notification on completion.

**Architecture:** Reuse `audio/retranscription.rs` unchanged in what it does; wrap its entry point in a `tokio::sync::Mutex` queue and add status writes. Status lives in each meeting folder's `metadata.json` under `transcription.enhanced` (spec section 5.2) through a new small module `audio/enhance_status.rs`. The frontend triggers the job from `useRecordingStop.ts` right after the meeting row is saved, and the sidebar reads statuses through one new Tauri command plus one event.

**Tech Stack:** Rust (Tauri 2, tokio, serde_json, once_cell, tempfile for tests), TypeScript (Next.js 14, React 18, `@tauri-apps/api`, sonner toasts, shadcn `Switch` and `Select`).

Spec: `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md`, sections 4.1 and 5.2.

## Global Constraints

- Additive changes only. New behaviour goes in new modules; edits to existing files stay small and marked with a comment that names the spec section (`// spec 4.1`).
- `transcripts.json` and the v1 keys of `metadata.json` (`version`, `meeting_id`, `created_at`, `completed_at`, `duration_seconds`, `status`, `retranscribed_at`, `audio_file`, `transcript_file`) keep their current meaning. New data goes under `transcription.enhanced`.
- `audio.mp4` stays mono. Nothing in this phase touches the audio pipeline.
- Do not delete or restructure upstream features (summary, LLM providers, licensing, analytics).
- No em-dashes in any text, code comment or UI string.
- Rust tests run from `frontend/src-tauri` with `cargo test --lib <filter>`. The first build compiles whisper.cpp with Metal and takes 10 to 20 minutes; later runs are incremental.
- Frontend type check: `cd frontend && pnpm exec tsc --noEmit`. Lint: `pnpm run lint`. If `tsc` is missing, run `pnpm install` once.
- Commit after every task on the phase branch `phase-1/auto-enhance`, created from `main` after `docs/personal-recorder-design` is merged (or from that branch if it is not merged yet).

## File Structure

| File | Responsibility |
| --- | --- |
| `frontend/src-tauri/src/audio/recording_preferences.rs` (modify) | Four new preference fields with serde defaults so existing stores still load |
| `frontend/src-tauri/src/audio/enhance_status.rs` (create) | Read and write `transcription.enhanced` in `metadata.json`; `list_enhance_statuses` Tauri command |
| `frontend/src-tauri/src/audio/mod.rs` (modify) | Declare the new module |
| `frontend/src-tauri/src/audio/retranscription.rs` (modify) | Queue, status transitions, completion notification, remove the "already in progress" refusal |
| `frontend/src-tauri/src/lib.rs` (modify) | Register `list_enhance_statuses` |
| `frontend/src/components/RecordingSettings.tsx` (modify) | TS type for the new fields; "Enhance after recording" switch and model select |
| `frontend/src/lib/autoEnhance.ts` (create) | `startAutoEnhanceIfEnabled(meetingId, folderPath)` |
| `frontend/src/hooks/useRecordingStop.ts` (modify) | One call to `startAutoEnhanceIfEnabled` after the meeting is saved |
| `frontend/src/app/meeting-details/page.tsx` (modify) | Refetch transcripts when `retranscription-complete` arrives for the open meeting |
| `frontend/src/components/Sidebar/SidebarProvider.tsx` (modify) | `enhanceStatus` on meetings, fetched once and patched by event |
| `frontend/src/components/Sidebar/index.tsx` (modify) | Badge next to the meeting title |

---

### Task 1: Enhance preferences (Rust and TypeScript types)

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs:14-40`
- Modify: `frontend/src/components/RecordingSettings.tsx:11-31`

**Interfaces:**
- Produces: `RecordingPreferences { auto_enhance: bool, enhance_provider: String, enhance_model: String, enhance_language: Option<String>, ... }` in Rust, and the matching TS interface fields `auto_enhance: boolean; enhance_provider: 'whisper' | 'parakeet'; enhance_model: string; enhance_language: string | null;`. Tasks 5 and 6 read them through `get_recording_preferences`.

- [ ] **Step 1: Write the failing test**

Append to the end of `frontend/src-tauri/src/audio/recording_preferences.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_preferences_json_gets_enhance_defaults() {
        // A store written before this phase has none of the enhance fields.
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert!(p.auto_enhance);
        assert_eq!(p.enhance_provider, "whisper");
        assert_eq!(p.enhance_model, "large-v3");
        assert_eq!(p.enhance_language, None);
    }

    #[test]
    fn default_preferences_enable_enhance_with_large_v3() {
        let p = RecordingPreferences::default();
        assert!(p.auto_enhance);
        assert_eq!(p.enhance_model, "large-v3");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend/src-tauri && cargo test --lib recording_preferences`
Expected: compile error, `no field auto_enhance on type RecordingPreferences`.

- [ ] **Step 3: Add the fields and defaults**

In `recording_preferences.rs`, change the struct to:

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecordingPreferences {
    pub save_folder: PathBuf,
    pub auto_save: bool,
    pub file_format: String,
    #[serde(default)]
    pub preferred_mic_device: Option<String>,
    #[serde(default)]
    pub preferred_system_device: Option<String>,
    #[cfg(target_os = "macos")]
    #[serde(default)]
    pub system_audio_backend: Option<String>,
    // spec 4.1: run the enhance (full-file re-transcription) job automatically after a recording stops
    #[serde(default = "default_true")]
    pub auto_enhance: bool,
    /// "whisper" or "parakeet"
    #[serde(default = "default_enhance_provider")]
    pub enhance_provider: String,
    /// Model name as listed by `whisper_get_available_models` or `parakeet_get_available_models`
    #[serde(default = "default_enhance_model")]
    pub enhance_model: String,
    /// Language hint for the enhance job. None means auto-detect.
    #[serde(default)]
    pub enhance_language: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_enhance_provider() -> String {
    "whisper".to_string()
}

fn default_enhance_model() -> String {
    "large-v3".to_string()
}
```

And extend `impl Default for RecordingPreferences`:

```rust
impl Default for RecordingPreferences {
    fn default() -> Self {
        Self {
            save_folder: get_default_recordings_folder(),
            auto_save: true,
            file_format: "mp4".to_string(),
            preferred_mic_device: None,
            preferred_system_device: None,
            #[cfg(target_os = "macos")]
            system_audio_backend: Some("coreaudio".to_string()),
            auto_enhance: default_true(),
            enhance_provider: default_enhance_provider(),
            enhance_model: default_enhance_model(),
            enhance_language: None,
        }
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd frontend/src-tauri && cargo test --lib recording_preferences`
Expected: `test result: ok. 2 passed`.

- [ ] **Step 5: Mirror the type in TypeScript**

In `frontend/src/components/RecordingSettings.tsx`, change the interface and initial state:

```ts
export interface RecordingPreferences {
  save_folder: string;
  auto_save: boolean;
  file_format: string;
  preferred_mic_device: string | null;
  preferred_system_device: string | null;
  // spec 4.1
  auto_enhance: boolean;
  enhance_provider: 'whisper' | 'parakeet';
  enhance_model: string;
  enhance_language: string | null;
}
```

```ts
  const [preferences, setPreferences] = useState<RecordingPreferences>({
    save_folder: '',
    auto_save: true,
    file_format: 'mp4',
    preferred_mic_device: null,
    preferred_system_device: null,
    auto_enhance: true,
    enhance_provider: 'whisper',
    enhance_model: 'large-v3',
    enhance_language: null,
  });
```

- [ ] **Step 6: Type-check the frontend**

Run: `cd frontend && pnpm exec tsc --noEmit`
Expected: no errors.

- [ ] **Step 7: Commit**

```bash
git add frontend/src-tauri/src/audio/recording_preferences.rs frontend/src/components/RecordingSettings.tsx
git commit -m "feat(enhance): add auto-enhance preferences with defaults"
```

---

### Task 2: `enhance_status.rs` file helpers

**Files:**
- Create: `frontend/src-tauri/src/audio/enhance_status.rs`
- Modify: `frontend/src-tauri/src/audio/mod.rs:47` (after `pub mod retranscription;`)

**Interfaces:**
- Produces:
  - `pub const STATUS_QUEUED: &str = "queued"; STATUS_RUNNING = "running"; STATUS_DONE = "done"; STATUS_FAILED = "failed";`
  - `pub fn write_enhance_status(folder: &Path, provider: &str, model: &str, status: &str, error: Option<&str>) -> anyhow::Result<()>`
  - `pub fn read_enhance_status(folder: &Path) -> Option<String>`
  - Task 3 calls `write_enhance_status`; Task 4 calls `read_enhance_status`.

- [ ] **Step 1: Write the failing tests**

Create `frontend/src-tauri/src/audio/enhance_status.rs` with only the tests for now:

```rust
//! Enhance job status stored in a meeting folder's metadata.json under
//! `transcription.enhanced` (spec 5.2). File helpers plus one Tauri command
//! that lists the status of every meeting in the database.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    fn read_root(dir: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("metadata.json")).unwrap()).unwrap()
    }

    #[test]
    fn write_then_read_round_trips_and_preserves_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("metadata.json"),
            r#"{"version":"1.0","meeting_name":"Meeting 1","status":"completed"}"#,
        )
        .unwrap();

        write_enhance_status(dir.path(), "whisper", "large-v3", STATUS_QUEUED, None).unwrap();
        assert_eq!(read_enhance_status(dir.path()).as_deref(), Some("queued"));

        write_enhance_status(dir.path(), "whisper", "large-v3", STATUS_FAILED, Some("boom")).unwrap();
        let root = read_root(dir.path());
        assert_eq!(root["meeting_name"], "Meeting 1");
        assert_eq!(root["status"], "completed");
        assert_eq!(root["transcription"]["enhanced"]["status"], "failed");
        assert_eq!(root["transcription"]["enhanced"]["error"], "boom");
        assert_eq!(root["transcription"]["enhanced"]["model"], "large-v3");
        assert_eq!(root["transcription"]["enhanced"]["provider"], "whisper");
        assert!(root["transcription"]["enhanced"]["at"].is_string());
    }

    #[test]
    fn missing_metadata_reads_none_and_write_creates_it() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_enhance_status(dir.path()), None);
        write_enhance_status(dir.path(), "whisper", "large-v3", STATUS_RUNNING, None).unwrap();
        assert_eq!(read_enhance_status(dir.path()).as_deref(), Some("running"));
    }

    #[test]
    fn legacy_metadata_without_enhance_reads_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("metadata.json"),
            r#"{"version":"1.0","retranscribed_at":"2026-09-30T19:31:30Z"}"#,
        )
        .unwrap();
        assert_eq!(read_enhance_status(dir.path()), None);
    }
}
```

Add to `frontend/src-tauri/src/audio/mod.rs`, right after `pub mod retranscription;`:

```rust
// spec 4.1: enhance job status in metadata.json
pub mod enhance_status;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib enhance_status`
Expected: compile errors, `cannot find function write_enhance_status`, `STATUS_QUEUED`.

- [ ] **Step 3: Implement the helpers**

Insert above the `#[cfg(test)]` block:

```rust
pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnhanceStatusEntry {
    pub meeting_id: String,
    pub status: String,
}

/// Merge `transcription.enhanced = {provider, model, status, at, error}` into
/// metadata.json, creating the file when it is missing. Every other key is kept.
/// Writes go through a temp file and a rename so a crash never leaves half a file.
pub fn write_enhance_status(
    folder: &Path,
    provider: &str,
    model: &str,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    let path = folder.join("metadata.json");
    let mut root: serde_json::Value = if path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&path)?)?
    } else {
        serde_json::json!({})
    };
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("metadata.json root is not an object"))?;
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
            "status": status,
            "at": chrono::Utc::now().to_rfc3339(),
            "error": error,
        }),
    );
    let tmp = folder.join(".metadata.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&root)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// `transcription.enhanced.status`, or None when the folder has no metadata.json
/// or no enhance record (every folder recorded before this phase).
pub fn read_enhance_status(folder: &Path) -> Option<String> {
    let text = std::fs::read_to_string(folder.join("metadata.json")).ok()?;
    let root: serde_json::Value = serde_json::from_str(&text).ok()?;
    root.get("transcription")?
        .get("enhanced")?
        .get("status")?
        .as_str()
        .map(str::to_string)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend/src-tauri && cargo test --lib enhance_status`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/audio/enhance_status.rs frontend/src-tauri/src/audio/mod.rs
git commit -m "feat(enhance): metadata.json status helpers"
```

---

### Task 3: Queue, status transitions and notification in `retranscription.rs`

**Files:**
- Modify: `frontend/src-tauri/src/audio/retranscription.rs:1-17` (imports), `:89-138` (`start_retranscription`), `:781-800` (`start_retranscription_command`)

**Interfaces:**
- Consumes: `enhance_status::{write_enhance_status, STATUS_*}` from Task 2; `crate::notifications::commands::NotificationManagerState<R>` (type alias `Arc<RwLock<Option<NotificationManager<R>>>>`, already managed in `lib.rs:472`).
- Produces: event `enhance-status-changed` with payload `{ meeting_id: string, status: "queued" | "running" | "done" | "failed" }` (Task 7 listens to it); `retranscription-progress` now also emits `stage: "queued"`, `progress_percentage: 0` while waiting. `start_retranscription_command` no longer returns `Err("Retranscription already in progress")`; a second call queues.

- [ ] **Step 1: Write the failing test**

Append inside the existing `mod tests` at the bottom of `retranscription.rs`:

```rust
    #[tokio::test]
    async fn enhance_queue_serializes_jobs() {
        let first = ENHANCE_QUEUE.lock().await;
        assert!(
            ENHANCE_QUEUE.try_lock().is_err(),
            "a second job must wait while the first holds the slot"
        );
        drop(first);
        assert!(ENHANCE_QUEUE.try_lock().is_ok());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend/src-tauri && cargo test --lib retranscription::tests::enhance_queue`
Expected: compile error, `cannot find value ENHANCE_QUEUE`.

- [ ] **Step 3: Add imports and the queue**

Replace the import block at the top of `retranscription.rs` (lines 3 to 17) with:

```rust
use crate::audio::decoder::decode_audio_file;
use crate::audio::vad::get_speech_chunks_with_progress;
use super::common::{create_transcript_segments, split_segment_at_silence, write_transcripts_json};
use super::constants::AUDIO_EXTENSIONS;
use super::enhance_status::{self, STATUS_DONE, STATUS_FAILED, STATUS_QUEUED, STATUS_RUNNING};
use crate::config::{DEFAULT_WHISPER_MODEL, DEFAULT_PARAKEET_MODEL};
use crate::notifications::commands::NotificationManagerState;
use crate::parakeet_engine::ParakeetEngine;
use crate::state::AppState;
use crate::whisper_engine::WhisperEngine;
use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};
```

Right after `static RETRANSCRIPTION_CANCELLED: AtomicBool = AtomicBool::new(false);` add:

```rust
/// spec 4.1: enhance jobs run one at a time, in arrival order. A meeting that
/// stops while another is still enhancing waits here instead of being refused.
static ENHANCE_QUEUE: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd frontend/src-tauri && cargo test --lib retranscription::tests::enhance_queue`
Expected: `test result: ok. 1 passed`.

- [ ] **Step 5: Rewrite `start_retranscription` to queue and record status**

Replace the whole function `start_retranscription` (from the doc comment `/// Start retranscription of a meeting's audio` down to its closing brace, just before `/// Find audio file in meeting folder`) with:

```rust
/// Start retranscription of a meeting's audio.
/// spec 4.1: waits for the enhance queue, records status in metadata.json at
/// each transition, and notifies when done.
pub async fn start_retranscription<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionResult> {
    let folder = PathBuf::from(&meeting_folder_path);
    let provider_name = provider.clone().unwrap_or_else(|| "whisper".to_string());
    let model_name = model.clone().unwrap_or_else(|| "configured".to_string());

    set_enhance_status(&app, &meeting_id, &folder, &provider_name, &model_name, STATUS_QUEUED, None);
    if ENHANCE_QUEUE.try_lock().is_err() {
        emit_progress(&app, &meeting_id, "queued", 0, "Waiting for the current enhance job to finish...");
    }
    let _queue_slot = ENHANCE_QUEUE.lock().await;

    // Acquire guard - ensures the in-progress flag is cleared even on panic/early return
    let _guard = RetranscriptionGuard::acquire().map_err(|e| anyhow!(e))?;

    // Reset cancellation flag
    RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);
    set_enhance_status(&app, &meeting_id, &folder, &provider_name, &model_name, STATUS_RUNNING, None);

    let use_parakeet = provider.as_deref() == Some("parakeet");
    let result = run_retranscription(app.clone(), meeting_id.clone(), meeting_folder_path, language, model, provider).await;

    // Unload the engine after the batch job (success, failure, or cancellation)
    super::common::unload_engine_after_batch(use_parakeet).await;

    match &result {
        Ok(res) => {
            set_enhance_status(&app, &meeting_id, &folder, &provider_name, &model_name, STATUS_DONE, None);
            notify_enhance_complete(&app).await;
            let _ = app.emit(
                "retranscription-complete",
                serde_json::json!({
                    "meeting_id": res.meeting_id,
                    "segments_count": res.segments_count,
                    "duration_seconds": res.duration_seconds,
                    "language": res.language
                }),
            );
        }
        Err(e) => {
            set_enhance_status(&app, &meeting_id, &folder, &provider_name, &model_name, STATUS_FAILED, Some(&e.to_string()));
            let _ = app.emit(
                "retranscription-error",
                RetranscriptionError {
                    meeting_id: meeting_id.clone(),
                    error: e.to_string(),
                },
            );
        }
    }

    result
}

/// Write the status to metadata.json and tell the UI. Failures to write are
/// logged, never fatal: the transcript matters more than the badge.
fn set_enhance_status<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    folder: &Path,
    provider: &str,
    model: &str,
    status: &str,
    error: Option<&str>,
) {
    if let Err(e) = enhance_status::write_enhance_status(folder, provider, model, status, error) {
        warn!("Could not write enhance status '{}' for {}: {}", status, meeting_id, e);
    }
    let _ = app.emit(
        "enhance-status-changed",
        serde_json::json!({ "meeting_id": meeting_id, "status": status }),
    );
}

/// OS notification through the existing NotificationManager. Respects the
/// user's `show_transcription_complete` preference because the manager checks it.
async fn notify_enhance_complete<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<NotificationManagerState<R>>() {
        let guard = state.read().await;
        if let Some(manager) = guard.as_ref() {
            if let Err(e) = manager.show_transcription_complete(None).await {
                warn!("Enhance complete notification failed: {}", e);
            }
        }
    }
}
```

- [ ] **Step 6: Let the command queue instead of refusing**

In `start_retranscription_command`, delete these four lines:

```rust
    // Check if retranscription is already in progress (guard will be acquired in start_retranscription)
    if RETRANSCRIPTION_IN_PROGRESS.load(Ordering::SeqCst) {
        return Err("Retranscription already in progress".to_string());
    }
```

and replace them with:

```rust
    // spec 4.1: a job that arrives while another runs waits in ENHANCE_QUEUE.
```

Leave `cancel_retranscription_command` and `is_retranscription_in_progress_command` as they are. Cancel applies to the running job only; a queued job runs when its turn comes. That limit is acceptable for this phase.

- [ ] **Step 7: Build and run the module tests**

Run: `cd frontend/src-tauri && cargo test --lib retranscription`
Expected: all tests in the module pass (the four `create_transcript_segments` tests plus the queue test). No warnings about unused imports (`Path` is used by `set_enhance_status`).

- [ ] **Step 8: Commit**

```bash
git add frontend/src-tauri/src/audio/retranscription.rs
git commit -m "feat(enhance): queue jobs, record status in metadata.json, notify on completion"
```

---

### Task 4: `list_enhance_statuses` command

**Files:**
- Modify: `frontend/src-tauri/src/audio/enhance_status.rs` (add the command above the tests)
- Modify: `frontend/src-tauri/src/lib.rs:826` (register after `is_retranscription_in_progress_command`)

**Interfaces:**
- Consumes: `crate::database::repositories::meeting::MeetingsRepository::get_meetings(pool) -> Result<Vec<MeetingModel>, sqlx::Error>` where `MeetingModel { id: String, title: String, folder_path: Option<String>, .. }`; `AppState.db_manager.pool()`.
- Produces: Tauri command `list_enhance_statuses() -> Vec<EnhanceStatusEntry { meeting_id: String, status: String }>`. Meetings without a folder or without an enhance record are omitted. Task 7 calls it.

- [ ] **Step 1: Add the command**

In `enhance_status.rs`, below `read_enhance_status`:

```rust
/// Status of every meeting that has a folder and an enhance record. Meetings
/// recorded before this phase, or whose folder moved, are simply absent.
#[tauri::command]
pub async fn list_enhance_statuses(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<EnhanceStatusEntry>, String> {
    let pool = state.db_manager.pool();
    let meetings = crate::database::repositories::meeting::MeetingsRepository::get_meetings(pool)
        .await
        .map_err(|e| format!("Failed to list meetings: {}", e))?;
    Ok(meetings
        .into_iter()
        .filter_map(|m| {
            let folder = m.folder_path?;
            let status = read_enhance_status(Path::new(&folder))?;
            Some(EnhanceStatusEntry { meeting_id: m.id, status })
        })
        .collect())
}
```

- [ ] **Step 2: Register it**

In `frontend/src-tauri/src/lib.rs`, after the line `audio::retranscription::is_retranscription_in_progress_command,` add:

```rust
            audio::enhance_status::list_enhance_statuses,
```

- [ ] **Step 3: Verify it compiles**

Run: `cd frontend/src-tauri && cargo check`
Expected: `Finished` with no errors. If `MeetingsRepository` is not found, check `database/repositories/meeting.rs:7` for the struct name and `database/repositories/mod.rs` for the module list; both exist at the time of writing.

- [ ] **Step 4: Commit**

```bash
git add frontend/src-tauri/src/audio/enhance_status.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(enhance): list_enhance_statuses command"
```

---

### Task 5: Trigger the job after save

**Files:**
- Create: `frontend/src/lib/autoEnhance.ts`
- Modify: `frontend/src/hooks/useRecordingStop.ts:296-297` (after `await markMeetingAsSaved();`) and its imports

**Interfaces:**
- Consumes: Tauri commands `get_recording_preferences` (returns the TS `RecordingPreferences` from Task 1) and `start_retranscription_command({ meetingId, meetingFolderPath, language, model, provider })`.
- Produces: `startAutoEnhanceIfEnabled(meetingId: string, folderPath: string | null): Promise<boolean>`; resolves `true` when a job was started.

- [ ] **Step 1: Create the helper**

`frontend/src/lib/autoEnhance.ts`:

```ts
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import type { RecordingPreferences } from '@/components/RecordingSettings';

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
  if (!prefs.auto_enhance) return false;

  try {
    await invoke('start_retranscription_command', {
      meetingId,
      meetingFolderPath: folderPath,
      language: prefs.enhance_language,
      model: prefs.enhance_model,
      provider: prefs.enhance_provider,
    });
    toast.info('Enhancing transcript in the background', {
      description: `${prefs.enhance_provider} ${prefs.enhance_model}. The transcript updates when it finishes.`,
    });
    return true;
  } catch (error) {
    console.error('[autoEnhance] failed to start:', error);
    toast.error('Automatic enhance did not start', {
      description: error instanceof Error ? error.message : String(error),
    });
    return false;
  }
}
```

- [ ] **Step 2: Call it from the stop hook**

In `frontend/src/hooks/useRecordingStop.ts`, add the import next to the other `@/lib` imports:

```ts
import { startAutoEnhanceIfEnabled } from '@/lib/autoEnhance';
```

Then find this line inside `handleRecordingStop` (about line 297):

```ts
          await markMeetingAsSaved();
```

and add directly below it:

```ts
          // spec 4.1: enhance in the background. Never blocks navigation.
          void startAutoEnhanceIfEnabled(meetingId, folderPath);
```

`meetingId` and `folderPath` are already in scope at that point (`const meetingId = responseData.meeting_id;` and `const folderPath = sessionStorage.getItem('last_recording_folder_path');` a few lines above).

- [ ] **Step 3: Type-check**

Run: `cd frontend && pnpm exec tsc --noEmit`
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/autoEnhance.ts frontend/src/hooks/useRecordingStop.ts
git commit -m "feat(enhance): start enhance automatically after a recording is saved"
```

---

### Task 6: Refresh the open meeting when enhance completes

**Files:**
- Modify: `frontend/src/app/meeting-details/page.tsx:1-12` (imports) and after the `usePaginatedTranscripts` destructuring (about line 51)

**Interfaces:**
- Consumes: `refetch` from `usePaginatedTranscripts`, event `retranscription-complete` with `{ meeting_id: string }`.

Why: `RetranscribeDialog` refetches through its `onComplete` callback, but an automatic job has no dialog open. Without this, the page keeps showing the live transcript until it is reopened.

- [ ] **Step 1: Add the listener**

Add to the imports in `page.tsx`:

```ts
import { listen } from "@tauri-apps/api/event";
```

Right after the `usePaginatedTranscripts({ meetingId: meetingId || '' });` statement add:

```ts
  // spec 4.1: an automatic enhance has no dialog to refetch for us.
  useEffect(() => {
    if (!meetingId) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen<{ meeting_id: string }>('retranscription-complete', (event) => {
      if (event.payload.meeting_id === meetingId) {
        void refetch();
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [meetingId, refetch]);
```

- [ ] **Step 2: Type-check**

Run: `cd frontend && pnpm exec tsc --noEmit`
Expected: no errors. If `refetch` is not stable and the effect re-subscribes on every render, that is harmless here because the cleanup unlistens.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/app/meeting-details/page.tsx
git commit -m "feat(enhance): refetch transcripts when a background enhance completes"
```

---

### Task 7: Sidebar badge

**Files:**
- Modify: `frontend/src/components/Sidebar/SidebarProvider.tsx:19-22` (type), `:96-112` (`fetchMeetings`), plus one new effect
- Modify: `frontend/src/components/Sidebar/index.tsx:618` (after the title span)

**Interfaces:**
- Consumes: Tauri command `list_enhance_statuses` (Task 4) and event `enhance-status-changed` (Task 3).
- Produces: `CurrentMeeting.enhanceStatus?: string` on every meeting in the sidebar context.

- [ ] **Step 1: Extend the meeting type and fetch**

In `SidebarProvider.tsx` change the interface:

```ts
export interface CurrentMeeting {
  id: string;
  title: string;
  // spec 4.1: "queued" | "running" | "done" | "failed", absent for legacy meetings
  enhanceStatus?: string;
}
```

Replace the body of `fetchMeetings` so it merges statuses:

```ts
  const fetchMeetings = React.useCallback(async () => {
    if (serverAddress) {
      try {
        const meetings = await invoke('api_get_meetings') as Array<{ id: string, title: string }>;
        // spec 4.1: statuses are optional; a failure here must not hide the meeting list
        const statuses = await invoke<Array<{ meeting_id: string; status: string }>>('list_enhance_statuses')
          .catch((error) => { console.warn('list_enhance_statuses failed:', error); return []; });
        const statusById = new Map(statuses.map((s) => [s.meeting_id, s.status]));
        const transformedMeetings = meetings.map((meeting: any) => ({
          id: meeting.id,
          title: meeting.title,
          enhanceStatus: statusById.get(meeting.id),
        }));
        setMeetings(transformedMeetings);
        Analytics.trackBackendConnection(true);
      } catch (error) {
        console.error('Error fetching meetings:', error);
        setMeetings([]);
        Analytics.trackBackendConnection(false, error instanceof Error ? error.message : 'Unknown error');
      }
    }
  }, [serverAddress]);
```

Add a new effect below the existing `useEffect` blocks in the provider (before `const baseItems`):

```ts
  // spec 4.1: live badge updates while a job moves through queued, running, done, failed
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen<{ meeting_id: string; status: string }>('enhance-status-changed', (event) => {
      setMeetings((prev) =>
        prev.map((m) => (m.id === event.payload.meeting_id ? { ...m, enhanceStatus: event.payload.status } : m)),
      );
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
```

Make sure `listen` is imported: `import { listen } from '@tauri-apps/api/event';` (add it if the file does not already import it).

Also carry the status into the sidebar items so `index.tsx` can read it without a lookup. In `baseItems`:

```ts
        ...meetings.map(meeting => ({ id: meeting.id, title: meeting.title, type: 'file' as const, enhanceStatus: meeting.enhanceStatus }))
```

and extend `SidebarItem` in the same file:

```ts
export interface SidebarItem {
  id: string;
  title: string;
  type: 'folder' | 'file';
  children?: SidebarItem[];
  enhanceStatus?: string;
}
```

`index.tsx` has its own local `SidebarItem` interface at line 35; add `enhanceStatus?: string;` there too.

- [ ] **Step 2: Render the badge**

In `frontend/src/components/Sidebar/index.tsx`, above the component (after the imports) add:

```tsx
// spec 4.1: small status chip next to the meeting title. "done" shows nothing.
const ENHANCE_BADGE: Record<string, { label: string; className: string }> = {
  queued: { label: 'queued', className: 'bg-gray-100 text-gray-600' },
  running: { label: 'enhancing', className: 'bg-blue-50 text-blue-700' },
  failed: { label: 'enhance failed', className: 'bg-red-50 text-red-700' },
};

function EnhanceBadge({ status }: { status?: string }) {
  const badge = status ? ENHANCE_BADGE[status] : undefined;
  if (!badge) return null;
  return (
    <span className={`ml-2 flex-shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium ${badge.className}`}>
      {badge.label}
    </span>
  );
}
```

Then find the title span in the file-item branch:

```tsx
                <span className="flex-1 break-words">{item.title}</span>
```

and add directly after it:

```tsx
                {isMeetingItem && <EnhanceBadge status={item.enhanceStatus} />}
```

- [ ] **Step 3: Type-check and lint**

Run: `cd frontend && pnpm exec tsc --noEmit && pnpm run lint`
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/Sidebar/SidebarProvider.tsx frontend/src/components/Sidebar/index.tsx
git commit -m "feat(enhance): sidebar badge for queued, running and failed enhance jobs"
```

---

### Task 8: Settings UI

**Files:**
- Modify: `frontend/src/components/RecordingSettings.tsx` (imports, two handlers, one JSX block after the Auto Save toggle)

**Interfaces:**
- Consumes: `whisper_get_available_models` returning `RawModelInfo[]` (exported from `@/hooks/useTranscriptionModels`); `set_recording_preferences` through the existing `savePreferences`.

- [ ] **Step 1: Imports and model list**

Add to the imports:

```ts
import type { RawModelInfo } from '@/hooks/useTranscriptionModels';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
```

Inside the component, after the `useRecordingState` and `useConfig` lines, add:

```ts
  // spec 4.1: only downloaded Whisper models can be picked for the enhance job
  const [enhanceModels, setEnhanceModels] = useState<string[]>([]);
  useEffect(() => {
    invoke<RawModelInfo[]>('whisper_get_available_models')
      .then((models) => setEnhanceModels(models.filter((m) => m.status === 'Available').map((m) => m.name)))
      .catch((error) => console.error('Failed to load Whisper models:', error));
  }, []);

  const handleAutoEnhanceToggle = async (enabled: boolean) => {
    const next = { ...preferences, auto_enhance: enabled };
    setPreferences(next);
    await savePreferences(next);
  };

  const handleEnhanceModelChange = async (model: string) => {
    const next = { ...preferences, enhance_provider: 'whisper' as const, enhance_model: model };
    setPreferences(next);
    await savePreferences(next);
  };
```

- [ ] **Step 2: Add the JSX block**

Directly after the `{/* Auto Save Toggle */}` block (the `div` ending with `</Switch>` and `</div>`), add:

```tsx
      {/* spec 4.1: enhance after recording */}
      <div className="p-4 border rounded-lg space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex-1">
            <div className="font-medium">Enhance transcript after recording</div>
            <div className="text-sm text-gray-600">
              Re-transcribe the full recording with a slower, more accurate model as soon as it stops
            </div>
          </div>
          <Switch
            checked={preferences.auto_enhance}
            onCheckedChange={handleAutoEnhanceToggle}
            disabled={saving}
          />
        </div>
        {preferences.auto_enhance && (
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-sm text-gray-700">Model</span>
            <Select value={preferences.enhance_model} onValueChange={handleEnhanceModelChange} disabled={saving}>
              <SelectTrigger className="w-64">
                <SelectValue placeholder="Pick a downloaded Whisper model" />
              </SelectTrigger>
              <SelectContent>
                {enhanceModels.map((name) => (
                  <SelectItem key={name} value={name}>{name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            {enhanceModels.length === 0 && (
              <span className="text-xs text-amber-700">
                No Whisper model downloaded yet. Download one in Transcription settings.
              </span>
            )}
          </div>
        )}
      </div>
```

- [ ] **Step 3: Type-check and lint**

Run: `cd frontend && pnpm exec tsc --noEmit && pnpm run lint`
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/RecordingSettings.tsx
git commit -m "feat(enhance): settings switch and model picker for auto-enhance"
```

---

### Task 9: Manual acceptance on a real recording

**Files:** none. This task verifies the phase end to end in the running app.

- [ ] **Step 1: Run the app**

Run: `cd frontend && ./clean_run.sh`
Expected: app launches. Open Settings, Recording tab: the "Enhance transcript after recording" switch is on and the model select shows `large-v3`.

- [ ] **Step 2: Record a short test**

Start a recording, speak for about 60 seconds, stop. Expected within a few seconds of stopping:
- toast "Recording saved successfully!" then toast "Enhancing transcript in the background";
- the new meeting in the sidebar shows the `queued` then `enhancing` chip;
- `metadata.json` in the new folder under `~/Movies/meetily-recordings` contains `"transcription": {"enhanced": {"status": "running", ...}}` (check with `cat`).

- [ ] **Step 3: Wait for completion**

Expected within a minute or two for a 60 second clip on an Apple Silicon Mac:
- macOS notification "Transcription has been completed";
- the chip disappears (status `done`);
- the meeting details page, if open, refreshes its transcript without a reload;
- `metadata.json` shows `"status": "done"` and a fresh `retranscribed_at`.

- [ ] **Step 4: Queue test**

Start a second recording while the first job is still running (use a 10 minute recording for the first if needed), stop it. Expected: the second meeting shows `queued`, the first keeps `enhancing`, and the second starts when the first finishes. Opening the Enhance dialog on the queued meeting shows the message "Waiting for the current enhance job to finish...".

- [ ] **Step 5: Toggle off**

Turn the switch off in Settings, record 20 seconds, stop. Expected: no enhance toast, no chip, `metadata.json` has no `transcription` key.

- [ ] **Step 6: Audio saving off**

Turn "Save Audio Recordings" off, record 20 seconds, stop. Expected: no enhance toast, no chip, `metadata.json` has no `transcription` key. Turn it back on.

- [ ] **Step 7: Stop during an enhance**

Start a recording while an enhance job is running, stop it after 30 seconds. Expected: the running job still ends `done`; the new meeting queues and then enhances.

- [ ] **Step 8: Finish the branch**

Turn the switch back on. Then use the `superpowers:finishing-a-development-branch` skill to merge `phase-1/auto-enhance`.

---

## Self-review against the spec

- 4.1 preferences: Task 1. Trigger point in `useRecordingStop.ts`: Task 5. Queue replacing the refusal: Task 3. Status in `metadata.json`: Tasks 2 and 3. Sidebar badge: Task 7. Notification: Task 3. Model lifecycle unchanged: Task 3 keeps `unload_engine_after_batch`. Tray stop path: `RecordingPostProcessingProvider` calls the same `handleRecordingStop`, so Task 5 covers it with no extra code.
- 5.2: only `transcription.enhanced` is written in this phase; `transcript.md`, `speakers`, `project`, `title` and `ended_by` belong to later phases.
- Names used across tasks: `write_enhance_status`, `read_enhance_status`, `STATUS_*`, `EnhanceStatusEntry`, `list_enhance_statuses`, `enhance-status-changed`, `startAutoEnhanceIfEnabled`, `EnhanceBadge`, `enhanceStatus`. Each is defined in exactly one task and spelled the same everywhere.
- Known limit, accepted: cancel only affects the running job, not a queued one.

## Outcome (2026-10-27)

Implemented on branch `phase-1/auto-enhance` (15 commits). Every task review and the whole-branch review passed after one fix wave (skip when audio saving is off, keep Whisper loaded during a job, cancel targets the running meeting only, toast on failure) and one acceptance fix wave (silent recording ends done with zero segments, compact status dot). Manual acceptance passed on a real recording. Dev-build note: `tauri dev` inherits Microphone and Screen & System Audio Recording permission from the launching terminal.

### Follow-ups deferred from review (none block merge)

- : enhance_provider is a free String in Rust but a 'whisper' | 'parakeet' union in TS; the consumer (Task 5) should fall back on unknown values.
- : default_true is a generic helper name in recording_preferences.rs.
- : fixed temp filename .metadata.json.tmp; concurrent writers to one folder would clobber (jobs are serialized, latent only).
- : non-object `transcription` value replaced silently, no warn log.
- : no tests for malformed JSON / non-object root / non-object transcription.
- : temp file left behind if rename fails.
- : guard failure after the queue slot would leave status "queued" with no failed event (cannot happen today; only one guard acquirer).
- : user cancel is recorded as status "failed" (status union has no cancelled value); Task 7 badge should not read a cancel as an error.
- : cancel_retranscription_command takes no meeting id and always targets the running job; UI must not offer cancel on a queued meeting.
- : submitting the same meeting twice overwrites its status and can race the metadata write inside run_retranscription.
- : when no model is passed, metadata records the literal "configured" rather than the resolved model.
- : the queue test only proves the mutex blocks, not that start_retranscription takes it.
- : list_enhance_statuses does blocking std::fs reads inside an async command; wrap in spawn_blocking if the library grows to thousands.
- : no test for the command itself (needs AppState); helper is tested.
- : start toast omits enhance_language (cosmetic).
- : the void call runs before refetchMeetings; an extremely fast job could emit its status before the list refresh (sidebar re-reads statuses on fetch anyway).
- : listen promise has no .catch; an unavailable Tauri runtime (plain browser) would log an unhandled rejection, harmless.
- : enhance-status-changed for a meeting not yet in the list is dropped until the next fetchMeetings (which merges statuses).
- : listen promise has no .catch (same as Task 6).
- : savePreferences toast always says "Device preferences saved" (plan-mandated reuse).
- : if enhance_model is not among downloaded models the Select trigger is blank while the stored value is still used; hint shows only when the list is empty.
- : switch flips before save; a save failure leaves the UI out of sync (matches existing handleAutoSaveToggle pattern).
- : amber hint flashes before the model list loads.
- : write_enhance_status can create a stub metadata.json before write_retranscription_metadata, which then skips the base v1 keys (latent; all real folders have metadata.json; Phase 2 reconcile should use entry().or_insert for base keys).
- : a panic inside run_retranscription leaves status "running" (locks and guard release; no failed write).
- : Whisper stays resident after an enhance that overlapped a recording (reused next time).
- : settings hint should also show when the stored enhance_model is not among downloaded models.
- : duplicate refetch when the dialog and the page both listen to retranscription-complete (harmless).
- : theoretical late metadata write if save_recording_only hits its 5-minute timeout.
- : the Parakeet unload after stop is still unguarded; only matters if enhance_provider is set to parakeet (spec default is whisper).
- : cancelling a queued meeting is not supported (returns an error); dedupe of a meeting submitted twice not implemented.
- user cancel shows two toasts (dialog info + sidebar "Enhance failed: Retranscription cancelled"). Ruling: cosmetic; skip the sidebar toast when the error text is a cancellation in a follow-up.
- refused cancel on a queued meeting is only logged, dialog closes without showing the message. Ruling: queued-cancel is unsupported in Phase 1; surface it when queued-cancel is implemented.
- microsecond race in cancel_retranscription (lock released before setting the flag). Ruling: real but needs a cancel at the exact job hand-off; set the flag under the lock in a follow-up.
- tests reset the running meeting with bare .lock().unwrap(), a panic would cascade. Ruling: test hygiene only.
- enhance job's unload_engine_after_batch vs a live recording started during the job. Ruling: live provider is Parakeet (spec 4.1); the final reviewer also found the batch unload already skips while recording.
- a cancelled job is recorded as "failed" and shows the chip. Ruling: status union has no cancelled value (spec 5.2); revisit if cancel becomes common.
