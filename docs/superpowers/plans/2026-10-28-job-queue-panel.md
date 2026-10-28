# Job Queue Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A title-bar button opening a popover that lists every background job (enhance with its title and wiki steps, summary, model downloads) with status, progress, time left, cancel and retry.

**Architecture:** New `src-tauri/src/jobs.rs` holds an in-memory registry (pure `Registry` struct, unit tested) behind a global mutex; each mutation emits `jobs-changed` with the full list. Existing code paths call `jobs::add / start / progress / step / finish`; the existing events stay. Frontend: `useJobs` hook + `JobsButton` (Radix popover from `ui/popover.tsx`) in `TitleBar.tsx`.

**Tech Stack:** Rust (Tauri 2, once_cell, uuid, chrono, serde), Next.js 14 + React 18, lucide-react, Tailwind.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-10-28-job-queue-panel-design.md`.
- Additive: new module `jobs.rs`, new `components/Jobs/`; existing events (`retranscription-progress`, `enhance-status-changed`, `hook-status-changed`, download events) keep firing unchanged.
- No new crate or npm dependency.
- Do not build on `audio_v2/`, `lib_old_complex.rs`, `audio/stt.rs`, `whisper_engine/parallel_processor.rs`.
- Registry is in memory only; Recent capped at 10 terminal jobs.
- ETA: `None` until **both** 15 s elapsed and 10 % reached; EMA α = 0.3; always `None` without a percentage.
- `cancel_job` on an unknown or terminal job returns `Ok(())`.
- Concurrency unchanged: `ENHANCE_QUEUE` still serializes enhance.
- In worktree sessions use `/usr/bin/git` (RTK rewrite is refused).
- Rust tests: run from `frontend/src-tauri`.

## File map

| File | Change |
|---|---|
| `frontend/src-tauri/src/jobs.rs` | Create: types, `Registry`, globals, Tauri commands, tests |
| `frontend/src-tauri/src/lib.rs` | `pub mod jobs;`, `jobs::init` in setup, register 4 commands |
| `frontend/src-tauri/src/audio/retranscription.rs` | Enhance job lifecycle, queued cancel, steps, progress |
| `frontend/src-tauri/src/audio/post_enhance_hook.rs` | Own process group, register pid, report hook status to jobs |
| `frontend/src-tauri/src/summary/commands.rs`, `summary/service.rs` | Summary job start/finish |
| `frontend/src-tauri/src/whisper_engine/commands.rs`, `parakeet_engine/commands.rs`, `summary/summary_engine/commands.rs` | Download jobs |
| `frontend/src/hooks/useJobs.ts` | Create |
| `frontend/src/components/Jobs/JobsButton.tsx` | Create (button + panel in one file) |
| `frontend/src/components/TitleBar.tsx` | Mount `JobsButton` on the right |

---

### Task 1: Job registry core

**Files:**
- Create: `frontend/src-tauri/src/jobs.rs`
- Modify: `frontend/src-tauri/src/lib.rs` (add `pub mod jobs;` after `pub mod database;`, line ~45)

**Interfaces:**
- Produces (used by every later task):
  - `pub enum JobKind { Enhance, Summary, Download }`, `pub enum JobState { Queued, Running, Done, Failed, Cancelled }` (serde snake_case), `JobState::is_terminal()`
  - `pub enum Control { Enhance { folder, language, model, provider }, Summary { started_at }, WhisperDownload, ParakeetDownload, BuiltinDownload }`
  - `pub enum Outcome { Done, Failed(String), Cancelled }`
  - `pub const STEP_TRANSCRIBE: &str = "Transcribe"; STEP_TITLE = "Title/tags"; STEP_WIKI = "Wiki";`
  - Free fns: `init(AppHandle)`, `add(id, kind, meeting_id: Option<&str>, title, control, running: bool)`, `start(id)`, `progress(id, pct: u8, detail: Option<String>)`, `enhance_progress(meeting_id, pct, detail)`, `step(id, name, state, error: Option<String>)`, `finish(id, Outcome)`, `is_cancel_requested(id) -> bool`, `request_cancel(id) -> Option<Job>`, `hook_status(meeting_id, status: &str)`, `set_hook_pid(meeting_id, Option<u32>)`, `list() -> Vec<Job>`, `take(id) -> Option<Job>`, `clear_recent()`

- [ ] **Step 1: Write the module with its failing tests**

Create `frontend/src-tauri/src/jobs.rs`:

```rust
//! Job queue panel (docs/superpowers/specs/2026-10-28-job-queue-panel-design.md): one
//! in-memory list of background jobs that the title-bar panel shows, cancels and retries.
//! Every change emits `jobs-changed` with the whole list; it is small, so no diffing.

use chrono::{DateTime, Utc};
use once_cell::sync::{Lazy, OnceCell};
use serde::Serialize;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const RECENT_CAP: usize = 10;
const ETA_MIN_ELAPSED: Duration = Duration::from_secs(15);
const ETA_MIN_PERCENT: u8 = 10;
const ETA_ALPHA: f64 = 0.3;

pub const STEP_TRANSCRIBE: &str = "Transcribe";
pub const STEP_TITLE: &str = "Title/tags";
pub const STEP_WIKI: &str = "Wiki";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Enhance,
    Summary,
    Download,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
    pub name: String,
    pub state: JobState,
    pub error: Option<String>,
}

/// How to stop or restart a job. Never sent to the UI.
#[derive(Debug, Clone)]
pub enum Control {
    Enhance { folder: String, language: Option<String>, model: Option<String>, provider: Option<String> },
    Summary { started_at: DateTime<Utc> },
    WhisperDownload,
    ParakeetDownload,
    BuiltinDownload,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Done,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: String,
    pub kind: JobKind,
    pub meeting_id: Option<String>,
    /// model name for downloads; empty for meeting jobs (the UI shows the meeting title)
    pub title: String,
    pub state: JobState,
    pub steps: Vec<Step>,
    pub progress: Option<u8>,
    pub detail: Option<String>,
    pub eta_secs: Option<u32>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    #[serde(skip)]
    pub control: Control,
    #[serde(skip)]
    pub cancel_requested: bool,
    #[serde(skip)]
    pub hook_pid: Option<u32>,
    #[serde(skip)]
    started: Option<Instant>,
    #[serde(skip)]
    last: Option<(Instant, u8)>,
    #[serde(skip)]
    rate: Option<f64>,
}

#[derive(Default)]
pub struct Registry {
    jobs: Vec<Job>,
}

