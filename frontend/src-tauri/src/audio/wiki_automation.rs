//! spec 4.8: the post-enhance hook without a script. The settings table says which wiki
//! belongs to which project; this module composes the Claude Code run and starts it
//! through the same logged, detached spawn as the custom hook.

use anyhow::{bail, Result};
use log::warn;
use std::path::{Path, PathBuf};
use std::process::Command;
use super::post_enhance_hook::{spawn_logged, write_done_marker, HookContext, OnExit};
use super::recording_preferences::PostProcessingTarget;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// Meetings with a Claude run still in flight, so a re-enhance cannot start a second one.
static RUNNING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn running() -> &'static Mutex<HashSet<String>> {
    RUNNING.get_or_init(|| Mutex::new(HashSet::new()))
}

/// `~` and `~/...` resolve against HOME; anything else is returned as is.
pub fn expand_home(path: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(home).join(rest),
        None if path == "~" => PathBuf::from(home),
        None => PathBuf::from(path),
    }
}

pub const ALLOWED_TOOLS: [&str; 2] = ["mcp__open-knowledge", "mcp__noesis"];
pub const DENIED_TOOLS: [&str; 3] = ["mcp__open-knowledge__delete", "mcp__open-knowledge__move", "mcp__noesis__assign_meeting"];

pub fn find_target<'a>(targets: &'a [PostProcessingTarget], project: &str) -> Option<&'a PostProcessingTarget> {
    targets.iter().find(|t| t.project == project && !t.directory.trim().is_empty())
}

/// Nobody reads a headless run, so a question would hang until the budget runs out.
const UNATTENDED: &str = "This run is unattended: do not ask questions; if the wiki already covers this meeting, stop without changes.";

pub fn build_prompt(target: &PostProcessingTarget, ctx: &HookContext) -> String {
    let where_ = format!(
        "the new meeting recording at {} (project {}, meeting id {}, title: {})",
        ctx.folder.display(), ctx.project, ctx.meeting_id, ctx.title
    );
    let skill = target.skill.trim();
    if skill.is_empty() {
        format!("Process {}. Draft only: write the transcript document and a draft log entry; leave everything uncommitted for review. {}", where_, UNATTENDED)
    } else {
        format!("Use the {} skill to process {}. Draft only: write the transcript document and a draft log entry; do not score, do not update trackers or the Blueprint. Leave everything uncommitted for review. {}", skill, where_, UNATTENDED)
    }
}

pub fn format_budget(usd: f64) -> String {
    let s = format!("{:.2}", usd.max(0.0));
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn claude_args(prompt: &str, budget_usd: f64) -> Vec<String> {
    let mut a: Vec<String> = vec!["-p".into(), prompt.into(), "--permission-mode".into(), "acceptEdits".into(), "--allowedTools".into()];
    a.extend(ALLOWED_TOOLS.iter().map(|s| s.to_string()));
    a.push("--disallowedTools".into());
    a.extend(DENIED_TOOLS.iter().map(|s| s.to_string()));
    a.push("--max-budget-usd".into());
    a.push(format_budget(budget_usd));
    a
}

/// The app launched from Finder has a minimal PATH; the user bins come first.
pub fn hook_path_env() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let inherited = std::env::var("PATH").unwrap_or_default();
    format!("{}/.local/bin:/opt/homebrew/bin:/usr/local/bin:{}", home, inherited)
}

pub fn locate_claude() -> Option<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        PathBuf::from(format!("{}/.local/bin/claude", home)),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    candidates.into_iter().find(|p| p.is_file()).or_else(|| which::which("claude").ok())
}

fn note(folder: &Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(folder.join("hook.log")) {
        let _ = writeln!(f, "=== {} {}", chrono::Local::now().to_rfc3339(), line);
    }
}

