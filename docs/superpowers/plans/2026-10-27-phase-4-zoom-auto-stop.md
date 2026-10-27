# Phase 4: Zoom Auto-Stop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the Zoom meeting ends, the recording stops by itself about 20 seconds later (default on). A recording starts when a Zoom meeting begins (default on). The meeting's `metadata.json` records how it ended.

**Architecture:** A small macOS-only background task polls every 5 seconds for Zoom's in-meeting helper process `CptHost` (present only while a meeting is in progress; verified on this Mac: present during a call, absent with Zoom open and idle). A pure, tested state machine turns the samples into `Started` and `Ended` events with a 4-tick (20 s) debounce on the end. On `Ended` while recording, the task calls the same stop path the tray uses, through a new `stop_for_system` helper that also stamps `ended_by: "auto-stop"` into the metadata and notifies. On `Started`, when the optional switch is on and nothing is recording, it triggers the frontend's auto-start exactly like the tray does. Two preferences and two switches in Recording settings.

**Tech Stack:** Rust (Tauri 2, tokio, `std::process::Command` for `pgrep`), TypeScript (React, shadcn `Switch`).

Spec: `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md`, section 4.5 and the `ended_by` key of 5.2.

## Global Constraints

- Additive changes marked `// spec 4.5`. New logic lives in a new top-level module `frontend/src-tauri/src/call_detector.rs`. Edits to existing files stay small.
- Detection signal: a process whose exact name is `CptHost` (`pgrep -x CptHost`). Poll period 5 s; end debounce 20 s (4 consecutive idle samples after a call). macOS only (`#[cfg(target_os = "macos")]` around the runtime loop and the spawn; the state machine itself is platform-neutral so its tests run everywhere).
- Preferences in `recording_preferences.json`: `auto_stop_on_call_end: bool` (default true), `auto_start_on_call: bool` (default true), both with serde defaults so existing stores load.
- Auto-stop reuses the tray's stop path: `recording_commands::stop_recording(app, RecordingArgs { save_path })` then emit `recording-stop-complete` with `true` so the frontend saves the row (and Phase 1's auto-enhance follows). `metadata.json` gets `ended_by: "auto-stop"`; a user stop writes `ended_by: "user"`.
- Auto-start reuses the tray's start path: set `sessionStorage.autoStartRecording` and navigate the main window to `/`. Skipped with a warning when the main window is not available.
- No em-dashes in any text, code comment or UI string.
- Rust tests: `cd frontend/src-tauri && cargo test --lib <module>`; full suite on `main` is 268 passed, 3 ignored, upstream warnings exist, add none. Type check: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json`. Never run pnpm.
- Branch `phase-4/zoom-auto-stop`, one commit per task, no push.

## File Structure

| File | Responsibility |
| --- | --- |
| `frontend/src-tauri/src/audio/recording_preferences.rs` (modify) | Two new preferences with defaults and a test |
| `frontend/src/components/RecordingSettings.tsx` (modify) | TS fields and two switches |
| `frontend/src-tauri/src/audio/recording_saver.rs` (modify) | `ended_by` on metadata, `set_ended_by` |
| `frontend/src-tauri/src/audio/recording_manager.rs` (modify) | `set_ended_by` pass-through |
| `frontend/src-tauri/src/audio/recording_commands.rs` (modify) | `stop_for_system(app, ended_by)` |
| `frontend/src-tauri/src/call_detector.rs` (create) | `CallState`, `step`, `zoom_meeting_active`, `run` |
| `frontend/src-tauri/src/lib.rs` (modify) | `mod call_detector;` and the spawn in setup |

---

### Task 1: Preferences

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs` (struct, `Default`, tests)
- Modify: `frontend/src/components/RecordingSettings.tsx` (interface and initial state)

**Interfaces:**
- Produces: `RecordingPreferences.auto_stop_on_call_end: bool` and `auto_start_on_call: bool` (Rust) and the matching TS fields `auto_stop_on_call_end: boolean; auto_start_on_call: boolean;`. Task 3 reads them through `load_recording_preferences`.

- [ ] **Step 1: Failing test**

In the existing `mod tests` of `recording_preferences.rs` add:

```rust
    #[test]
    fn call_detection_defaults_stop_on_and_start_on() {
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert!(p.auto_stop_on_call_end);
        assert!(p.auto_start_on_call);
        let d = RecordingPreferences::default();
        assert!(d.auto_stop_on_call_end);
        assert!(d.auto_start_on_call);
    }
```

