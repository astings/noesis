# Job Queue Follow-ups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clear the known gaps of the job queue panel. Free the sidebar meeting title from the space its hidden buttons reserve. Repair meetings whose enhance status is stuck at `running`.

**Architecture:** All changes are small and local:
- a startup sweep in `audio/enhance_status.rs`;
- a drop guard in `jobs.rs`;
- a `cancelled` hook status, plus a refusal of overlapping hook runs, in `post_enhance_hook.rs` / `hook_status.rs`;
- a cancel poll around the title/tags LLM call in `meeting_tagging.rs`;
- a class change on the sidebar row.

**Tech Stack:** Rust (Tauri 2, tokio), Next.js 14 + React 18, Tailwind.

Branch: `fix/job-queue-followups`. Builds on the job queue panel: spec `docs/superpowers/specs/2026-10-28-job-queue-panel-design.md`, plan `docs/superpowers/plans/2026-10-28-job-queue-panel.md`.

## Findings that drive this plan

1. **Stuck "enhance running"** (meeting `_inbox/2026-10-28_11-02`).
   - Its `metadata.json` has `transcription.enhanced.status = "running"`, `at = 2026-10-28T15:16:11Z`.
   - The enhance started, then the app quit or was hot-rebuilt before it finished. `tauri:dev` rebuilds on every Rust edit, and there were many today.
   - Nothing at startup resets `queued` or `running`, so the badge stays forever.
   - It is the only stuck meeting on disk.
   - `audio_cleanup` deletes audio only when the status is `done`, so this meeting's audio is intact and can be re-enhanced.
2. **Hook cancel shows "Hook failed".** A cancelled hook reports `failed`, and `SidebarProvider.tsx:213-215` toasts every `failed`.
3. **An enhance panic leaves its job Running.** No guard finishes the job when `run_retranscription` unwinds.
4. **A manual "Run hook" during an auto-run hook attaches to the auto job's row.**
   - `jobs::hook_status`, `set_hook_pid` and `hook_cancel_requested` are keyed by meeting.
   - Wiki automation already refuses a second run of the same meeting. The custom command path does not.
5. **Single-meeting title/tags cannot be cancelled.**
   - It is one LLM call (`llm_client::generate_summary` in `meeting_tagging::run_inner`), followed by writes: `write_tags`, then `library::assign`.
   - Cancelling during the call is safe. Cancelling during the writes is not, because the folder rename and the DB update must stay together.
6. **Sidebar titles are cramped.**
   - The row (`Sidebar/index.tsx:420-449`) keeps the Rename and Delete buttons in layout with `opacity-0`, so they reserve about 44 px even when invisible.
   - At a sidebar width of 260 px, that leaves the title about 130 px.

## Global Constraints

- Filesystem is the source of truth. The startup sweep rewrites only `transcription.enhanced` through the existing `write_enhance_status`, and keeps every other key.
- Existing events keep firing. New values may be added (`cancelled` for hook and tagging status); existing ones keep their meaning.
- Cancel never interrupts a write. It only interrupts waiting: the queue, the LLM call, or a child process.
- No new crate or npm dependency.
- Rename and Delete stay reachable by keyboard through the existing right-click menu (Rename, Move to section, Delete).
- Use `/usr/bin/git` (a shell hook rewrites plain `git`). Run Rust tests from `frontend/src-tauri`. Type check with `cd frontend && pnpm exec tsc --noEmit`.
  - Errors already present in `src/app/layout.tsx` (CSS imports) and `tests/**` (`bun:test`) are known. Only new errors count.
- A `tauri:dev` started from this checkout hot-rebuilds on every Rust edit. Tell the owner before editing if one is running.

## File map

| File | Change |
|---|---|
| `frontend/src-tauri/src/audio/enhance_status.rs` | `mark_interrupted(root)` + tests |
| `frontend/src-tauri/src/lib.rs` | call it at startup, before reconcile |
| `frontend/src-tauri/src/jobs.rs` | `FailOnDrop` guard + test; `hook_status` handles `"cancelled"` |
| `frontend/src-tauri/src/audio/retranscription.rs` | guard around the transcription run; pass the job id to the title step |
| `frontend/src-tauri/src/audio/hook_status.rs` | `CANCELLED` constant |
| `frontend/src-tauri/src/audio/post_enhance_hook.rs` | report `cancelled`; refuse a run while one is running |
| `frontend/src-tauri/src/audio/meeting_tagging.rs` | cancel poll around the LLM call; `cancelled` tagging status |
| `frontend/src/components/Sidebar/SidebarProvider.tsx` | no toast on `cancelled` |
| `frontend/src/components/MeetingStatusTags.tsx` | label `cancelled` |
| `frontend/src/components/Jobs/JobsButton.tsx` | show Cancel on single title/tags rows again |
| `frontend/src/components/Sidebar/index.tsx` | buttons take space only on hover |