/// Run Claude Code in the wiki for this meeting. Errors are also noted in hook.log.
/// `Ok(false)` means skipped (`hook.done` exists and `force` is off, or a run is in flight).
/// `then` runs after the child exits, once the marker is written.
pub fn run(target: &PostProcessingTarget, ctx: &HookContext, budget_usd: f64, force: bool, then: Option<OnExit>) -> Result<bool> {
    if !force && ctx.folder.join("hook.done").exists() {
        note(&ctx.folder, "skipped: hook.done exists");
        return Ok(false);
    }
    if running().lock().unwrap_or_else(|e| e.into_inner()).contains(&ctx.meeting_id) {
        note(&ctx.folder, "skipped: a run for this meeting is still in progress");
        return Ok(false);
    }
    let dir = expand_home(target.directory.trim());
    if !dir.is_dir() {
        note(&ctx.folder, &format!("wiki directory {} not found", dir.display()));
        bail!("wiki directory {} not found", dir.display());
    }
    let Some(claude) = locate_claude() else {
        note(&ctx.folder, "Claude Code (claude) not found in ~/.local/bin, /opt/homebrew/bin, /usr/local/bin or PATH");
        bail!("claude binary not found");
    };
    let prompt = build_prompt(target, ctx);
    let args = claude_args(&prompt, budget_usd);
    let mut cmd = Command::new(&claude);
    cmd.args(&args)
        .current_dir(&dir)
        .env("PATH", hook_path_env())
        .env("NOESIS_FOLDER", &ctx.folder)
        .env("NOESIS_PROJECT", &ctx.project)
        .env("NOESIS_TITLE", &ctx.title)
        .env("NOESIS_MEETING_ID", &ctx.meeting_id)
        .env("MEETILY_FOLDER", &ctx.folder)
        .env("MEETILY_PROJECT", &ctx.project)
        .env("MEETILY_TITLE", &ctx.title)
        .env("MEETILY_MEETING_ID", &ctx.meeting_id);
    let header = format!("{} {}", claude.display(), args.join(" "));
    running().lock().unwrap_or_else(|e| e.into_inner()).insert(ctx.meeting_id.clone());
    let id = ctx.meeting_id.clone();
    let folder = ctx.folder.clone();
    let on_exit: OnExit = Box::new(move |success| {
        // Marker first, then release the id: a re-enhance arriving in between sees the marker.
        if success {
            write_done_marker(&folder);
        }
        running().lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        if let Some(f) = then {
            f(success);
        }
    });
    spawn_logged(cmd, &header, &ctx.folder, &ctx.meeting_id, Some(on_exit))
        .inspect_err(|e| {
            running().lock().unwrap_or_else(|e| e.into_inner()).remove(&ctx.meeting_id);
            warn!("Wiki automation failed to start for {}: {}", ctx.meeting_id, e)
        })
        .map(|_| true)
}

/// spec 4.8: where the settings card says Claude Code was found
#[tauri::command]
pub async fn locate_claude_cli() -> Option<String> {
    locate_claude().map(|p| p.display().to_string())
}