impl Registry {
    pub fn add(&mut self, id: &str, kind: JobKind, meeting_id: Option<&str>, title: &str, control: Control, running: bool, now: Instant) {
        self.jobs.push(Job {
            id: id.to_string(),
            kind,
            meeting_id: meeting_id.map(str::to_string),
            title: title.to_string(),
            state: JobState::Queued,
            steps: Vec::new(),
            progress: None,
            detail: None,
            eta_secs: None,
            started_at: None,
            finished_at: None,
            error: None,
            control,
            cancel_requested: false,
            hook_pid: None,
            started: None,
            last: None,
            rate: None,
        });
        if running {
            self.start(id, now);
        }
    }

    fn get(&mut self, id: &str) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id && !j.state.is_terminal())
    }

    /// The oldest live enhance job of a meeting: the running one comes before any re-queued one.
    fn enhance_of(&mut self, meeting_id: &str) -> Option<&mut Job> {
        self.jobs
            .iter_mut()
            .find(|j| j.kind == JobKind::Enhance && !j.state.is_terminal() && j.meeting_id.as_deref() == Some(meeting_id))
    }

    pub fn start(&mut self, id: &str, now: Instant) {
        if let Some(j) = self.get(id) {
            j.state = JobState::Running;
            j.started_at = Some(Utc::now());
            j.started = Some(now);
        }
    }

    /// Returns whether the visible percentage changed (callers emit only then).
    pub fn progress(&mut self, id: &str, pct: u8, detail: Option<String>, now: Instant) -> bool {
        let Some(j) = self.get(id) else { return false };
        if j.state != JobState::Running {
            return false;
        }
        let pct = pct.min(100);
        let changed = j.progress != Some(pct) || (detail.is_some() && j.detail != detail);
        let started = *j.started.get_or_insert(now);
        if let Some((t, p)) = j.last {
            let dt = now.duration_since(t).as_secs_f64();
            if pct > p && dt > 0.0 {
                let inst = (pct - p) as f64 / dt;
                j.rate = Some(j.rate.map_or(inst, |r| ETA_ALPHA * inst + (1.0 - ETA_ALPHA) * r));
            }
        }
        j.last = Some((now, pct));
        let warm = now.duration_since(started) >= ETA_MIN_ELAPSED && pct >= ETA_MIN_PERCENT;
        j.eta_secs = match j.rate {
            Some(r) if warm && r > 0.0 && pct < 100 => Some(((100 - pct) as f64 / r).round() as u32),
            _ => None,
        };
        j.progress = Some(pct);
        if detail.is_some() {
            j.detail = detail;
        }
        changed
    }

    pub fn enhance_progress(&mut self, meeting_id: &str, pct: u8, detail: Option<String>, now: Instant) -> bool {
        match self.enhance_of(meeting_id).map(|j| j.id.clone()) {
            Some(id) => self.progress(&id, pct, detail, now),
            None => false,
        }
    }

    pub fn step(&mut self, id: &str, name: &str, state: JobState, error: Option<String>) {
        let Some(j) = self.get(id) else { return };
        match j.steps.iter_mut().find(|s| s.name == name) {
            Some(s) => {
                s.state = state;
                s.error = error;
            }
            None => j.steps.push(Step { name: name.to_string(), state, error }),
        }
    }

    /// Idempotent: a terminal job is left as is. A failure after a cancel request is a cancel.
    pub fn finish(&mut self, id: &str, outcome: Outcome) {
        let Some(j) = self.get(id) else { return };
        let outcome = match outcome {
            Outcome::Failed(_) if j.cancel_requested => Outcome::Cancelled,
            o => o,
        };
        j.state = match outcome {
            Outcome::Done => JobState::Done,
            Outcome::Failed(e) => {
                j.error = Some(e.lines().next().unwrap_or("").to_string());
                JobState::Failed
            }
            Outcome::Cancelled => JobState::Cancelled,
        };
        j.finished_at = Some(Utc::now());
        j.eta_secs = None;
        j.hook_pid = None;
        self.trim();
    }

    pub fn is_cancel_requested(&self, id: &str) -> bool {
        self.jobs.iter().any(|j| j.id == id && j.cancel_requested)
    }

    /// Marks the cancel; a queued job ends at once. Returns the job as it was before, or
    /// `None` when unknown or already terminal (nothing to stop).
    pub fn request_cancel(&mut self, id: &str) -> Option<Job> {
        let j = self.get(id)?;
        let before = j.clone();
        j.cancel_requested = true;
        if before.state == JobState::Queued {
            self.finish(id, Outcome::Cancelled);
        }
        Some(before)
    }

    /// Wiki step of the meeting's live enhance job, driven by `hook-status-changed` values.
    /// A hook run with no enhance job (the manual "Run hook" action) is ignored.
    pub fn hook_status(&mut self, meeting_id: &str, status: &str) {
        let Some(j) = self.enhance_of(meeting_id) else { return };
        let id = j.id.clone();
        let cancelled = j.cancel_requested;
        match status {
            "running" => self.step(&id, STEP_WIKI, JobState::Running, None),
            "done" => {
                self.step(&id, STEP_WIKI, JobState::Done, None);
                self.finish(&id, Outcome::Done);
            }
            "failed" if cancelled => {
                self.step(&id, STEP_WIKI, JobState::Cancelled, None);
                self.finish(&id, Outcome::Cancelled);
            }
            "failed" => {
                // spec 5: the transcript is the success criterion; the step carries the failure
                self.step(&id, STEP_WIKI, JobState::Failed, Some("see hook.log".to_string()));
                self.finish(&id, Outcome::Done);
            }
            _ => {}
        }
    }

    pub fn set_hook_pid(&mut self, meeting_id: &str, pid: Option<u32>) {
        if let Some(j) = self.enhance_of(meeting_id) {
            j.hook_pid = pid;
        }
    }

    /// Removes a terminal job (retry replaces it with a new one).
    pub fn take(&mut self, id: &str) -> Option<Job> {
        let i = self.jobs.iter().position(|j| j.id == id && j.state.is_terminal())?;
        Some(self.jobs.remove(i))
    }

    pub fn clear_recent(&mut self) {
        self.jobs.retain(|j| !j.state.is_terminal());
    }

    fn trim(&mut self) {
        let terminal = self.jobs.iter().filter(|j| j.state.is_terminal()).count();
        let mut drop = terminal.saturating_sub(RECENT_CAP);
        // finish order is not insertion order: drop the oldest finished first
        while drop > 0 {
            let oldest = self
                .jobs
                .iter()
                .enumerate()
                .filter(|(_, j)| j.state.is_terminal())
                .min_by_key(|(_, j)| j.finished_at)
                .map(|(i, _)| i);
            match oldest {
                Some(i) => {
                    self.jobs.remove(i);
                    drop -= 1;
                }
                None => break,
            }
        }
    }

    pub fn snapshot(&self) -> Vec<Job> {
        self.jobs.clone()
    }
}