---

### Task 1: Repair stuck enhance statuses at startup

**Files:**
- Modify: `frontend/src-tauri/src/audio/enhance_status.rs`
- Modify: `frontend/src-tauri/src/lib.rs` (startup block, just before `// spec 4.2: repair the meeting index from disk once at startup`, ~line 620)

**Interfaces:**
- Produces: `pub fn mark_interrupted(root: &Path) -> usize` (number of folders repaired).

- [ ] **Step 1: Write the failing test** (in `enhance_status.rs` `mod tests`; create the module with `use super::*;` if there is none)

```rust
    #[test]
    fn mark_interrupted_fails_running_and_queued_and_keeps_the_rest() {
        let root = tempfile::tempdir().unwrap();
        let make = |name: &str, status: Option<&str>| {
            let dir = root.path().join("proj").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let mut meta = serde_json::json!({ "meeting_id": name, "title": name });
            if let Some(s) = status {
                meta["transcription"] = serde_json::json!({ "enhanced": { "provider": "whisper", "model": "large-v3", "status": s, "at": "x", "error": null } });
            }
            std::fs::write(dir.join("metadata.json"), meta.to_string()).unwrap();
            dir
        };
        let running = make("a", Some(STATUS_RUNNING));
        let queued = make("b", Some(STATUS_QUEUED));
        let done = make("c", Some(STATUS_DONE));
        let legacy = make("d", None);

        assert_eq!(mark_interrupted(root.path()), 2);

        let read = |d: &std::path::Path| -> serde_json::Value { serde_json::from_str(&std::fs::read_to_string(d.join("metadata.json")).unwrap()).unwrap() };
        for d in [&running, &queued] {
            let m = read(d);
            assert_eq!(m["transcription"]["enhanced"]["status"], STATUS_FAILED);
            assert_eq!(m["transcription"]["enhanced"]["error"], INTERRUPTED);
            assert_eq!(m["transcription"]["enhanced"]["model"], "large-v3", "provider and model kept");
            assert_eq!(m["title"], m["meeting_id"], "other keys kept");
        }
        assert_eq!(read(&done)["transcription"]["enhanced"]["status"], STATUS_DONE);
        assert!(read(&legacy).get("transcription").is_none());
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cd frontend/src-tauri && cargo test --lib enhance_status::tests::mark_interrupted`
Expected: compile error, `mark_interrupted` / `INTERRUPTED` not found.

- [ ] **Step 3: Implement** (in `enhance_status.rs`, after `read_enhance_status`)

```rust
/// Error recorded on an enhance the app did not live to finish.
pub const INTERRUPTED: &str = "Interrupted: the app closed before enhance finished. Enhance again to retry.";

/// No enhance job survives a restart, so a `queued` or `running` status found at startup is
/// stale (the app quit or was rebuilt mid-job). Marks those `failed` under `<root>/<project>/<meeting>/`
/// and keeps provider and model. Audio is kept: cleanup only deletes after `done`.
pub fn mark_interrupted(root: &Path) -> usize {
    let mut repaired = 0;
    let Ok(projects) = std::fs::read_dir(root) else { return 0 };
    for project in projects.flatten().filter(|e| e.path().is_dir()) {
        let Ok(meetings) = std::fs::read_dir(project.path()) else { continue };
        for meeting in meetings.flatten().filter(|e| e.path().is_dir()) {
            let folder = meeting.path();
            let status = read_enhance_status(&folder);
            if !matches!(status.as_deref(), Some(STATUS_QUEUED) | Some(STATUS_RUNNING)) {
                continue;
            }
            let text = std::fs::read_to_string(folder.join("metadata.json")).unwrap_or_default();
            let meta: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            let enhanced = &meta["transcription"]["enhanced"];
            let provider = enhanced["provider"].as_str().unwrap_or("whisper");
            let model = enhanced["model"].as_str().unwrap_or("configured");
            match write_enhance_status(&folder, provider, model, STATUS_FAILED, Some(INTERRUPTED)) {
                Ok(()) => {
                    log::info!("enhance status: marked interrupted job failed in {}", folder.display());
                    repaired += 1;
                }
                Err(e) => log::warn!("enhance status: could not repair {}: {}", folder.display(), e),
            }
        }
    }
    repaired
}
```

