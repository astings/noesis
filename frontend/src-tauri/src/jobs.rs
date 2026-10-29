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
pub const STEP_AGENT: &str = "Agent";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Enhance,
    Summary,
    Download,
    Tagging,
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
    /// one meeting (`meeting_id` set) or the "tag untagged meetings" batch (`meeting_id` None)
    Tagging,
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
    /// set when the enhance handed its transcript to the post-enhance hook
    #[serde(skip)]
    awaiting_hook: bool,
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
            awaiting_hook: false,
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

    fn live_enhances<'a>(&'a mut self, meeting_id: &'a str) -> impl Iterator<Item = &'a mut Job> + 'a {
        self.jobs
            .iter_mut()
            .filter(move |j| j.kind == JobKind::Enhance && !j.state.is_terminal() && j.meeting_id.as_deref() == Some(meeting_id))
    }

    /// The first live enhance job of the meeting that handed its transcript to the hook.
    fn hook_job<'a>(&'a mut self, meeting_id: &'a str) -> Option<&'a mut Job> {
        self.live_enhances(meeting_id).find(|j| j.awaiting_hook)
    }

    /// Marks the job as waiting for the post-enhance hook (called just before dispatch).
    pub fn await_hook(&mut self, id: &str) {
        if let Some(j) = self.get(id) {
            j.awaiting_hook = true;
        }
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
        // the rate is measured between percent changes, not between calls: repeats at the same
        // percent keep the anchor, so sub-percent callbacks do not shorten the ETA
        match j.last {
            Some((t, p)) if pct > p => {
                let dt = now.duration_since(t).as_secs_f64();
                if dt > 0.0 {
                    let inst = (pct - p) as f64 / dt;
                    j.rate = Some(j.rate.map_or(inst, |r| ETA_ALPHA * inst + (1.0 - ETA_ALPHA) * r));
                }
                j.last = Some((now, pct));
            }
            Some((_, p)) if pct == p => {}
            _ => j.last = Some((now, pct)),
        }
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
        let target = self
            .live_enhances(meeting_id)
            .find(|j| j.steps.iter().any(|s| s.name == STEP_TRANSCRIBE && s.state == JobState::Running))
            .map(|j| j.id.clone());
        match target {
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

    /// True when the enhance job for this meeting has a cancel request pending.
    pub fn hook_cancel_requested(&mut self, meeting_id: &str) -> bool {
        self.hook_job(meeting_id).is_some_and(|j| j.cancel_requested)
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
        let Some(j) = self.hook_job(meeting_id) else { return };
        let id = j.id.clone();
        let cancelled = j.cancel_requested;
        match status {
            "running" => self.step(&id, STEP_AGENT, JobState::Running, None),
            // a killed hook can still exit 0, so a cancel request wins over either exit status
            "done" | "failed" if cancelled => {
                self.step(&id, STEP_AGENT, JobState::Cancelled, None);
                self.finish(&id, Outcome::Cancelled);
            }
            "done" => {
                self.step(&id, STEP_AGENT, JobState::Done, None);
                self.finish(&id, Outcome::Done);
            }
            "failed" => {
                // spec 5: the transcript is the success criterion; the step carries the failure
                self.step(&id, STEP_AGENT, JobState::Failed, Some("see hook.log".to_string()));
                self.finish(&id, Outcome::Done);
            }
            "cancelled" => {
                self.step(&id, STEP_AGENT, JobState::Cancelled, None);
                self.finish(&id, Outcome::Cancelled);
            }
            _ => {}
        }
    }

    pub fn set_hook_pid(&mut self, meeting_id: &str, pid: Option<u32>) {
        if let Some(j) = self.hook_job(meeting_id) {
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
    // emit under the lock so snapshots reach the UI in order (no Rust-side listener, no deadlock)
    let mut r = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    let (out, changed) = f(&mut r);
    if let (true, Some(app)) = (changed, APP.get()) {
        let _ = app.emit("jobs-changed", r.snapshot());
    }
    out
}

pub fn add(id: &str, kind: JobKind, meeting_id: Option<&str>, title: &str, control: Control, running: bool) {
    update(|r| (r.add(id, kind, meeting_id, title, control, running, Instant::now()), true))
}
/// Summary jobs are started and finished in different functions; both derive the id.
pub fn summary_id(meeting_id: &str, started_at: DateTime<Utc>) -> String {
    format!("summary:{}:{}", meeting_id, started_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
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
pub fn hook_cancel_requested(meeting_id: &str) -> bool {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).hook_cancel_requested(meeting_id)
}
pub fn request_cancel(id: &str) -> Option<Job> {
    update(|r| {
        let j = r.request_cancel(id);
        let changed = j.is_some();
        (j, changed)
    })
}
pub fn await_hook(id: &str) {
    update(|r| (r.await_hook(id), false))
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
        // the batch checks the flag between meetings; each call is aborted while waiting for the model
        Control::Tagging => {}
    }
    Ok(())
}

/// Kills the hook's whole process group (Claude Code and its children). The hook is
/// spawned with `process_group(0)`, so its pid is the group id.
pub(crate) fn kill_group(pid: u32) {
    if pid <= 1 {
        return;
    }
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
                // the command also syncs the folder slug after the run
                crate::audio::retranscription::start_retranscription_command(app, meeting_id, folder, language, model, provider)
                    .await
                    .map(|_| ())
            }
            Control::WhisperDownload => crate::whisper_engine::commands::whisper_download_model(app, model).await,
            Control::ParakeetDownload => crate::parakeet_engine::commands::parakeet_download_model(app, model).await,
            Control::BuiltinDownload => {
                let state = app.state::<crate::summary::summary_engine::commands::ModelManagerState>();
                crate::summary::summary_engine::commands::builtin_ai_download_model(app.clone(), state, model).await
            }
            Control::Tagging => match job.meeting_id {
                Some(meeting_id) => crate::audio::meeting_tagging::run_meeting_tagging(app, meeting_id).await.map(|_| ()),
                None => crate::audio::meeting_tagging::tag_untagged_meetings(app).await.map(|_| ()),
            },
            Control::Summary { .. } => Ok(()),
        };
        if let Err(e) = result {
            log::warn!("jobs: retry failed: {}", e);
        }
    });
    Ok(())
}

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
    fn eta_ignores_repeated_same_percent_calls() {
        let mut r = Registry::default();
        let t0 = Instant::now();
        r.add("a", JobKind::Download, None, "m", Control::WhisperDownload, true, t0);
        r.progress("a", 0, None, t0);
        // 1 %/s, with four extra callbacks at the same percent inside every second
        for s in 1..=30u64 {
            let at = |ms: u64| t0 + Duration::from_millis(s * 1000 + ms);
            r.progress("a", s as u8, None, at(0));
            for ms in [200, 400, 600, 800] {
                r.progress("a", s as u8, None, at(ms));
            }
        }
        assert_eq!(r.snapshot()[0].eta_secs, Some(70), "rate stays 1 %/s");
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
        r.await_hook("a");
        r.hook_status("m", "running");
        r.hook_status("m", "failed");
        let a = &r.snapshot()[0];
        assert_eq!(a.state, JobState::Done, "a failed wiki step does not fail the enhance");
        assert_eq!(a.steps[0], Step { name: STEP_AGENT.into(), state: JobState::Failed, error: Some("see hook.log".into()) });
        assert_eq!(r.snapshot()[1].state, JobState::Queued, "the re-queued job is untouched");
    }

    #[test]
    fn hook_cancelled_status_ends_the_job_cancelled() {
        let mut r = Registry::default();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), true, Instant::now());
        r.await_hook("a");
        r.hook_status("m", "running");
        r.hook_status("m", "cancelled");
        assert_eq!(r.snapshot()[0].state, JobState::Cancelled);
    }

    #[test]
    fn hook_failure_after_cancel_is_cancelled() {
        let mut r = Registry::default();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), true, Instant::now());
        r.await_hook("a");
        r.hook_status("m", "running");
        r.request_cancel("a");
        r.hook_status("m", "failed");
        assert_eq!(r.snapshot()[0].state, JobState::Cancelled);
    }

    #[test]
    fn hook_done_after_cancel_is_cancelled() {
        let mut r = Registry::default();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), true, Instant::now());
        r.await_hook("a");
        r.hook_status("m", "running");
        r.request_cancel("a");
        r.hook_status("m", "done");
        assert_eq!(r.snapshot()[0].state, JobState::Cancelled);
    }

    #[test]
    fn hook_cancel_requested_follows_the_enhance_job() {
        let mut r = Registry::default();
        assert!(!r.hook_cancel_requested("m"));
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), true, Instant::now());
        r.await_hook("a");
        assert!(!r.hook_cancel_requested("m"));
        r.request_cancel("a");
        assert!(r.hook_cancel_requested("m"));
        assert!(!r.hook_cancel_requested("other"));
    }

    #[test]
    fn enhance_progress_ignores_queued_jobs() {
        let mut r = Registry::default();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), false, Instant::now());
        assert!(!r.enhance_progress("m", 0, Some("Waiting".into()), Instant::now()));
        assert_eq!(r.snapshot()[0].progress, None);
    }

    #[test]
    fn hook_done_ends_the_enhance_done() {
        let mut r = Registry::default();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), true, Instant::now());
        r.await_hook("a");
        r.hook_status("m", "running");
        r.hook_status("m", "done");
        let a = &r.snapshot()[0];
        assert_eq!(a.state, JobState::Done);
        assert_eq!(a.steps[0], Step { name: STEP_AGENT.into(), state: JobState::Done, error: None });
    }

    #[test]
    fn hook_and_progress_target_different_jobs_of_one_meeting() {
        let mut r = Registry::default();
        let t = Instant::now();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), true, t);
        r.await_hook("a");
        r.add("b", JobKind::Enhance, Some("m"), "", enhance(), true, t);
        r.step("b", STEP_TRANSCRIBE, JobState::Running, None);
        assert!(r.enhance_progress("m", 40, None, t));
        r.hook_status("m", "running");
        let s = r.snapshot();
        assert_eq!(s[1].progress, Some(40));
        assert_eq!(s[0].progress, None);
        assert_eq!(s[0].steps[0].name, STEP_AGENT);
        assert!(s[1].steps.iter().all(|x| x.name != STEP_AGENT));
    }

    #[test]
    fn hook_lookups_ignore_a_job_not_awaiting_the_hook() {
        let mut r = Registry::default();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), true, Instant::now());
        r.step("a", STEP_TRANSCRIBE, JobState::Running, None);
        r.hook_status("m", "running");
        r.set_hook_pid("m", Some(42));
        r.request_cancel("a");
        assert!(!r.hook_cancel_requested("m"));
        let a = &r.snapshot()[0];
        assert_eq!(a.steps.len(), 1);
        assert_eq!(a.hook_pid, None);
    }

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
}