static REGISTRY: Lazy<StdMutex<Registry>> = Lazy::new(Default::default);
static APP: OnceCell<AppHandle> = OnceCell::new();

pub fn init(app: AppHandle) {
    let _ = APP.set(app);
}

/// Runs `f` on the registry and emits the new list when `f` says something changed.
fn update<T>(f: impl FnOnce(&mut Registry) -> (T, bool)) -> T {
    let (out, snapshot) = {
        let mut r = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        let (out, changed) = f(&mut r);
        (out, changed.then(|| r.snapshot()))
    };
    if let (Some(list), Some(app)) = (snapshot, APP.get()) {
        let _ = app.emit("jobs-changed", list);
    }
    out
}

pub fn add(id: &str, kind: JobKind, meeting_id: Option<&str>, title: &str, control: Control, running: bool) {
    update(|r| (r.add(id, kind, meeting_id, title, control, running, Instant::now()), true))
}
pub fn start(id: &str) {
    update(|r| (r.start(id, Instant::now()), true))
}
pub fn progress(id: &str, pct: u8, detail: Option<String>) {
    update(|r| ((), r.progress(id, pct, detail, Instant::now())))
}
pub fn enhance_progress(meeting_id: &str, pct: u8, detail: Option<String>) {
    update(|r| ((), r.enhance_progress(meeting_id, pct, detail, Instant::now())))
}
pub fn step(id: &str, name: &str, state: JobState, error: Option<String>) {
    update(|r| (r.step(id, name, state, error), true))
}
pub fn finish(id: &str, outcome: Outcome) {
    update(|r| (r.finish(id, outcome), true))
}
pub fn is_cancel_requested(id: &str) -> bool {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).is_cancel_requested(id)
}
pub fn request_cancel(id: &str) -> Option<Job> {
    update(|r| {
        let j = r.request_cancel(id);
        let changed = j.is_some();
        (j, changed)
    })
}
pub fn hook_status(meeting_id: &str, status: &str) {
    update(|r| (r.hook_status(meeting_id, status), true))
}
pub fn set_hook_pid(meeting_id: &str, pid: Option<u32>) {
    update(|r| (r.set_hook_pid(meeting_id, pid), false))
}
pub fn list() -> Vec<Job> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).snapshot()
}
pub fn take(id: &str) -> Option<Job> {
    update(|r| {
        let j = r.take(id);
        let changed = j.is_some();
        (j, changed)
    })
}
pub fn clear_recent() {
    update(|r| (r.clear_recent(), true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enhance() -> Control {
        Control::Enhance { folder: "/f".into(), language: None, model: None, provider: None }
    }

    #[test]
    fn queued_runs_then_finishes() {
        let mut r = Registry::default();
        let t = Instant::now();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), false, t);
        assert_eq!(r.snapshot()[0].state, JobState::Queued);
        r.start("a", t);
        assert_eq!(r.snapshot()[0].state, JobState::Running);
        r.finish("a", Outcome::Done);
        assert_eq!(r.snapshot()[0].state, JobState::Done);
        r.finish("a", Outcome::Failed("late".into()));
        assert_eq!(r.snapshot()[0].state, JobState::Done, "finish is idempotent");
    }

    #[test]
    fn cancel_queued_ends_now_and_terminal_cancel_is_a_noop() {
        let mut r = Registry::default();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), false, Instant::now());
        assert_eq!(r.request_cancel("a").unwrap().state, JobState::Queued);
        assert_eq!(r.snapshot()[0].state, JobState::Cancelled);
        assert!(r.request_cancel("a").is_none());
        assert!(r.request_cancel("unknown").is_none());
    }

    #[test]
    fn failure_after_cancel_request_is_cancelled() {
        let mut r = Registry::default();
        r.add("a", JobKind::Summary, Some("m"), "", Control::Summary { started_at: Utc::now() }, true, Instant::now());
        r.request_cancel("a");
        r.finish("a", Outcome::Failed("Summary generation was cancelled".into()));
        assert_eq!(r.snapshot()[0].state, JobState::Cancelled);
    }

    #[test]
    fn recent_is_capped_at_ten() {
        let mut r = Registry::default();
        for i in 0..12 {
            let id = i.to_string();
            r.add(&id, JobKind::Download, None, "m", Control::WhisperDownload, true, Instant::now());
            r.finish(&id, Outcome::Done);
        }
        r.add("live", JobKind::Download, None, "m", Control::WhisperDownload, true, Instant::now());
        let list = r.snapshot();
        assert_eq!(list.iter().filter(|j| j.state.is_terminal()).count(), 10);
        assert!(list.iter().all(|j| j.id != "0" && j.id != "1"), "oldest dropped first");
        assert!(list.iter().any(|j| j.id == "live"));
    }

    #[test]
    fn eta_waits_for_15s_and_10_percent_then_smooths() {
        let mut r = Registry::default();
        let t0 = Instant::now();
        r.add("a", JobKind::Download, None, "m", Control::WhisperDownload, true, t0);
        r.progress("a", 20, None, t0 + Duration::from_secs(5));
        assert_eq!(r.snapshot()[0].eta_secs, None, "too early");
        r.progress("a", 30, None, t0 + Duration::from_secs(15));
        // rate 1 %/s, 70 % left
        assert_eq!(r.snapshot()[0].eta_secs, Some(70));
        r.progress("a", 50, None, t0 + Duration::from_secs(25));
        // inst 2 %/s, ema 0.3*2 + 0.7*1 = 1.3 %/s, 50 % left -> 38 s
        assert_eq!(r.snapshot()[0].eta_secs, Some(38));
    }

    #[test]
    fn eta_needs_ten_percent_even_when_slow() {
        let mut r = Registry::default();
        let t0 = Instant::now();
        r.add("a", JobKind::Download, None, "m", Control::WhisperDownload, true, t0);
        r.progress("a", 2, None, t0 + Duration::from_secs(10));
        r.progress("a", 5, None, t0 + Duration::from_secs(20));
        assert_eq!(r.snapshot()[0].eta_secs, None);
    }

    #[test]
    fn hook_status_drives_the_wiki_step_of_the_running_enhance() {
        let mut r = Registry::default();
        let t = Instant::now();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), true, t);
        r.add("b", JobKind::Enhance, Some("m"), "", enhance(), false, t);
        r.hook_status("m", "running");
        r.hook_status("m", "failed");
        let a = &r.snapshot()[0];
        assert_eq!(a.state, JobState::Done, "a failed wiki step does not fail the enhance");
        assert_eq!(a.steps[0], Step { name: STEP_WIKI.into(), state: JobState::Failed, error: Some("see hook.log".into()) });
        assert_eq!(r.snapshot()[1].state, JobState::Queued, "the re-queued job is untouched");
    }

    #[test]
    fn hook_failure_after_cancel_is_cancelled() {
        let mut r = Registry::default();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), true, Instant::now());
        r.hook_status("m", "running");
        r.request_cancel("a");
        r.hook_status("m", "failed");
        assert_eq!(r.snapshot()[0].state, JobState::Cancelled);
    }

    #[test]
    fn enhance_progress_ignores_queued_jobs() {
        let mut r = Registry::default();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), false, Instant::now());
        assert!(!r.enhance_progress("m", 0, Some("Waiting".into()), Instant::now()));
        assert_eq!(r.snapshot()[0].progress, None);
    }
}
```

Add to `lib.rs` after `pub mod database;`:

```rust
pub mod jobs; // job queue panel
```

- [ ] **Step 2: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib jobs::`
Expected: 9 passed. If the ETA arithmetic test fails by one, check rounding uses `.round()` (38.46 → 38).