- [ ] **Step 4: Call it at startup** (in `lib.rs`, immediately before the `// spec 4.2: repair the meeting index from disk once at startup` block)

```rust
            // no enhance job survives a restart: a queued/running status on disk is stale
            let interrupted = audio::enhance_status::mark_interrupted(&audio::get_default_recordings_folder());
            if interrupted > 0 {
                log::info!("Marked {} interrupted enhance job(s) failed", interrupted);
            }
```

If `enhance_status` is not `pub` from `audio`, check `audio/mod.rs` and use the existing path (`pub mod enhance_status;` is expected).

- [ ] **Step 5: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib enhance_status && cargo check`
Expected: pass; no new warnings in touched files.

- [ ] **Step 6: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/audio/enhance_status.rs frontend/src-tauri/src/lib.rs
/usr/bin/git commit -m "fix(enhance): a queued or running status found at startup is marked interrupted"
```

---

### Task 2: An enhance that panics still ends its job

**Files:**
- Modify: `frontend/src-tauri/src/jobs.rs` (next to the free functions, before `#[cfg(test)]`)
- Modify: `frontend/src-tauri/src/audio/retranscription.rs` (`start_retranscription`, around `let result = run_retranscription(...).await;`)

**Interfaces:**
- Produces: `pub struct FailOnDrop`, with `FailOnDrop::new(id: &str) -> FailOnDrop` and `fn disarm(self)`.

- [ ] **Step 1: Write the failing test** (in `jobs.rs` `mod tests`; it uses the global registry, so it picks a unique id)

```rust
    #[test]
    fn fail_on_drop_fails_the_job_unless_disarmed() {
        add("fod-a", JobKind::Download, None, "m", Control::WhisperDownload, true);
        add("fod-b", JobKind::Download, None, "m", Control::WhisperDownload, true);
        let a = FailOnDrop::new("fod-a");
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = a;
            panic!("engine blew up");
        }));
        FailOnDrop::new("fod-b").disarm();
        let state = |id: &str| list().into_iter().find(|j| j.id == id).unwrap().state;
        assert_eq!(state("fod-a"), JobState::Failed);
        assert_eq!(state("fod-b"), JobState::Running);
        finish("fod-b", Outcome::Done);
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cd frontend/src-tauri && cargo test --lib jobs::tests::fail_on_drop`
Expected: compile error, `FailOnDrop` not found.

- [ ] **Step 3: Implement** (in `jobs.rs`)

```rust
/// Fails the job if dropped while armed: a panic unwinding through the work ends the
/// row instead of leaving it Running. `finish` is idempotent, so an ended job is untouched.
pub struct FailOnDrop(Option<String>);

impl FailOnDrop {
    pub fn new(id: &str) -> Self {
        Self(Some(id.to_string()))
    }
    pub fn disarm(mut self) {
        self.0 = None;
    }
}

impl Drop for FailOnDrop {
    fn drop(&mut self) {
        if let Some(id) = self.0.take() {
            finish(&id, Outcome::Failed("Enhance stopped unexpectedly. See the app log.".to_string()));
        }
    }
}
```

- [ ] **Step 4: Arm it around the transcription run** (in `start_retranscription`, inside the `let result = {` block)

```rust
        let crash_guard = jobs::FailOnDrop::new(&job_id);
        let result = run_retranscription(app.clone(), meeting_id.clone(), meeting_folder_path, language, model, provider).await;
        crash_guard.disarm();
```

This replaces the existing `let result = run_retranscription(...)` line.

- [ ] **Step 5: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib jobs:: && cargo test --lib retranscription`
Expected: pass.

- [ ] **Step 6: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/jobs.rs frontend/src-tauri/src/audio/retranscription.rs
/usr/bin/git commit -m "fix(jobs): an enhance that panics ends its job as failed"
```

