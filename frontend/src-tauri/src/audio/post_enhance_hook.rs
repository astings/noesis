//! spec 4.7: run a user command when an enhance job finishes. The app only hands over
//! the meeting folder, project, title and id; what happens next is the user's script.

use anyhow::{Context, Result};
use log::{info, warn};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct HookContext {
    pub folder: PathBuf,
    pub project: String,
    pub title: String,
    pub meeting_id: String,
}

/// Project and title from metadata.json; `_inbox` and empty title when they cannot be read.
pub fn read_context(folder: &Path, meeting_id: &str) -> HookContext {
    let meta: serde_json::Value = std::fs::read_to_string(folder.join("metadata.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let project = meta.get("project").and_then(|v| v.as_str()).unwrap_or(super::library::INBOX).to_string();
    let title = meta
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| meta.get("meeting_name").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    HookContext { folder: folder.to_path_buf(), project, title, meeting_id: meeting_id.to_string() }
}

// spec 4.8: shared logged, detached spawn
/// Called once when the child has exited (or could not be waited on): `true` means exit status 0.
pub type OnExit = Box<dyn FnOnce(bool) + Send + 'static>;

/// Start `command` detached, logging to <folder>/hook.log after a header line.
/// `on_exit` runs on the reaper thread after the child exits.
pub fn spawn_logged(mut command: Command, header: &str, folder: &Path, meeting_id: &str, on_exit: Option<OnExit>) -> Result<()> {
    let log_path = folder.join("hook.log");
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("open {}", log_path.display()))?;
    writeln!(log, "=== {} {}", chrono::Local::now().to_rfc3339(), header)?;
    let stderr = log.try_clone().context("clone hook.log handle")?;
    // own process group: cancelling from the job panel kills Claude Code and its children together
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .spawn()
        .context("spawn post-enhance hook")?;
    info!("Post-enhance hook started (pid {}) for {} in {}", child.id(), meeting_id, folder.display());
    crate::jobs::set_hook_pid(meeting_id, Some(child.id()));
    let meeting_id = meeting_id.to_string();
    std::thread::spawn(move || {
        let success = match child.wait() {
            Ok(status) => {
                info!("Post-enhance hook for {} exited with {}", meeting_id, status);
                status.success()
            }
            Err(e) => {
                warn!("Post-enhance hook for {} could not be waited on: {}", meeting_id, e);
                false
            }
        };
        if let Some(f) = on_exit {
            f(success);
        }
    });
    Ok(())
}

/// `hook.done` holds the time of the successful run; both hook kinds write it.
pub fn write_done_marker(folder: &Path) {
    let marker = folder.join("hook.done");
    if let Err(e) = std::fs::write(&marker, format!("{}\n", chrono::Local::now().to_rfc3339())) {
        warn!("Could not write {}: {}", marker.display(), e);
    }
}

/// Start `sh -c command` in the meeting folder and return at once. Output is appended to hook.log.
/// Exit 0 writes `hook.done`; `then` runs after that.
pub fn spawn(command: &str, ctx: &HookContext, then: Option<OnExit>) -> Result<()> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(&ctx.folder)
        .env("NOESIS_FOLDER", &ctx.folder)
        .env("NOESIS_PROJECT", &ctx.project)
        .env("NOESIS_TITLE", &ctx.title)
        .env("NOESIS_MEETING_ID", &ctx.meeting_id)
        .env("MEETILY_FOLDER", &ctx.folder)
        .env("MEETILY_PROJECT", &ctx.project)
        .env("MEETILY_TITLE", &ctx.title)
        .env("MEETILY_MEETING_ID", &ctx.meeting_id);
    let folder = ctx.folder.clone();
    let on_exit: OnExit = Box::new(move |success| {
        if success {
            write_done_marker(&folder);
        }
        if let Some(f) = then {
            f(success);
        }
    });
    spawn_logged(cmd, &format!("sh -c {}", command.replace('\n', " ")), &ctx.folder, &ctx.meeting_id, Some(on_exit))
}

// spec 8: one dispatch for the auto run after enhance and the manual "Run hook"
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HookKind {
    Wiki,
    Command,
}

/// Receives `(meeting_id, status)` on every hook status transition.
pub type Notify = std::sync::Arc<dyn Fn(&str, &str) + Send + Sync>;

pub const NO_HOOK: &str = "No hook configured. Set one in Settings, Recordings.";

/// The wiki row for the meeting's project wins, else the custom command. `force` re-runs
/// wiki automation even when `hook.done` exists.
pub fn dispatch<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    prefs: &super::recording_preferences::RecordingPreferences,
    ctx: &HookContext,
    force: bool,
) -> std::result::Result<HookKind, String> {
    use tauri::Emitter;
    let app = app.clone();
    let notify: Notify = std::sync::Arc::new(move |id: &str, status: &str| {
        crate::jobs::hook_status(id, status);
        let _ = app.emit("hook-status-changed", serde_json::json!({ "meeting_id": id, "status": status }));
    });
    dispatch_with(prefs, ctx, force, notify)
}