- [ ] **Step 3: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/jobs.rs frontend/src-tauri/src/lib.rs
/usr/bin/git commit -m "feat(jobs): in-memory job registry with ETA and recent cap"
```

---

### Task 2: Tauri commands and enhance wiring

**Files:**
- Modify: `frontend/src-tauri/src/jobs.rs` (append commands)
- Modify: `frontend/src-tauri/src/lib.rs` (setup ~line 495, `generate_handler!` ~line 808)
- Modify: `frontend/src-tauri/src/audio/retranscription.rs` (`start_retranscription` lines 134-265, `emit_progress` ~line 781)

**Interfaces:**
- Consumes: Task 1 free functions and types.
- Produces: commands `list_jobs`, `cancel_job { id }`, `retry_job { id }`, `clear_recent_jobs`. `cancel_job` handles every `Control` variant; Tasks 3 and 4 only feed state.

- [ ] **Step 1: Append the commands to `jobs.rs`** (above `#[cfg(test)]`)

```rust
#[tauri::command]
pub fn list_jobs() -> Vec<Job> {
    list()
}

#[tauri::command]
pub fn clear_recent_jobs() {
    clear_recent()
}

/// spec 3.1. Unknown or finished jobs: nothing to stop, so Ok.
#[tauri::command]
pub async fn cancel_job(app: AppHandle, id: String) -> Result<(), String> {
    use tauri::Manager;
    let Some(job) = request_cancel(&id) else { return Ok(()) };
    if job.state == JobState::Queued {
        return Ok(()); // already ended by request_cancel; start_retranscription checks the flag
    }
    match &job.control {
        Control::Enhance { .. } => {
            let running = |name: &str| job.steps.iter().any(|s| s.name == name && s.state == JobState::Running);
            if let Some(pid) = job.hook_pid {
                kill_group(pid);
            } else if running(STEP_TRANSCRIBE) {
                crate::audio::retranscription::cancel_retranscription(job.meeting_id.as_deref())?;
            }
            // title step: not interruptible; start_retranscription skips the wiki step after it
        }
        Control::Summary { started_at } => {
            let process_id = started_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
            let meeting_id = job.meeting_id.clone().unwrap_or_default();
            crate::summary::commands::api_cancel_summary(app.clone(), app.state(), meeting_id, process_id).await?;
        }
        Control::WhisperDownload => {
            crate::whisper_engine::commands::whisper_cancel_download(job.title.clone()).await?;
        }
        Control::ParakeetDownload => {
            crate::parakeet_engine::commands::parakeet_cancel_download(job.title.clone()).await?;
        }
        Control::BuiltinDownload => {
            crate::summary::summary_engine::commands::builtin_ai_cancel_download(app.clone(), app.state(), job.title.clone()).await?;
            finish(&id, Outcome::Cancelled); // its download error path is silent on cancel
        }
    }
    Ok(())
}

/// Kills the hook's whole process group (Claude Code and its children). The hook is
/// spawned with `process_group(0)`, so its pid is the group id.
fn kill_group(pid: u32) {
    let _ = std::process::Command::new("kill").args(["-TERM", &format!("-{}", pid)]).status();
}

/// Starts the job again with the arguments it first ran with. Summary needs the meeting
/// page (transcript, template, model), so it is not retried from here.
#[tauri::command]
pub async fn retry_job(app: AppHandle, id: String) -> Result<(), String> {
    use tauri::Manager;
    let job = list().into_iter().find(|j| j.id == id && j.state.is_terminal()).ok_or("Job not found")?;
    if matches!(job.control, Control::Summary { .. }) {
        return Err("Open the meeting to generate the summary again.".to_string());
    }
    take(&id);
    let model = job.title.clone();
    tauri::async_runtime::spawn(async move {
        let result = match job.control {
            Control::Enhance { folder, language, model, provider } => {
                let meeting_id = job.meeting_id.unwrap_or_default();
                crate::audio::retranscription::start_retranscription(app, meeting_id, folder, language, model, provider)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            Control::WhisperDownload => crate::whisper_engine::commands::whisper_download_model(app, model).await,
            Control::ParakeetDownload => crate::parakeet_engine::commands::parakeet_download_model(app, model).await,
            Control::BuiltinDownload => {
                let state = app.state::<crate::summary::summary_engine::commands::ModelManagerState>();
                crate::summary::summary_engine::commands::builtin_ai_download_model(app.clone(), state, model).await
            }
            Control::Summary { .. } => Ok(()),
        };
        if let Err(e) = result {
            log::warn!("jobs: retry failed: {}", e);
        }
    });
    Ok(())
}
```