Run: `cd frontend/src-tauri && cargo test --lib recording_preferences`
Expected: compile error, no field `auto_stop_on_call_end`.

- [ ] **Step 2: Fields**

Add to the struct after `enhance_language`:

```rust
    // spec 4.5: Zoom call detection
    /// Stop the recording about 20 seconds after the Zoom meeting ends.
    #[serde(default = "default_true")]
    pub auto_stop_on_call_end: bool,
    /// Start a recording when a Zoom meeting begins.
    #[serde(default = "default_true")]
    pub auto_start_on_call: bool,
```

(`default_true` already exists from Phase 1.) In `impl Default`, add `auto_stop_on_call_end: true, auto_start_on_call: true,`.

In `RecordingSettings.tsx`, add to the `RecordingPreferences` interface:

```ts
  // spec 4.5
  auto_stop_on_call_end: boolean;
  auto_start_on_call: boolean;
```

and to the initial `useState` object: `auto_stop_on_call_end: true, auto_start_on_call: true,`.

- [ ] **Step 3: Verify and commit**

Run: `cd frontend/src-tauri && cargo test --lib recording_preferences` (3 passed) and `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json` (silent).

```bash
git add frontend/src-tauri/src/audio/recording_preferences.rs frontend/src/components/RecordingSettings.tsx
git commit -m "feat(call-detector): auto-stop and auto-start preferences"
```

---

### Task 2: `ended_by` and `stop_for_system`

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_saver.rs` (`MeetingMetadata`, `RecordingSaver` fields and `new()`, a setter, the completed-metadata block in `stop_and_save`)
- Modify: `frontend/src-tauri/src/audio/recording_manager.rs` (pass-through)
- Modify: `frontend/src-tauri/src/audio/recording_commands.rs` (new function)

**Interfaces:**
- Produces: `RecordingSaver::set_ended_by(&mut self, reason: Option<String>)`, `RecordingManager::set_ended_by(&mut self, reason: Option<String>)`, and `pub async fn recording_commands::stop_for_system<R: Runtime>(app: AppHandle<R>, ended_by: &str) -> Result<(), String>`. Task 3 calls `stop_for_system(app, "auto-stop")`.
- `metadata.json` gains `ended_by` (`"user"` by default, `"auto-stop"` when the detector stopped it).

- [ ] **Step 1: Metadata and saver**

In `MeetingMetadata` add after `transcription`:

```rust
    /// spec 4.5: "user" or "auto-stop"
    #[serde(default)]
    pub ended_by: Option<String>,
```

Set `ended_by: None` in the metadata literal in `initialize_meeting_folder`. Add a saver field `ended_by: Option<String>` (initialize to `None` in `new()`) and a setter next to `set_project`:

```rust
    /// spec 4.5: how the recording ended; written into metadata.json at stop
    pub fn set_ended_by(&mut self, reason: Option<String>) {
        self.ended_by = reason;
    }
```

In `stop_and_save`, inside the block that marks the metadata completed, right after `metadata.completed_at = Some(...)`, add:

```rust
            metadata.ended_by = Some(self.ended_by.clone().unwrap_or_else(|| "user".to_string())); // spec 4.5
```

- [ ] **Step 2: Manager pass-through**

In `recording_manager.rs`, next to `set_project`:

```rust
    // spec 4.5
    pub fn set_ended_by(&mut self, reason: Option<String>) {
        self.recording_saver.set_ended_by(reason);
    }