pub fn dispatch_with(
    prefs: &super::recording_preferences::RecordingPreferences,
    ctx: &HookContext,
    force: bool,
    notify: Notify,
) -> std::result::Result<HookKind, String> {
    use super::hook_status::{set, DONE, FAILED, RUNNING};
    let target = if prefs.wiki_automation { super::wiki_automation::find_target(&prefs.wiki_targets, &ctx.project) } else { None };
    let command = prefs.post_enhance_command.trim();
    if target.is_none() && command.is_empty() {
        return Err(NO_HOOK.to_string());
    }
    let id = ctx.meeting_id.clone();
    let transition = move |status: &'static str| {
        set(&id, status);
        notify(&id, status);
    };
    // Running before the spawn: a fast child must not finish before it is marked running.
    transition(RUNNING);
    let t = transition.clone();
    let then: OnExit = Box::new(move |success| t(if success { DONE } else { FAILED }));
    match target {
        Some(target) => match super::wiki_automation::run(target, ctx, prefs.wiki_budget_usd, force, Some(then)) {
            Ok(true) => Ok(HookKind::Wiki),
            // Skipped on hook.done (auto run only): report what the folder says.
            Ok(false) if !force && ctx.folder.join("hook.done").exists() => {
                transition(DONE);
                Ok(HookKind::Wiki)
            }
            // Skipped because a run is in flight; its own exit settles the status.
            Ok(false) => Err("Wiki automation is already running for this meeting.".to_string()),
            Err(e) => {
                transition(FAILED);
                Err(e.to_string())
            }
        },
        // Auto run skips on hook.done, like wiki automation; a forced run always spawns.
        None if !force && ctx.folder.join("hook.done").exists() => {
            transition(DONE);
            Ok(HookKind::Command)
        }
        None => match spawn(command, ctx, Some(then)) {
            Ok(()) => Ok(HookKind::Command),
            Err(e) => {
                transition(FAILED);
                Err(format!("Hook failed to start: {}", e))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(path: &Path) -> String {
        for _ in 0..250 {
            if let Ok(s) = std::fs::read_to_string(path) {
                if s.lines().any(|l| l == "END") {
                    return s;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("hook output never appeared at {}", path.display());
    }

    #[test]
    fn read_context_takes_project_and_title_from_metadata() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("metadata.json"), r#"{"project":"dex","title":"Weekly sync"}"#).unwrap();
        let ctx = read_context(dir.path(), "m-1");
        assert_eq!((ctx.project.as_str(), ctx.title.as_str(), ctx.meeting_id.as_str()), ("dex", "Weekly sync", "m-1"));
        let empty = read_context(Path::new("/nonexistent"), "m-2");
        assert_eq!((empty.project.as_str(), empty.title.as_str()), ("_inbox", ""));
    }

    #[test]
    fn spawn_runs_detached_with_env_and_logs_to_hook_log() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = HookContext {
            folder: dir.path().to_path_buf(),
            project: "dex".into(),
            title: "Weekly sync".into(),
            meeting_id: "m-1".into(),
        };
        spawn("echo \"$MEETILY_PROJECT|$MEETILY_TITLE|$MEETILY_MEETING_ID|$MEETILY_FOLDER|$(pwd)\"; echo \"$NOESIS_PROJECT|$NOESIS_TITLE|$NOESIS_MEETING_ID|$NOESIS_FOLDER\"; echo oops >&2; echo END", &ctx, None).unwrap();
        let log = wait_for(&dir.path().join("hook.log"));
        assert!(log.starts_with("=== "), "header line first: {}", log);
        let real = dir.path().canonicalize().unwrap(); // macOS: /var -> /private/var in pwd
        let expected = format!("dex|Weekly sync|m-1|{}|{}", dir.path().display(), real.display());
        assert!(log.contains(&expected), "{}", log);
        let noesis = format!("dex|Weekly sync|m-1|{}", dir.path().display());
        assert!(log.contains(&format!("{}\n", noesis)), "NOESIS_* set too: {}", log);
        assert!(log.contains("oops"), "stderr is captured too: {}", log);
    }

    #[test]
    fn spawn_appends_to_an_existing_log() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hook.log"), "previous run\n").unwrap();
        let ctx = read_context(dir.path(), "m-3");
        spawn("echo END", &ctx, None).unwrap();
        let log = wait_for(&dir.path().join("hook.log"));
        assert!(log.starts_with("previous run\n=== "), "{}", log);
    }

    // spec 4.8
    fn outcome_after(dir: &Path, script: &str) -> String {
        let out = dir.join("outcome");
        let o = out.clone();
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        let on_exit: OnExit = Box::new(move |success| {
            std::fs::write(o, if success { "ok" } else { "fail" }).unwrap();
        });
        spawn_logged(cmd, "test", dir, "m-outcome", Some(on_exit)).unwrap();
        for _ in 0..250 {
            if let Ok(s) = std::fs::read_to_string(&out) {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("on_exit never ran");
    }

    #[test]
    fn spawn_logged_reports_the_exit_outcome() {
        let ok = tempfile::tempdir().unwrap();
        assert_eq!(outcome_after(ok.path(), "echo END"), "ok");
        let bad = tempfile::tempdir().unwrap();
        assert_eq!(outcome_after(bad.path(), "echo END; exit 3"), "fail");
    }

    fn wait_until(mut ok: impl FnMut() -> bool) -> bool {
        for _ in 0..250 {
            if ok() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    // spec 8: the custom command reports done the same way as wiki automation
    #[test]
    fn custom_command_writes_hook_done_only_on_exit_zero() {
        let ok = tempfile::tempdir().unwrap();
        spawn("exit 0", &read_context(ok.path(), "m-done-ok"), None).unwrap();
        assert!(wait_until(|| ok.path().join("hook.done").exists()), "hook.done after exit 0");
        let bad = tempfile::tempdir().unwrap();
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let f = finished.clone();
        spawn("exit 1", &read_context(bad.path(), "m-done-bad"), Some(Box::new(move |_| f.store(true, std::sync::atomic::Ordering::SeqCst)))).unwrap();
        assert!(wait_until(|| finished.load(std::sync::atomic::Ordering::SeqCst)));
        assert!(!bad.path().join("hook.done").exists(), "no hook.done after exit 1");
    }

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
        // the exit status is not deterministic once the group is signalled; only that the hook settled matters
        assert!(rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(), "on_exit runs after the group is killed");
        let alive = std::process::Command::new("kill").args(["-0", &read("child.pid")]).status().unwrap().success();
        assert!(!alive, "the backgrounded child is gone too");
    }

    type Seen = std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>;
    fn recorder() -> (Seen, Notify) {
        let seen: Seen = Default::default();
        let s = seen.clone();
        (seen, std::sync::Arc::new(move |id: &str, status: &str| s.lock().unwrap().push((id.into(), status.into()))))
    }

    #[test]
    fn dispatch_without_any_hook_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = super::super::recording_preferences::RecordingPreferences::default();
        let (seen, notify) = recorder();
        let err = dispatch_with(&prefs, &read_context(dir.path(), "m-none"), false, notify).unwrap_err();
        assert_eq!(err, "No hook configured. Set one in Settings, Recordings.");
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn dispatch_runs_the_command_and_tracks_status() {
        let dir = tempfile::tempdir().unwrap();
        let mut prefs = super::super::recording_preferences::RecordingPreferences::default();
        prefs.post_enhance_command = "exit 0".into();
        let (seen, notify) = recorder();
        assert_eq!(dispatch_with(&prefs, &read_context(dir.path(), "m-cmd-ok"), false, notify).unwrap(), HookKind::Command);
        assert!(wait_until(|| seen.lock().unwrap().len() == 2), "{:?}", seen.lock().unwrap());
        assert_eq!(*seen.lock().unwrap(), vec![("m-cmd-ok".to_string(), "running".to_string()), ("m-cmd-ok".to_string(), "done".to_string())]);
        assert_eq!(super::super::hook_status::memory_status("m-cmd-ok"), None, "done lives on disk");

        let bad = tempfile::tempdir().unwrap();
        prefs.post_enhance_command = "exit 2".into();
        let (seen, notify) = recorder();
        dispatch_with(&prefs, &read_context(bad.path(), "m-cmd-bad"), false, notify).unwrap();
        assert!(wait_until(|| seen.lock().unwrap().len() == 2));
        assert_eq!(seen.lock().unwrap()[1].1, "failed");
        assert_eq!(super::super::hook_status::memory_status("m-cmd-bad"), Some("failed"));
    }

    #[test]
    fn dispatch_command_skips_on_hook_done_unless_forced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hook.done"), "x").unwrap();
        let mut prefs = super::super::recording_preferences::RecordingPreferences::default();
        prefs.post_enhance_command = "touch command-ran".into();
        let (seen, notify) = recorder();
        dispatch_with(&prefs, &read_context(dir.path(), "m-skip"), false, notify).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!dir.path().join("command-ran").exists(), "not spawned when hook.done exists");
        assert_eq!(seen.lock().unwrap().last().unwrap().1, "done");
        let (_seen, notify) = recorder();
        dispatch_with(&prefs, &read_context(dir.path(), "m-force"), true, notify).unwrap();
        assert!(wait_until(|| dir.path().join("command-ran").exists()), "forced run spawns");
    }

    #[test]
    fn dispatch_prefers_the_wiki_row_and_marks_a_failed_start() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("metadata.json"), r#"{"project":"dex"}"#).unwrap();
        let mut prefs = super::super::recording_preferences::RecordingPreferences::default();
        prefs.post_enhance_command = "touch command-ran".into();
        prefs.wiki_automation = true;
        prefs.wiki_targets = vec![super::super::recording_preferences::WikiTarget { project: "dex".into(), directory: "/nonexistent/wiki".into(), skill: String::new() }];
        let (seen, notify) = recorder();
        let err = dispatch_with(&prefs, &read_context(dir.path(), "m-wiki"), true, notify).unwrap_err();
        assert!(err.contains("/nonexistent/wiki"), "{}", err);
        assert_eq!(seen.lock().unwrap().last().unwrap().1, "failed");
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!dir.path().join("command-ran").exists(), "the command does not run when a wiki row exists");
    }
}
