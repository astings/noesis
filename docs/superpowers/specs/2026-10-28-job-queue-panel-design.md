# Job queue panel: design

Status: approved 2026-10-28. Branch `enhance/job-queue`.

## 1. Goal

One place to see what the app is doing in the background, how long it has left, and to cancel it. A button at the top right of the title bar opens a popover listing every job: enhance (re-transcribe, then title/tags, then the wiki hook), summary generation and model downloads.

Out of scope: reordering the queue, pausing, a dedicated jobs page, persisting history across restarts, changing concurrency. Today's concurrency stays: enhance runs one at a time behind `ENHANCE_QUEUE`, summaries run alongside, downloads are independent. The panel observes and cancels; it does not schedule.

## 2. Current state

- Enhance: serialized by `ENHANCE_QUEUE` (`audio/retranscription.rs`). Emits `retranscription-progress` with stage and percent, no ETA. Only the running job can be cancelled (`cancel_retranscription`); queued jobs are invisible and refuse cancel.
- Summary: per-meeting `CancellationToken` in `SummaryService`, `api_cancel_summary` exists, no percent.
- Title/tags (`meeting_tagging::run_after_enhance`) and wiki hook (`post_enhance_hook::dispatch`) run after enhance. The hook emits `hook-status-changed`; neither can be cancelled.
- Model downloads (Whisper, Parakeet, built-in AI) emit their own progress events and have their own cancel commands.
- Each consumer listens to its own events. Nothing aggregates them.

## 3. Backend: job registry

New module `src-tauri/src/jobs.rs` (additive, keeps upstream merges feasible).

```rust
Job {
    id: String,                       // uuid
    kind: JobKind,                    // Enhance | Summary | Download
    meeting_id: Option<String>,       // None for downloads
    title: String,                    // meeting title or model name
    state: JobState,                  // Queued | Running | Done | Failed | Cancelled
    steps: Vec<Step>,                 // Enhance: transcribe, title, wiki. Empty otherwise.
    progress: Option<u8>,             // None = indeterminate
    detail: Option<String>,           // stage label, or "48 / 1500 MB" for downloads
    eta_secs: Option<u32>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    error: Option<String>,            // first line only
}
Step { name: String, state: JobState, error: Option<String> }
```

- Registry: a `Lazy<StdMutex<Vec<Job>>>` in memory. Every mutation emits `jobs-changed` with the full list (small, so no diffing). Recent (terminal-state) jobs are capped at 10, oldest dropped first.
- API used by existing code paths: `jobs::enqueue`, `start`, `progress`, `step`, `finish(Ok|Err|Cancelled)`. Existing events stay so `RetranscribeDialog`, `SidebarProvider` and `autoEnhance.ts` keep working.
- Tauri commands: `list_jobs() -> Vec<Job>`, `cancel_job(id)`, `clear_recent_jobs()`.
- `cancel_job` on a job already terminal or unknown returns `Ok(())` and does nothing.

### 3.1 Cancel per kind

| Kind | Queued | Running |
|---|---|---|
| Enhance | New per-job cancel flag, checked right after acquiring `ENHANCE_QUEUE`; the job ends `Cancelled` without transcribing | Existing `RETRANSCRIPTION_CANCELLED`. During the title or wiki step: cancel skips remaining steps; the wiki step kills the child process |
| Summary | n/a (starts immediately) | `SummaryService::cancel_summary` |
| Download | n/a | Existing cancel command of that model manager |

Wiki hook kill: `post_enhance_hook` keeps the spawned child's handle (or pid) registered against the job id; cancel kills the process group so Claude Code and its children stop. `hook.done` is not written on cancel.

### 3.2 ETA

Computed in Rust when progress updates: rate = Δpercent / Δtime, smoothed with an exponential moving average (α = 0.3). `eta_secs` is `None` until both 15 s have elapsed and 10 % is reached (enhance jumps to 25 % within seconds while decoding, so percent alone misleads), and always `None` when `progress` is `None`. The UI rounds: `<1 min`, `~N min`.

## 4. UI

Files: `components/Jobs/JobsButton.tsx`, `components/Jobs/JobsPanel.tsx`, `hooks/useJobs.ts`. No new dependency, no SidebarProvider change.

- `useJobs`: calls `list_jobs` on mount and on window focus, then replaces state on each `jobs-changed`.
- Button in `TitleBar.tsx`, right side next to search, always visible.

| State | Look |
|---|---|
| Idle | Muted icon |
| Running | Spinning ring around the icon, badge = running + queued count |
| Failed, unseen | Red dot, cleared when the panel opens |

- Panel: 360 px popover anchored to the button, closes on Esc and outside click. Sections Running, Queued (with position "1st in queue"), Recent (with Clear).

```
Running
  ┌ Weekly sync — Enhance                  ✕ ┐
  │ ✓ Transcribe  ● Title/tags  ○ Wiki       │
  │ ▓▓▓▓▓▓▓▓░░░░  62% · ~3 min left          │
  └──────────────────────────────────────────┘
  ┌ Client call — Summary                  ✕ ┐
  │ ░▒▓░▒▓░▒▓░▒  1:42 elapsed                 │
  └──────────────────────────────────────────┘
Queued
  Design review — Enhance · 1st in queue   ✕
Recent                                  Clear
  ✓ Standup — Enhance · 4 min
  ✕ Retro — Summary · failed: Ollama unreachable   Retry
  – large-v3 — Download · cancelled
```

- Determinate bar when `progress` is set; otherwise indeterminate bar plus elapsed time. ETA shows "Estimating…" while `eta_secs` is `None` on a determinate job.
- Clicking the meeting title opens the meeting and closes the panel. Download rows have no link and show size in `detail`.
- Cancel: one click, no confirmation; the row moves to Recent as Cancelled.
- Retry on failed rows (`retry_job`) restarts the job with the arguments it first ran with (enhance: folder, language, model, provider; downloads: model). Summary has no Retry: it needs the meeting page's transcript, template and model, so its row links to the meeting.
- Enhance steps appear as they are reached (Transcribe, then Title/tags if on, then Wiki if a hook runs); no pending placeholders.
- Empty: "Nothing running", plus Recent if any.
- Accessibility: popover is `role="dialog"` with focus moved in and returned to the button on close, rows are a list, status changes go through an `aria-live="polite"` region, every state has a text label.

## 5. Errors

- A failed job goes to Recent as `Failed` with the first error line; the full error still goes to logs and `metadata.json` as today.
- Enhance where transcription succeeds but title/tags or wiki fails: job ends `Done`, that step `Failed` with its error. The transcript is the success criterion, matching current behavior.
- App crash or quit: the registry is empty on next launch; `metadata.json` enhance status stays the durable record.
- Lost events: the full-list payload self-heals on the next event; `useJobs` also refetches on focus.

## 6. Testing

- `cargo test jobs`: state transitions (queued → running → done, cancel while queued, cancel when terminal is a no-op), Recent capped at 10, ETA `None` before 15 s / 10 %, smoothed after.
- Queued enhance cancel: two enhances, cancel the second while it waits; it never transcribes and ends `Cancelled`.
- Hook kill: hook command `sleep 60`, cancel, assert the process is gone and the job is `Cancelled`.
- Manual in `pnpm run tauri:dev`: enhance one meeting, queue a second, run a summary, start a download, cancel each from the panel.