If `ModelManagerState` lives elsewhere, use the path `builtin_ai_cancel_download`'s `State<'_, ModelManagerState>` imports in `summary/summary_engine/commands.rs`. If the borrow of `app.state()` inside the spawned task fails to compile, bind `let app2 = app.clone();` and call `app2.state::<…>()` inside the block.

- [ ] **Step 2: Register in `lib.rs`**

In `.setup(|_app| {`, first line of the closure:

```rust
            jobs::init(_app.handle().clone());
```

In `tauri::generate_handler![`, next to `summary::commands::api_cancel_summary,`:

```rust
            jobs::list_jobs,
            jobs::cancel_job,
            jobs::retry_job,
            jobs::clear_recent_jobs,
```

- [ ] **Step 3: Wire the enhance lifecycle in `start_retranscription`**

Add at the top of `retranscription.rs`:

```rust
use crate::jobs::{self, Control, JobKind, JobState, Outcome, STEP_TITLE, STEP_TRANSCRIBE};
```

Replace the start of `start_retranscription` up to the queue lock (lines ~142-150) with:

```rust
    let queued_folder = PathBuf::from(&meeting_folder_path);
    let provider_name = provider.clone().unwrap_or_else(|| "whisper".to_string());
    let model_name = model.clone().unwrap_or_else(|| "configured".to_string());
    let job_id = uuid::Uuid::new_v4().to_string();
    let control = Control::Enhance {
        folder: meeting_folder_path.clone(),
        language: language.clone(),
        model: model.clone(),
        provider: provider.clone(),
    };
    jobs::add(&job_id, JobKind::Enhance, Some(&meeting_id), "", control, false);

    set_enhance_status(&app, &meeting_id, &queued_folder, &provider_name, &model_name, STATUS_QUEUED, None);
    if ENHANCE_QUEUE.try_lock().is_err() {
        emit_progress(&app, &meeting_id, "queued", 0, "Waiting for the current enhance job to finish...");
    }
    let _queue_slot = ENHANCE_QUEUE.lock().await;
    // job queue panel: a queued job cancelled while it waited never transcribes
    if jobs::is_cancel_requested(&job_id) {
        set_enhance_status(&app, &meeting_id, &queued_folder, &provider_name, &model_name, STATUS_FAILED, Some("Cancelled"));
        return Err(anyhow!("Retranscription cancelled"));
    }
    jobs::start(&job_id);
    jobs::step(&job_id, STEP_TRANSCRIBE, JobState::Running, None);
```

Replace the guard acquisition line inside the `let result = {` block:

```rust
        let _guard = match RetranscriptionGuard::acquire(&meeting_id) {
            Ok(g) => g,
            Err(e) => {
                jobs::finish(&job_id, Outcome::Failed(e.clone()));
                return Err(anyhow!(e));
            }
        };
```

In `match &result {` → `Ok(res) => {`, first line:

```rust
            jobs::step(&job_id, STEP_TRANSCRIBE, JobState::Done, None);
```

Replace the title/tags block:

```rust
            if res.segments_count > 0 && matches!(&prefs, Ok(p) if p.auto_title_tags) {
                jobs::step(&job_id, STEP_TITLE, JobState::Running, None);
                match super::meeting_tagging::run_after_enhance(&app, &meeting_id).await {
                    Ok(msg) => {
                        info!("Title and tags for {}: {}", meeting_id, msg);
                        jobs::step(&job_id, STEP_TITLE, JobState::Done, None);
                    }
                    Err(e) => {
                        warn!("Title and tags skipped for {}: {}", meeting_id, e);
                        jobs::step(&job_id, STEP_TITLE, JobState::Failed, Some(e.to_string()));
                    }
                }
            }
            // spec 3.1: a cancel during the title step skips the wiki step
            if jobs::is_cancel_requested(&job_id) {
                jobs::finish(&job_id, Outcome::Cancelled);
                return result;
            }
```

Replace the hook block (`if res.segments_count > 0 { match prefs { … } }`) with:

```rust
            // the wiki step reports through hook-status-changed (jobs::hook_status), which also
            // ends the job; every other path ends it here
            let mut hook_started = false;
            if res.segments_count > 0 {
                match prefs {
                    Ok(prefs) if prefs.hook_auto_run => {
                        let ctx = super::post_enhance_hook::read_context(&folder, &meeting_id);
                        match super::post_enhance_hook::dispatch(&app, &prefs, &ctx, false) {
                            Ok(kind) => {
                                hook_started = true;
                                info!("Post-enhance hook ({:?}) dispatched for {}", kind, meeting_id)
                            }
                            Err(e) => warn!("Post-enhance hook did not start for {}: {}", meeting_id, e),
                        }
                    }
                    Ok(_) => info!("Post-enhance hook not auto-run for {} (hook_auto_run off)", meeting_id),
                    Err(e) => warn!("Post-enhance hook skipped for {}: could not load preferences: {}", meeting_id, e),
                }
            }
            if !hook_started {
                jobs::finish(&job_id, Outcome::Done);
            }
```

`dispatch` returning `Ok` always ends with a `done` or `failed` notify (the reaper's `on_exit`, or the immediate `transition(DONE)` on `hook.done`), so the job cannot stay running. The `Err` paths finish here; `finish` is idempotent if the notify already ended it.

In `Err(e) => {` of the same match, first lines:

```rust
            jobs::step(&job_id, STEP_TRANSCRIBE, JobState::Failed, Some(e.to_string()));
            let outcome = if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) { Outcome::Cancelled } else { Outcome::Failed(e.to_string()) };
            jobs::finish(&job_id, outcome);
```

The flag check catches a cancel from the old dialog path too.

- [ ] **Step 4: Feed enhance progress**

In `emit_progress`, after the `app.emit(...)` call:

```rust
    jobs::enhance_progress(meeting_id, progress.min(100) as u8, Some(message.to_string()));
```

- [ ] **Step 5: Build and run the tests**

Run: `cd frontend/src-tauri && cargo check && cargo test --lib jobs:: && cargo test --lib retranscription`
Expected: compiles; all pass.

- [ ] **Step 6: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/jobs.rs frontend/src-tauri/src/lib.rs frontend/src-tauri/src/audio/retranscription.rs
/usr/bin/git commit -m "feat(jobs): enhance jobs in the registry, queued cancel, list/cancel/retry commands"
```

---

### Task 3: Wiki hook process group, pid and status

**Files:**
- Modify: `frontend/src-tauri/src/audio/post_enhance_hook.rs` (`spawn_logged` lines 40-71, `dispatch` lines 124-136, tests)

**Interfaces:**
- Consumes: `jobs::set_hook_pid(meeting_id, Option<u32>)`, `jobs::hook_status(meeting_id, &str)`.
- Produces: hook children run in their own process group, so `jobs::kill_group(pid)` (Task 2) stops Claude Code and its children.

- [ ] **Step 1: Write the failing test** (in `mod tests` of `post_enhance_hook.rs`)

```rust
    #[test]
    fn killing_the_process_group_stops_the_hook_and_its_children() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = read_context(dir.path(), "m-kill");
        let (tx, rx) = std::sync::mpsc::channel();
        // the hook shell runs in the meeting folder; $$ is its pid (the group id once grouped),
        // the backgrounded sleep is a child that must die with the group
        spawn("echo $$ > hook.pid; sleep 60 & echo $! > child.pid; wait", &ctx, Some(Box::new(move |ok| tx.send(ok).unwrap()))).unwrap();
        let child_file = dir.path().join("child.pid");
        assert!(wait_until(|| child_file.exists()), "hook started");
        let read = |f: &str| std::fs::read_to_string(dir.path().join(f)).unwrap().trim().to_string();
        std::process::Command::new("kill").args(["-TERM", &format!("-{}", read("hook.pid"))]).status().unwrap();
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(false), "hook exits non-zero");
        let alive = std::process::Command::new("kill").args(["-0", &read("child.pid")]).status().unwrap().success();
        assert!(!alive, "the backgrounded child is gone too");
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cd frontend/src-tauri && cargo test --lib post_enhance_hook::tests::killing`
Expected: FAIL. Without its own process group, `kill -TERM -<pid>` finds no group with that id, the hook keeps running, and `recv_timeout` times out.

- [ ] **Step 3: Implement in `spawn_logged`**

Before `let mut child = command`:

```rust
    // own process group: cancelling from the job panel kills Claude Code and its children together
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
```

After the `info!("Post-enhance hook started (pid {}) …")` line:

```rust
    crate::jobs::set_hook_pid(meeting_id, Some(child.id()));
```

Do not clear the pid in the reaper thread: `jobs::finish` already clears it when the job ends. Clearing it here could race with a re-queued enhance of the same meeting.

- [ ] **Step 4: Report hook status to jobs in `dispatch`**

Replace the `notify` closure in `dispatch`:

```rust
    let notify: Notify = std::sync::Arc::new(move |id: &str, status: &str| {
        crate::jobs::hook_status(id, status);
        let _ = app.emit("hook-status-changed", serde_json::json!({ "meeting_id": id, "status": status }));
    });
```

- [ ] **Step 5: Run the hook tests**

Run: `cd frontend/src-tauri && cargo test --lib post_enhance_hook && cargo test --lib wiki_automation`
Expected: all pass, including the new test.

- [ ] **Step 6: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/audio/post_enhance_hook.rs
/usr/bin/git commit -m "feat(jobs): hook runs in its own process group and reports to the job registry"
```

---

### Task 4: Summary and download jobs

**Files:**
- Modify: `frontend/src-tauri/src/summary/commands.rs` (`api_process_transcript`, after `register_cancellation_token` ~line 527)
- Modify: `frontend/src-tauri/src/summary/service.rs` (`process_transcript_background` result match ~lines 540-597)
- Modify: `frontend/src-tauri/src/whisper_engine/commands.rs` (`whisper_download_model` lines 422-492)
- Modify: `frontend/src-tauri/src/parakeet_engine/commands.rs` (`parakeet_download_model` lines 379-480)
- Modify: `frontend/src-tauri/src/summary/summary_engine/commands.rs` (`builtin_ai_download_model` lines 131-213)

**Interfaces:**
- Consumes: `jobs::add`, `jobs::progress`, `jobs::finish`, `Control::{Summary, WhisperDownload, ParakeetDownload, BuiltinDownload}`.
- Summary job id is derived from both ends: `jobs::summary_id(meeting_id, started_at)`.

- [ ] **Step 1: Add the summary id helper to `jobs.rs`** (next to `add`)

```rust
/// Summary jobs are started and finished in different functions; both derive the id.
pub fn summary_id(meeting_id: &str, started_at: DateTime<Utc>) -> String {
    format!("summary:{}:{}", meeting_id, started_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
}
```

- [ ] **Step 2: Summary start, in `api_process_transcript`** right after `let cancellation_token = SummaryService::register_cancellation_token(&m_id, started_at);`

```rust
    crate::jobs::add(
        &crate::jobs::summary_id(&m_id, started_at),
        crate::jobs::JobKind::Summary,
        Some(&m_id),
        "",
        crate::jobs::Control::Summary { started_at },
        true,
    );
```

- [ ] **Step 3: Summary finish, in `process_transcript_background`**

Just before `Self::cleanup_cancellation_token(&meeting_id, started_at);` at the end, the result match has three arms. Add `let job = crate::jobs::summary_id(&meeting_id, started_at);` above the match, then:
- at the end of the `Ok(...)` arm: `crate::jobs::finish(&job, crate::jobs::Outcome::Done);`
- in `Err(error) if cancellation_token.is_cancelled()`: `crate::jobs::finish(&job, crate::jobs::Outcome::Cancelled);`
- in `Err(error)`: `crate::jobs::finish(&job, crate::jobs::Outcome::Failed(error.to_string()));`

There are three early `return`s:
- Lines ~367 (LLM config) and ~438 (template) go through `Self::fail_and_cleanup(&pool, &meeting_id, started_at, &msg)`. Add one line at the top of `fail_and_cleanup` so both are covered:
  ```rust
  crate::jobs::finish(&crate::jobs::summary_id(meeting_id, started_at), crate::jobs::Outcome::Failed(error.to_string()));
  ```
  Use that function's own parameter names for the meeting id, start time and error.
- Line ~529 (`Err(error) => { Self::update_process_failed(...); Self::cleanup_cancellation_token(...); return; }`) needs its own line before `return;`:
  ```rust
  crate::jobs::finish(&crate::jobs::summary_id(&meeting_id, started_at), crate::jobs::Outcome::Failed(error.to_string()));
  ```

- [ ] **Step 4: Whisper download**

At the start of `whisper_download_model`, inside `if let Some(engine) = engine {`:

```rust
        let job_id = uuid::Uuid::new_v4().to_string();
        crate::jobs::add(&job_id, crate::jobs::JobKind::Download, None, &model_name, crate::jobs::Control::WhisperDownload, true);
```

Inside the progress callback (clone `job_id` into it as `job_id_cb`):

```rust
            crate::jobs::progress(&job_id_cb, progress, None);
```

In the result match: `Ok(())` arm → `crate::jobs::finish(&job_id, crate::jobs::Outcome::Done);`. Cancelled arm → `crate::jobs::finish(&job_id, crate::jobs::Outcome::Cancelled);`. Error arm → `crate::jobs::finish(&job_id, crate::jobs::Outcome::Failed(e.to_string()));`.

- [ ] **Step 5: Parakeet download**

Same pattern in `parakeet_download_model`, with `Control::ParakeetDownload`. The progress callback passes the size:

```rust
            crate::jobs::progress(
                &job_id_cb,
                progress.percent as u8,
                Some(format!("{:.0} / {:.0} MB", progress.downloaded_mb, progress.total_mb)),
            );
```

Finish Done, Cancelled (the `is_download_cancelled` arm) and Failed in the three result arms.

- [ ] **Step 6: Built-in AI download**

Same pattern in `builtin_ai_download_model`, with `Control::BuiltinDownload`:

```rust
            crate::jobs::progress(&job_id_cb, progress.percent as u8, Some(format!("{:.0} / {:.0} MB", progress.downloaded_mb, progress.total_mb)));
```

Result: `Ok` → Done; `Err` with `error_msg.starts_with("CANCELLED:")` → Cancelled; other `Err` → Failed(error_msg). If `progress.percent` is a float, cast with `as u8`.

- [ ] **Step 7: Build and run the tests**

Run: `cd frontend/src-tauri && cargo check && cargo test --lib summary && cargo test --lib jobs::`
Expected: compiles; all pass (`summary::service` cancellation test included).

- [ ] **Step 8: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/jobs.rs frontend/src-tauri/src/summary frontend/src-tauri/src/whisper_engine/commands.rs frontend/src-tauri/src/parakeet_engine/commands.rs
/usr/bin/git commit -m "feat(jobs): summary and model download jobs"
```

---

### Task 5: Frontend hook, button and panel

**Files:**
- Create: `frontend/src/hooks/useJobs.ts`
- Create: `frontend/src/components/Jobs/JobsButton.tsx`
- Modify: `frontend/src/components/TitleBar.tsx` (import, and mount on the right of the title strip)

**Interfaces:**
- Consumes: commands `list_jobs`, `cancel_job({ id })`, `retry_job({ id })`, `clear_recent_jobs`; event `jobs-changed` (payload `Job[]`, serde shape from Task 1); `useSidebar().meetings` (`{ id, title }`).

- [ ] **Step 1: `useJobs.ts`**

```ts
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type JobState = 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
export interface Job {
  id: string;
  kind: 'enhance' | 'summary' | 'download';
  meeting_id: string | null;
  title: string;
  state: JobState;
  steps: { name: string; state: JobState; error: string | null }[];
  progress: number | null;
  detail: string | null;
  eta_secs: number | null;
  started_at: string | null;
  finished_at: string | null;
  error: string | null;
}

export const isTerminal = (s: JobState) => s === 'done' || s === 'failed' || s === 'cancelled';

/** Background jobs from the Rust registry (jobs.rs). Refetches on focus so a dropped event heals. */
export function useJobs() {
  const [jobs, setJobs] = useState<Job[]>([]);
  useEffect(() => {
    const refresh = () => invoke<Job[]>('list_jobs').then(setJobs).catch(() => {});
    refresh();
    const unlisten = listen<Job[]>('jobs-changed', (e) => setJobs(e.payload));
    window.addEventListener('focus', refresh);
    return () => {
      unlisten.then((f) => f());
      window.removeEventListener('focus', refresh);
    };
  }, []);
  return jobs;
}
```

- [ ] **Step 2: `JobsButton.tsx`**

```tsx
'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { Activity, Check, Circle, Loader2, Minus, RotateCw, X } from 'lucide-react';
import { toast } from 'sonner';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { isTerminal, useJobs, type Job, type JobState } from '@/hooks/useJobs';

const KIND = { enhance: 'Enhance', summary: 'Summary', download: 'Download' } as const;
const ordinal = (n: number) => `${n}${['th', 'st', 'nd', 'rd'][n % 100 > 10 && n % 100 < 14 ? 0 : n % 10] ?? 'th'}`;
const eta = (s: number) => (s < 60 ? '<1 min left' : `~${Math.round(s / 60)} min left`);
const clock = (ms: number) => {
  const t = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
};
const minutes = (j: Job) =>
  j.started_at && j.finished_at ? `${Math.max(1, Math.round((Date.parse(j.finished_at) - Date.parse(j.started_at)) / 60000))} min` : '';

function StepIcon({ state }: { state: JobState }) {
  if (state === 'done') return <Check className="h-3 w-3 text-primary" aria-hidden />;
  if (state === 'running') return <Loader2 className="h-3 w-3 animate-spin" aria-hidden />;
  if (state === 'failed') return <X className="h-3 w-3 text-destructive" aria-hidden />;
  if (state === 'cancelled') return <Minus className="h-3 w-3" aria-hidden />;
  return <Circle className="h-3 w-3" aria-hidden />;
}

/** Title-bar button and popover listing background jobs (spec 4). */
export function JobsButton() {
  const jobs = useJobs();
  const { meetings } = useSidebar();
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [seenAt, setSeenAt] = useState(() => new Date().toISOString());
  const [now, setNow] = useState(Date.now());

  const running = jobs.filter((j) => j.state === 'running');
  const queued = jobs.filter((j) => j.state === 'queued');
  const recent = jobs.filter((j) => isTerminal(j.state)).sort((a, b) => (b.finished_at ?? '').localeCompare(a.finished_at ?? ''));
  const active = running.length + queued.length;
  const unseenFailure = recent.some((j) => j.state === 'failed' && (j.finished_at ?? '') > seenAt);

  // elapsed clocks tick only while the panel shows a running job
  useEffect(() => {
    if (!open || running.length === 0) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [open, running.length]);

  const name = (j: Job) => (j.meeting_id ? meetings.find((m) => m.id === j.meeting_id)?.title ?? 'Meeting' : j.title);
  const call = (cmd: string, id?: string) => invoke(cmd, id ? { id } : {}).catch((e) => toast.error(String(e)));
  const openMeeting = (j: Job) => {
    if (!j.meeting_id) return;
    setOpen(false);
    router.push(`/meeting-details?id=${j.meeting_id}`);
  };

  const Title = ({ j }: { j: Job }) =>
    j.meeting_id ? (
      <button type="button" onClick={() => openMeeting(j)} className="min-w-0 truncate text-left font-medium hover:underline">{name(j)}</button>
    ) : (
      <span className="min-w-0 truncate font-medium">{name(j)}</span>
    );

  const Cancel = ({ j }: { j: Job }) => (
    <button type="button" aria-label={`Cancel ${KIND[j.kind]} of ${name(j)}`} title="Cancel" onClick={() => call('cancel_job', j.id)}
      className="flex h-6 w-6 flex-none items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground">
      <X className="h-3.5 w-3.5" />
    </button>
  );

  const status = (j: Job) => {
    if (j.progress === null) return `${clock(now - Date.parse(j.started_at ?? new Date().toISOString()))} elapsed`;
    return `${j.progress}% · ${j.eta_secs === null ? 'Estimating…' : eta(j.eta_secs)}`;
  };

  return (
    <Popover open={open} onOpenChange={(o) => { setOpen(o); if (o) setSeenAt(new Date().toISOString()); }}>
      <PopoverTrigger asChild>
        <button type="button" aria-label={active ? `Background jobs, ${active} active` : 'Background jobs'} title="Background jobs"
          className="relative flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent/60 hover:text-foreground">
          {running.length ? <Loader2 className="h-4 w-4 animate-spin text-foreground" /> : <Activity className="h-4 w-4" />}
          {active > 0 && (
            <span className="absolute -right-0.5 -top-0.5 min-w-[14px] rounded-full bg-primary px-1 text-center text-[10px] leading-[14px] text-primary-foreground">{active}</span>
          )}
          {unseenFailure && active === 0 && <span className="absolute right-0.5 top-0.5 h-2 w-2 rounded-full bg-destructive" aria-hidden />}
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[360px] p-0" role="dialog" aria-label="Background jobs">
        <div aria-live="polite" className="max-h-[70vh] overflow-y-auto p-2 text-[13px]">
          {active === 0 && <p className="px-2 py-1.5 text-xs text-muted-foreground">Nothing running</p>}

          {running.length > 0 && <h3 className="px-2 pb-1 pt-1 text-[11px] font-medium uppercase text-muted-foreground">Running</h3>}
          <ul>
            {running.map((j) => (
              <li key={j.id} className="mb-1 rounded-md border border-border p-2">
                <div className="flex items-center gap-1.5">
                  <Title j={j} />
                  <span className="flex-none text-muted-foreground">— {KIND[j.kind]}</span>
                  <span className="flex-1" />
                  <Cancel j={j} />
                </div>
                {j.steps.length > 0 && (
                  <div className="mt-1 flex flex-wrap gap-x-3 text-xs text-muted-foreground">
                    {j.steps.map((s) => (
                      <span key={s.name} className="flex items-center gap-1"><StepIcon state={s.state} />{s.name}<span className="sr-only"> {s.state}</span></span>
                    ))}
                  </div>
                )}
                <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-muted" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={j.progress ?? undefined}>
                  {j.progress === null
                    ? <div className="h-full w-1/3 animate-pulse rounded-full bg-primary/60" />
                    : <div className="h-full rounded-full bg-primary transition-[width]" style={{ width: `${j.progress}%` }} />}
                </div>
                <div className="mt-1 flex gap-2 text-xs text-muted-foreground">
                  <span className="tabular-nums">{status(j)}</span>
                  {j.detail && <span className="min-w-0 truncate">{j.detail}</span>}
                </div>
              </li>
            ))}
          </ul>

          {queued.length > 0 && <h3 className="px-2 pb-1 pt-2 text-[11px] font-medium uppercase text-muted-foreground">Queued</h3>}
          <ul>
            {queued.map((j, i) => (
              <li key={j.id} className="flex items-center gap-1.5 rounded px-2 py-1">
                <Title j={j} />
                <span className="flex-none text-muted-foreground">— {KIND[j.kind]} · {ordinal(i + 1)} in queue</span>
                <span className="flex-1" />
                <Cancel j={j} />
              </li>
            ))}
          </ul>

          {recent.length > 0 && (
            <div className="flex items-center px-2 pb-1 pt-2">
              <h3 className="flex-1 text-[11px] font-medium uppercase text-muted-foreground">Recent</h3>
              <button type="button" onClick={() => call('clear_recent_jobs')} className="text-xs text-muted-foreground hover:text-foreground">Clear</button>
            </div>
          )}
          <ul>
            {recent.map((j) => (
              <li key={j.id} className="flex items-center gap-1.5 rounded px-2 py-1 text-xs">
                <StepIcon state={j.state} />
                <Title j={j} />
                <span className="min-w-0 flex-1 truncate text-muted-foreground">
                  — {KIND[j.kind]} · {j.state === 'done' ? minutes(j) : j.state}{j.error ? `: ${j.error}` : ''}
                  {j.steps.filter((s) => s.state === 'failed').map((s) => ` · ${s.name} failed`).join('')}
                </span>
                {j.state === 'failed' && j.kind !== 'summary' && (
                  <button type="button" onClick={() => call('retry_job', j.id)} aria-label={`Retry ${KIND[j.kind]} of ${name(j)}`}
                    className="flex flex-none items-center gap-1 rounded px-1.5 py-0.5 hover:bg-accent"><RotateCw className="h-3 w-3" />Retry</button>
                )}
              </li>
            ))}
          </ul>
        </div>
      </PopoverContent>
    </Popover>
  );
}
```

Radix Popover already gives Esc and outside-click close, moves focus in, and returns focus to the trigger. If `bg-destructive`, `text-destructive` or `bg-primary/60` are not defined in the Tailwind theme, use the tokens that `TitleBar.tsx` and `MeetingStatusTags.tsx` use.

- [ ] **Step 3: Mount in `TitleBar.tsx`**

Add the import:

```tsx
import { JobsButton } from '@/components/Jobs/JobsButton';
```

Inside the outer `data-tauri-drag-region` div, after the centered block's closing `</div>`, add:

```tsx
      <div className="absolute right-3 top-1/2 -translate-y-1/2">
        <JobsButton />
      </div>
```

- [ ] **Step 4: Type-check and lint**

Run: `cd frontend && pnpm exec tsc --noEmit && pnpm run lint`
Expected: no errors in the new or changed files.

- [ ] **Step 5: Commit**

```bash
/usr/bin/git add frontend/src/hooks/useJobs.ts frontend/src/components/Jobs/JobsButton.tsx frontend/src/components/TitleBar.tsx
/usr/bin/git commit -m "feat(jobs): title-bar jobs button and panel"
```

---

### Task 6: Manual verification in the app

**Files:** none (fix in the owning task's files if something fails, then commit there).

- [ ] **Step 1: Run the app**

Run: `cd frontend && pnpm run tauri:dev`

- [ ] **Step 2: Walk the scenarios and note each result**

1. Enhance meeting A. The button spins with badge 1. The panel shows the Transcribe step, a filling bar, "Estimating…", then "~N min left" after 15 s.
2. While A runs, enhance meeting B. B shows under Queued, "1st in queue". Cancel B: it moves to Recent as cancelled, and its `metadata.json` enhance status is `failed` / "Cancelled".
3. Cancel A while transcribing. It ends as cancelled.
4. With wiki automation on for A's project and a hook command of `sleep 60`, enhance A. The Wiki step runs. Cancel it: the `sleep` process is gone (`pgrep sleep`) and the row is cancelled.
5. Generate a summary. The bar is indeterminate with an elapsed clock. Cancel it: the row is cancelled and the meeting page shows the summary as cancelled.
6. Start a Parakeet or Whisper download. The row shows % and MB. Cancel it: cancelled. Start it again, let it fail (turn the network off): Failed with Retry. Retry restarts it.
7. Make a failure while the panel is closed: a red dot shows. Opening the panel clears it.
8. Reload the window (Cmd+R): the list comes back from `list_jobs`.

- [ ] **Step 3: Record the outcome**

Append to the PR description which scenarios passed. Any failure goes back to the task that owns the code.