```

- [ ] **Step 3: `stop_for_system`**

In `recording_commands.rs`, below `stop_recording`:

```rust
/// spec 4.5: stop initiated by the system (Zoom call ended), not by the user.
/// Stamps `ended_by`, runs the normal stop, then tells the frontend to save, exactly like the tray does.
pub async fn stop_for_system<R: Runtime>(app: AppHandle<R>, ended_by: &str) -> Result<(), String> {
    {
        let mut guard = RECORDING_MANAGER.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(manager) = guard.as_mut() {
            manager.set_ended_by(Some(ended_by.to_string()));
        }
    }
    stop_recording(app.clone(), RecordingArgs { save_path: String::new() }).await?;
    if let Err(e) = app.emit("recording-stop-complete", true) {
        warn!("stop_for_system: failed to emit recording-stop-complete: {}", e);
    }
    if let Some(state) = app.try_state::<crate::notifications::commands::NotificationManagerState<R>>() {
        let lock = state.read().await;
        if let Some(manager) = lock.as_ref() {
            if let Err(e) = manager.show_recording_stopped().await {
                warn!("stop_for_system: notification failed: {}", e);
            }
        }
    }
    Ok(())
}
```

`stop_recording` ignores `save_path` (its parameter is `_args`), so an empty string is fine. `RECORDING_MANAGER` is the module's existing `Mutex<Option<RecordingManager>>`. Make sure `use tauri::Manager;` is present for `try_state`.

- [ ] **Step 4: Verify and commit**

Run: `cd frontend/src-tauri && cargo check` (no new warnings) and `cargo test --lib recording_preferences` (still green).

```bash
git add frontend/src-tauri/src/audio/recording_saver.rs frontend/src-tauri/src/audio/recording_manager.rs frontend/src-tauri/src/audio/recording_commands.rs
git commit -m "feat(call-detector): ended_by in metadata and a system stop path"
```

---

### Task 3: The detector

**Files:**
- Create: `frontend/src-tauri/src/call_detector.rs`
- Modify: `frontend/src-tauri/src/lib.rs` (`mod call_detector;` next to the other `mod` lines; spawn in setup after the reconcile block)

**Interfaces:**
- Consumes: `recording_preferences::load_recording_preferences`, `recording_commands::is_recording().await`, `recording_commands::stop_for_system` (Task 2).
- Produces: `pub struct CallState`, `pub enum CallEvent { Started, Ended }`, `pub fn step(state: &mut CallState, in_call: bool, end_debounce_ticks: u32) -> Option<CallEvent>`, `pub fn zoom_meeting_active() -> bool` (macOS), `pub async fn run<R: Runtime>(app: AppHandle<R>)` (macOS).

- [ ] **Step 1: Failing tests**

Create `call_detector.rs`:

```rust
//! spec 4.5: Zoom call detection. Zoom on macOS runs the helper process `CptHost`
//! only while a meeting is in progress. A background task samples it every
//! 5 seconds; a pure state machine turns the samples into Started and Ended
//! events, debouncing the end so a reconnect blip does not stop a recording.

use log::{info, warn};
use tauri::{AppHandle, Runtime};

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(samples: &[bool], debounce: u32) -> Vec<Option<CallEvent>> {
        let mut s = CallState::default();
        samples.iter().map(|&x| step(&mut s, x, debounce)).collect()
    }

    #[test]
    fn started_fires_on_first_in_call_sample() {
        assert_eq!(feed(&[false, false, true, true], 4), vec![None, None, Some(CallEvent::Started), None]);
    }

    #[test]
    fn ended_fires_after_debounce_ticks_of_idle() {
        let out = feed(&[true, false, false, false, false, false], 4);
        assert_eq!(out, vec![Some(CallEvent::Started), None, None, None, Some(CallEvent::Ended), None]);
    }

    #[test]
    fn a_blip_resets_the_debounce() {
        let out = feed(&[true, false, false, true, false, false, false, false], 4);
        assert_eq!(
            out,
            vec![Some(CallEvent::Started), None, None, None, None, None, None, Some(CallEvent::Ended)]
        );
    }

    #[test]
    fn idle_without_a_call_never_ends() {
        assert!(feed(&[false; 10], 4).iter().all(|e| e.is_none()));
    }

    #[test]
    fn a_second_call_starts_again() {
        let out = feed(&[true, false, false, false, false, true], 4);
        assert_eq!(out[4], Some(CallEvent::Ended));
        assert_eq!(out[5], Some(CallEvent::Started));
    }
}
```

Add `mod call_detector;` to `lib.rs` next to the other module declarations.

Run: `cd frontend/src-tauri && cargo test --lib call_detector`
Expected: compile errors, missing `CallState`, `step`, `CallEvent`.

- [ ] **Step 2: Implement**

Insert above the tests:

```rust
/// Poll period for the helper process check.
pub const POLL_SECONDS: u64 = 5;
/// Idle samples required after a call before `Ended` fires (4 x 5 s = 20 s).
pub const END_DEBOUNCE_TICKS: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallEvent {
    Started,
    Ended,
}

#[derive(Debug, Default, Clone)]
pub struct CallState {
    in_call: bool,
    idle_ticks: u32,
}