---

### Task 3: Hook cancel reports "cancelled", and hook runs never overlap

**Files:**
- Modify: `frontend/src-tauri/src/audio/hook_status.rs` (constants, lines ~9-11)
- Modify: `frontend/src-tauri/src/audio/post_enhance_hook.rs` (`dispatch_with`, lines ~151-200; tests)
- Modify: `frontend/src-tauri/src/jobs.rs` (`Registry::hook_status`)
- Modify: `frontend/src/components/Sidebar/SidebarProvider.tsx` (~line 213)
- Modify: `frontend/src/components/MeetingStatusTags.tsx` (line 5)

**Interfaces:**
- Consumes: `crate::jobs::hook_cancel_requested(meeting_id) -> bool`.
- Produces: hook status value `"cancelled"` on `hook-status-changed`. `dispatch_with` returns `Err(HOOK_BUSY)` while a hook of that meeting is running.

- [ ] **Step 1: Write the failing tests** (in `post_enhance_hook.rs` `mod tests`, next to `dispatch_runs_the_command_and_tracks_status`; reuse that test's `recorder()` and prefs setup)

```rust
    #[test]
    fn dispatch_refuses_a_second_run_while_one_is_running() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = read_context(dir.path(), "m-busy");
        let mut prefs = super::super::recording_preferences::RecordingPreferences::default();
        prefs.post_enhance_command = "sleep 2".into();
        let (_seen, notify) = recorder();
        dispatch_with(&prefs, &ctx, true, notify.clone()).unwrap();
        assert_eq!(dispatch_with(&prefs, &ctx, true, notify).unwrap_err(), HOOK_BUSY);
    }
```

In `jobs.rs` `mod tests`:

```rust
    #[test]
    fn hook_cancelled_status_ends_the_job_cancelled() {
        let mut r = Registry::default();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), true, Instant::now());
        r.await_hook("a");
        r.hook_status("m", "running");
        r.hook_status("m", "cancelled");
        assert_eq!(r.snapshot()[0].state, JobState::Cancelled);
    }
```

If `Registry` has no `await_hook` method (the free fn may set the flag directly), use whatever the existing hook tests in `jobs.rs` call to mark a job as awaiting its hook.

- [ ] **Step 2: Run them to see them fail**

Run: `cd frontend/src-tauri && cargo test --lib post_enhance_hook::tests::dispatch_refuses && cargo test --lib jobs::tests::hook_cancelled`
Expected: compile error (`HOOK_BUSY`), then the jobs test fails (state Running).

- [ ] **Step 3: Implement**

`hook_status.rs`, next to `FAILED`:

```rust
pub const CANCELLED: &str = "cancelled";
```

`post_enhance_hook.rs`, next to `NO_HOOK`:

```rust
pub const HOOK_BUSY: &str = "A hook is already running for this meeting.";
```

In `dispatch_with`, after the `NO_HOOK` check:

```rust
    // one hook run per meeting: an overlapping run would share its job row and pid slot
    if super::hook_status::memory_status(&ctx.meeting_id) == Some(super::hook_status::RUNNING) {
        return Err(HOOK_BUSY.to_string());
    }
```

Replace the exit closure:

```rust
    let then: OnExit = Box::new(move |success| t(if success { DONE } else { FAILED }));
```

with this, adding `CANCELLED` to the `use super::hook_status::{…}` line:

```rust
    let cancel_id = ctx.meeting_id.clone();
    // the reaper already turned a cancelled exit into `false`; tell it apart from a failure
    let then: OnExit = Box::new(move |success| {
        t(if success { DONE } else if crate::jobs::hook_cancel_requested(&cancel_id) { CANCELLED } else { FAILED })
    });
```

`jobs.rs` `Registry::hook_status`, add an arm before `_ => {}`:

```rust
            "cancelled" => {
                self.step(&id, STEP_WIKI, JobState::Cancelled, None);
                self.finish(&id, Outcome::Cancelled);
            }
```

`SidebarProvider.tsx` needs no change: it toasts only on `'failed'`, so `'cancelled'` is silent. Confirm at line ~213.

`MeetingStatusTags.tsx` line 5:

```ts
const HOOK: Record<string, string> = { done: 'Ran', running: 'Running', failed: 'Failed', cancelled: 'Cancelled' };
```

- [ ] **Step 4: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib post_enhance_hook && cargo test --lib jobs:: && cargo test --lib wiki_automation && cd .. && pnpm exec tsc --noEmit`
Expected: Rust passes; tsc shows only the known errors.

- [ ] **Step 5: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/audio/hook_status.rs frontend/src-tauri/src/audio/post_enhance_hook.rs frontend/src-tauri/src/jobs.rs frontend/src/components/MeetingStatusTags.tsx
/usr/bin/git commit -m "fix(jobs): cancelled hooks report cancelled, and a meeting runs one hook at a time"
```

---

### Task 4: Title/tags can be cancelled while it waits for the model

**Files:**
- Modify: `frontend/src-tauri/src/audio/meeting_tagging.rs` (`run_inner` LLM call ~line 125, `run_step` ~181, `run_after_enhance` ~196, `run_meeting_tagging`, batch loop)
- Modify: `frontend/src-tauri/src/audio/retranscription.rs` (title step call)
- Modify: `frontend/src/components/Sidebar/SidebarProvider.tsx` (~line 227)
- Modify: `frontend/src/components/MeetingStatusTags.tsx` (line 6)
- Modify: `frontend/src/components/Jobs/JobsButton.tsx` (`cancel` helper)

**Interfaces:**
- Changes: `run_step(app, meeting_id, force_title, job: Option<&str>)` and `run_after_enhance(app, meeting_id, job: Option<&str>)`. The new `job` argument is the jobs-registry id whose cancel aborts the LLM call.
- Produces: tagging status value `"cancelled"` on `tagging-status-changed`.

- [ ] **Step 1: Write the failing test** (in `meeting_tagging.rs` `mod tests`)

```rust
    #[tokio::test]
    async fn unless_cancelled_returns_cancelled_when_the_job_is_cancelled() {
        crate::jobs::add("tag-c", crate::jobs::JobKind::Tagging, Some("m"), "", crate::jobs::Control::Tagging, true);
        let slow = async { tokio::time::sleep(std::time::Duration::from_secs(30)).await; Ok::<_, String>(1) };
        let canceller = async {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            crate::jobs::request_cancel("tag-c");
        };
        let (res, _) = tokio::join!(unless_cancelled(Some("tag-c"), slow), canceller);
        assert_eq!(res.unwrap_err(), CANCELLED_MSG);
        assert_eq!(unless_cancelled(None, async { Ok::<_, String>(2) }).await.unwrap(), 2);
    }
```

If `tokio::test` is not available in this crate's tests, check `Cargo.toml` dev-dependencies (`tokio` with `macros` and `rt`). Otherwise build a runtime with `tokio::runtime::Runtime::new().unwrap().block_on(...)`.

- [ ] **Step 2: Run it to see it fail**

Run: `cd frontend/src-tauri && cargo test --lib meeting_tagging::tests::unless_cancelled`
Expected: compile error, `unless_cancelled` not found.

- [ ] **Step 3: Implement the cancel poll** (in `meeting_tagging.rs`)

```rust
pub(crate) const CANCELLED_MSG: &str = "Cancelled";

/// Runs `fut` unless the job is cancelled first; polls every 250 ms. Only the wait for the
/// model goes through here, never the writes after it, so a cancel cannot split a rename.
async fn unless_cancelled<T>(job: Option<&str>, fut: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    let Some(job) = job else { return fut.await };
    let job = job.to_string();
    let cancelled = async move {
        while !crate::jobs::is_cancel_requested(&job) {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    };
    tokio::select! {
        r = fut => r,
        _ = cancelled => Err(CANCELLED_MSG.to_string()),
    }
}
```

Thread `job: Option<&str>` from `run_step` into `run_inner`, and wrap the LLM call:

```rust
    let completion = unless_cancelled(job, crate::summary::llm_client::generate_summary(
        /* … same arguments as today … */
    ))
    .await?;
```

`generate_summary` returns `Result<_, String>`, as the existing `?` shows. If it returns another error type, map it to `String` inside the wrapped future.

In `run_step`, report a cancel as `cancelled`, not `failed`:

```rust
        Err(e) if e == CANCELLED_MSG => set(app, meeting_id, CANCELLED, None),
```

Put this arm before the generic `Err(e)` arm, and add `CANCELLED: &str = "cancelled"` next to the module's other status constants (`RUNNING`, `DONE`, `FAILED`). Update callers:
- `run_meeting_tagging` → `run_step(&app, &meeting_id, true, Some(&job_id))`, with `jobs::add` moved before the call (it already is). Its `jobs::finish` maps `Err` to `Outcome::Failed`, which `finish` turns into Cancelled because cancel was requested.
- the batch loop → `run_step(app, id, false, Some(job_id))`.
- `run_after_enhance(app, meeting_id, job)` → `run_step(app, meeting_id, false, job)`.
- `retranscription.rs` title step → `super::meeting_tagging::run_after_enhance(&app, &meeting_id, Some(&job_id))`. A cancel during the title step now aborts the model call; the existing `is_cancel_requested` check after the step then finishes the job Cancelled.

Remove the comment in `jobs.rs` `cancel_job` that says the title step is not interruptible, and update the `Control::Tagging` comment: the batch checks between meetings, and each call is aborted while waiting for the model.

Frontend:
- `MeetingStatusTags.tsx` line 6: `const TAGGING: Record<string, string> = { done: 'Done', running: 'Running', failed: 'Failed', cancelled: 'Cancelled' };`
- `SidebarProvider.tsx` ~line 227 toasts only on `'failed'`, so `'cancelled'` stays silent. Confirm, no change needed.
- `JobsButton.tsx`: remove the `j.kind === 'tagging' && j.meeting_id ? null :` condition and its comment from `cancel`, so every row has Cancel.

- [ ] **Step 4: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib meeting_tagging && cargo test --lib jobs:: && cargo test --lib retranscription && cd .. && pnpm exec tsc --noEmit`
Expected: Rust passes; tsc shows only the known errors.

- [ ] **Step 5: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/audio/meeting_tagging.rs frontend/src-tauri/src/audio/retranscription.rs frontend/src-tauri/src/jobs.rs frontend/src/components/MeetingStatusTags.tsx frontend/src/components/Jobs/JobsButton.tsx
/usr/bin/git commit -m "feat(jobs): title/tags can be cancelled while waiting for the model"
```

---

### Task 5: Sidebar meeting titles use the full row width

**Files:**
- Modify: `frontend/src/components/Sidebar/index.tsx` (meeting row, lines ~399-449)

- [ ] **Step 1: Buttons take space only on hover or focus**

The button container at line ~427:

```tsx
                  <div className="flex items-center opacity-0 group-hover:opacity-100 transition-opacity duration-150">
```

becomes:

```tsx
                  <div className="ml-1 hidden flex-none items-center group-hover:flex group-focus-within:flex">
```

The title takes the whole row width until the pointer is over the row. Then the two buttons appear and the title truncates to make room. Keyboard users reach Rename and Delete through the row's right-click menu, which already has both.

- [ ] **Step 2: Reduce the meeting indent**

Line ~399: `style={isProject ? undefined : { paddingLeft: '22px' }}` → `{ paddingLeft: '14px' }`.

This keeps meetings visibly inside their project while the chevron column stays 14 px (`w-3.5`), and frees 8 px.

- [ ] **Step 3: Type check**

Run: `cd frontend && pnpm exec tsc --noEmit`
Expected: only the known errors.

- [ ] **Step 4: Commit**

```bash
/usr/bin/git add frontend/src/components/Sidebar/index.tsx
/usr/bin/git commit -m "fix(sidebar): meeting titles use the full row; rename and delete appear on hover"
```

---

### Task 6: Manual check in the app (owner)

- [ ] Restart `pnpm run tauri:dev`. The 11:02 meeting shows Enhance "Failed", and its meeting page offers Enhance. Re-enhance it, and the panel shows the job.
- [ ] Enhance a meeting with a wiki or command hook. Cancel during the Wiki step: the row ends Cancelled, the hook tag says Cancelled, and no "Hook failed" toast appears.
- [ ] Press "Run hook" twice quickly on one meeting: the second press shows "A hook is already running for this meeting."
- [ ] Generate titles and tags, then cancel within a few seconds: the row ends Cancelled, the title is unchanged, and there is no error toast.
- [ ] Sidebar: long titles fill the row, and hovering shows the pencil and trash and truncates the title. Right-click still offers Rename and Delete.
