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

/// Start `sh -c command` in the meeting folder and return at once. Output is appended to hook.log.
pub fn spawn(command: &str, ctx: &HookContext) -> Result<()> {
    let log_path = ctx.folder.join("hook.log");
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("open {}", log_path.display()))?;
    writeln!(log, "=== {} sh -c {}", chrono::Local::now().to_rfc3339(), command.replace('\n', " "))?;
    let stderr = log.try_clone().context("clone hook.log handle")?;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&ctx.folder)
        .env("MEETILY_FOLDER", &ctx.folder)
        .env("MEETILY_PROJECT", &ctx.project)
        .env("MEETILY_TITLE", &ctx.title)
        .env("MEETILY_MEETING_ID", &ctx.meeting_id)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .spawn()
        .context("spawn post-enhance hook")?;
    info!("Post-enhance hook started (pid {}) for {} in {}", child.id(), ctx.meeting_id, ctx.folder.display());
    // The enhance job does not depend on the hook, but someone has to reap the child or it
    // stays a zombie until the app quits: a plain thread waits for it in the background.
    let meeting_id = ctx.meeting_id.clone();
    std::thread::spawn(move || match child.wait() {
        Ok(status) => info!("Post-enhance hook for {} exited with {}", meeting_id, status),
        Err(e) => warn!("Post-enhance hook for {} could not be waited on: {}", meeting_id, e),
    });
    Ok(())
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
        spawn("echo \"$MEETILY_PROJECT|$MEETILY_TITLE|$MEETILY_MEETING_ID|$MEETILY_FOLDER|$(pwd)\"; echo oops >&2; echo END", &ctx).unwrap();
        let log = wait_for(&dir.path().join("hook.log"));
        assert!(log.starts_with("=== "), "header line first: {}", log);
        let real = dir.path().canonicalize().unwrap(); // macOS: /var -> /private/var in pwd
        let expected = format!("dex|Weekly sync|m-1|{}|{}", dir.path().display(), real.display());
        assert!(log.contains(&expected), "{}", log);
        assert!(log.contains("oops"), "stderr is captured too: {}", log);
    }

    #[test]
    fn spawn_appends_to_an_existing_log() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hook.log"), "previous run\n").unwrap();
        let ctx = read_context(dir.path(), "m-3");
        spawn("echo END", &ctx).unwrap();
        let log = wait_for(&dir.path().join("hook.log"));
        assert!(log.starts_with("previous run\n=== "), "{}", log);
    }
}