/// Feed one sample. Returns `Started` on the first in-call sample after idle, and
/// `Ended` once `end_debounce_ticks` consecutive idle samples follow a call.
pub fn step(state: &mut CallState, in_call: bool, end_debounce_ticks: u32) -> Option<CallEvent> {
    if in_call {
        state.idle_ticks = 0;
        if !state.in_call {
            state.in_call = true;
            return Some(CallEvent::Started);
        }
        return None;
    }
    if state.in_call {
        state.idle_ticks += 1;
        if state.idle_ticks >= end_debounce_ticks {
            state.in_call = false;
            state.idle_ticks = 0;
            return Some(CallEvent::Ended);
        }
    }
    None
}

/// True while Zoom's in-meeting helper `CptHost` is running.
/// ponytail: shells out to pgrep every 5 s instead of binding sysinfo's process API; swap if the cost ever shows.
#[cfg(target_os = "macos")]
pub fn zoom_meeting_active() -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", "CptHost"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Background loop: sample, step, act according to the preferences.
#[cfg(target_os = "macos")]
pub async fn run<R: Runtime>(app: AppHandle<R>) {
    use tauri::Manager;
    let mut state = CallState::default();
    info!("Call detector started (CptHost, every {}s, end debounce {} ticks)", POLL_SECONDS, END_DEBOUNCE_TICKS);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(POLL_SECONDS)).await;
        let sample = tokio::task::spawn_blocking(zoom_meeting_active).await.unwrap_or(false);
        let Some(event) = step(&mut state, sample, END_DEBOUNCE_TICKS) else { continue };
        let prefs = match crate::audio::recording_preferences::load_recording_preferences(&app).await {
            Ok(p) => p,
            Err(e) => {
                warn!("Call detector: could not load preferences, ignoring event {:?}: {}", event, e);
                continue;
            }
        };
        let recording = crate::audio::recording_commands::is_recording().await;
        match event {
            CallEvent::Ended if recording && prefs.auto_stop_on_call_end => {
                info!("Zoom meeting ended; stopping the recording");
                if let Err(e) = crate::audio::recording_commands::stop_for_system(app.clone(), "auto-stop").await {
                    warn!("Call detector: auto-stop failed: {}", e);
                }
            }
            CallEvent::Started if !recording && prefs.auto_start_on_call => {
                info!("Zoom meeting started; asking the frontend to start a recording");
                match app.get_webview_window("main") {
                    Some(window) => {
                        let _ = window.eval("sessionStorage.setItem('autoStartRecording', 'true')");
                        let _ = window.eval("window.location.assign('/')");
                    }
                    None => warn!("Call detector: main window not available, auto-start skipped"),
                }
            }
            _ => {}
        }
    }
}
```

In `lib.rs` setup, right after the reconcile block:

```rust
            // spec 4.5: stop (and optionally start) recordings with the Zoom meeting
            #[cfg(target_os = "macos")]
            {
                let handle = _app.handle().clone();
                tauri::async_runtime::spawn(call_detector::run(handle));
            }
```

If `log` or `tauri::Runtime` imports are unused on non-macOS builds, gate them with the same `cfg`; this repo builds on macOS, so keep it simple and only fix what `cargo check` reports.

- [ ] **Step 3: Verify and commit**

Run: `cd frontend/src-tauri && cargo test --lib call_detector` (5 passed) and `cargo check` (no new warnings).

```bash
git add frontend/src-tauri/src/call_detector.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(call-detector): stop the recording when the Zoom meeting ends"
```

---

### Task 4: Settings switches

**Files:**
- Modify: `frontend/src/components/RecordingSettings.tsx` (two handlers and one JSX block after the enhance block from Phase 1)

- [ ] **Step 1: Handlers**

Next to `handleAutoEnhanceToggle`:

```ts
  // spec 4.5
  const handleAutoStopToggle = async (enabled: boolean) => {
    const next = { ...preferences, auto_stop_on_call_end: enabled };
    setPreferences(next);
    await savePreferences(next);
  };

  const handleAutoStartToggle = async (enabled: boolean) => {
    const next = { ...preferences, auto_start_on_call: enabled };
    setPreferences(next);
    await savePreferences(next);
  };