/// spec 4.8: native folder picker for a wiki directory
#[tauri::command]
pub async fn select_wiki_directory<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
        .await
        .map_err(|e| e.to_string())?;
    Ok(picked.map(|p| p.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(project: &str, dir: &str, skill: &str) -> PostProcessingTarget {
        PostProcessingTarget { project: project.into(), directory: dir.into(), instructions: String::new(), skill: skill.into() }
    }
    fn ctx(folder: &Path) -> HookContext {
        HookContext { folder: folder.to_path_buf(), project: "dex".into(), title: "Weekly sync".into(), meeting_id: "m-1".into() }
    }

    #[test]
    fn find_target_matches_project_and_skips_blank_directories() {
        let t = vec![target("dex", "  ", "x"), target("dex", "/w/dex", "dex-case-tracking"), target("flo", "/w/flo", "")];
        assert_eq!(find_target(&t, "dex").map(|t| t.directory.as_str()), Some("/w/dex"));
        assert_eq!(find_target(&t, "flo").map(|t| t.skill.as_str()), Some(""));
        assert!(find_target(&t, "_inbox").is_none());
    }

    #[test]
    fn prompt_names_the_skill_when_set_and_asks_for_a_draft() {
        let c = ctx(Path::new("/r/dex/2026-10-27_16-18 weekly"));
        let with = build_prompt(&target("dex", "/w/dex", "dex-case-tracking"), &c);
        assert_eq!(with, "Use the dex-case-tracking skill to process the new meeting recording at /r/dex/2026-10-27_16-18 weekly (project dex, meeting id m-1, title: Weekly sync). Draft only: write the transcript document and a draft log entry; do not score, do not update trackers or the Blueprint. Leave everything uncommitted for review. This run is unattended: do not ask questions; if the wiki already covers this meeting, stop without changes.");
        let without = build_prompt(&target("dex", "/w/dex", ""), &c);
        assert_eq!(without, "Process the new meeting recording at /r/dex/2026-10-27_16-18 weekly (project dex, meeting id m-1, title: Weekly sync). Draft only: write the transcript document and a draft log entry; leave everything uncommitted for review. This run is unattended: do not ask questions; if the wiki already covers this meeting, stop without changes.");
    }

    #[test]
    fn budget_formats_without_trailing_zeros() {
        assert_eq!(format_budget(5.0), "5");
        assert_eq!(format_budget(2.5), "2.5");
        assert_eq!(format_budget(0.75), "0.75");
        assert_eq!(format_budget(0.0), "0");
    }

    #[test]
    fn claude_args_carry_the_safe_flags_in_order() {
        let a = claude_args("hello", 5.0);
        assert_eq!(a, vec![
            "-p", "hello", "--permission-mode", "acceptEdits",
            "--allowedTools", "mcp__open-knowledge", "mcp__noesis",
            "--disallowedTools", "mcp__open-knowledge__delete", "mcp__open-knowledge__move", "mcp__noesis__assign_meeting",
            "--max-budget-usd", "5",
        ]);
    }

    #[test]
    fn hook_path_puts_the_user_bins_first() {
        let p = hook_path_env();
        assert!(p.starts_with(&format!("{}/.local/bin:/opt/homebrew/bin:/usr/local/bin:", std::env::var("HOME").unwrap())), "{}", p);
    }

    #[test]
    fn expand_home_resolves_tilde_only() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_home("~/Code/wiki"), PathBuf::from(&home).join("Code/wiki"));
        assert_eq!(expand_home("~"), PathBuf::from(&home));
        assert_eq!(expand_home("/abs/path"), PathBuf::from("/abs/path"));
        assert_eq!(expand_home("rel~/x"), PathBuf::from("rel~/x"));
    }

    #[test]
    fn run_skips_while_a_run_is_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = ctx(dir.path());
        c.meeting_id = "m-running".into();
        running().lock().unwrap().insert("m-running".into());
        assert!(!run(&target("dex", dir.path().to_str().unwrap(), ""), &c, 5.0, false, None).unwrap());
        running().lock().unwrap().remove("m-running");
        let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
        assert!(log.contains("still in progress"), "{}", log);
        assert!(!dir.path().join("hook.done").exists());
    }

    #[test]
    fn run_skips_when_hook_done_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hook.done"), "x").unwrap();
        assert!(!run(&target("dex", dir.path().to_str().unwrap(), ""), &ctx(dir.path()), 5.0, false, None).unwrap());
        let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
        assert!(log.contains("skipped: hook.done exists"), "{}", log);
    }

    #[test]
    fn forced_run_ignores_hook_done() {
        // Gets past the marker check and fails on the missing wiki, so no claude is started.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hook.done"), "x").unwrap();
        let err = run(&target("dex", "/nonexistent/wiki", ""), &ctx(dir.path()), 5.0, true, None).unwrap_err();
        assert!(err.to_string().contains("/nonexistent/wiki"), "{}", err);
        let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
        assert!(!log.contains("skipped"), "{}", log);
    }

    #[test]
    fn run_fails_cleanly_when_the_wiki_directory_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(&target("dex", "/nonexistent/wiki", ""), &ctx(dir.path()), 5.0, false, None).unwrap_err();
        assert!(err.to_string().contains("/nonexistent/wiki"), "{}", err);
        let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
        assert!(log.contains("/nonexistent/wiki"), "{}", log);
        assert!(!dir.path().join("hook.done").exists());
    }
}