```

- [ ] **Step 2: JSX**

Directly after the `{/* spec 4.1: enhance after recording */}` block:

```tsx
      {/* spec 4.5: Zoom call detection */}
      <div className="p-4 border rounded-lg space-y-4">
        <div className="flex items-center justify-between">
          <div className="flex-1">
            <div className="font-medium">Stop recording when the Zoom call ends</div>
            <div className="text-sm text-gray-600">
              About 20 seconds after you leave the meeting, the recording stops and is saved
            </div>
          </div>
          <Switch checked={preferences.auto_stop_on_call_end} onCheckedChange={handleAutoStopToggle} disabled={saving} />
        </div>
        <div className="flex items-center justify-between">
          <div className="flex-1">
            <div className="font-medium">Start recording when a Zoom call begins</div>
            <div className="text-sm text-gray-600">
              Records into the project selected on the home screen
            </div>
          </div>
          <Switch checked={preferences.auto_start_on_call} onCheckedChange={handleAutoStartToggle} disabled={saving} />
        </div>
      </div>
```

- [ ] **Step 3: Verify and commit**

Run: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json` (silent).

```bash
git add frontend/src/components/RecordingSettings.tsx
git commit -m "feat(call-detector): settings switches for auto-stop and auto-start"
```

---

### Task 5: Manual acceptance (owner, real Zoom call)

- [ ] **Step 1:** Restart the dev app. The log shows `Call detector started`.
- [ ] **Step 2:** Start a Zoom meeting alone (your personal meeting room), start recording in Meetily, speak for 30 seconds, then leave the meeting. Expected: within about 25 seconds the recording stops on its own, a "recording stopped" notification appears, the meeting is saved, the enhance job runs, and `metadata.json` has `"ended_by": "auto-stop"`. (notifications for system stops are shown regardless of the "recording stopped" preference)
- [ ] **Step 3:** Record 20 seconds without Zoom and stop manually. Expected: `"ended_by": "user"`, nothing else changes.
- [ ] **Step 4:** With "Start recording when a Zoom call begins" on (the default), join a meeting. Expected: the home screen opens and a recording starts into the selected project within about 10 seconds. Leave the meeting; it stops. Turn the switch back off if you do not want it.
- [ ] **Step 5:** Finish with `superpowers:finishing-a-development-branch` (squash merge to `main`).

---

## Self-review against the spec

- 4.5 signal (`CptHost`), 5 s poll, 20 s debounce, state machine tested, auto-stop default on through the tray stop path with `ended_by: "auto-stop"`, auto-start default on through the tray start path, two preferences with switches, macOS gating: Tasks 1 to 4.
- 5.2 `ended_by` key: Task 2.
- Fallback (system-audio silence) deliberately not built, as the spec says.
- Names used across tasks: `auto_stop_on_call_end`, `auto_start_on_call`, `set_ended_by`, `stop_for_system`, `CallState`, `CallEvent`, `step`, `zoom_meeting_active`, `run`, `POLL_SECONDS`, `END_DEBOUNCE_TICKS`.

## Outcome (2026-10-27)

Implemented on branch `phase-4/zoom-auto-stop` (12 commits incl. docs). Task reviews passed clean. The whole-branch review found one Critical race (a manual stop inside the 20 s debounce could overlap the detector's stop and create a duplicate row), fixed at the root by making a system stop a no-op once another stop has begun; plus hardenings: only a recording live during the call is auto-stopped, failed process checks are skipped, the stop action is isolated, and system stops notify regardless of the per-type preference. At the owner's request after acceptance, auto-start on a Zoom meeting defaults to on. Owner acceptance on a real Zoom call passed.

### Follow-ups deferred from review (none block merge)

- : stop_for_system still emits recording-stop-complete and notifies when nothing is recording (stop_recording returns Ok early); Task 3 only calls it while recording.
- : three identical toggle handlers could share one generic helper.
- : a pgrep spawn/join failure counts as idle; skip the sample on error if acceptance shows flakiness.
- : a panic inside stop_for_system would silently kill the detector task.
- : ignored transitions are not logged (a debug line would help diagnose a missed auto-stop).
- : top-level imports not cfg-gated (warn on non-macOS builds; accepted).
- : ended_by (and status completed) not written when auto_save is off (pre-existing early return).
- : first detector tick 5 s after setup can land during a slow dev-mode page load for auto-start.
- : an IS_RECORDING_STOPPING early return in stop_recording would protect the UI+tray pair too (changes existing callers' error surface; out of scope).
- microsecond check-then-act window between stop_for_system releasing the manager lock and stop_recording taking the manager; a check-and-set on the stopping flag inside stop_recording would close it for every caller. Ruling: real but microseconds wide with no await inside; follow-up alongside the IS_RECORDING_STOPPING early return noted earlier.
- intermediate commits 9615411/0ebdde9 may not build alone; squash merge makes it moot.
