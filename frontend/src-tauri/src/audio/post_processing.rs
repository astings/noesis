//! Generic settings section 1 (was spec 4.8): the post-enhance hook without a script. The settings
//! card says which directory belongs to which project; this module renders the prompt template,
//! composes the Claude Code run and starts it through the same logged, detached spawn as the custom hook.

use anyhow::{bail, Result};
use log::warn;
use std::path::{Path, PathBuf};
use std::process::Command;
use super::post_enhance_hook::{spawn_logged, write_done_marker, HookContext, OnExit};
use super::recording_preferences::{PostProcessingTarget, RecordingPreferences};
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

/// Defaults for the editable tool lists. The session is unattended and its input is other
/// people's words, so delete, move and assign stay denied until the user says otherwise.
pub const ALLOWED_TOOLS: [&str; 2] = ["mcp__open-knowledge", "mcp__noesis"];
pub const DENIED_TOOLS: [&str; 3] = ["mcp__open-knowledge__delete", "mcp__open-knowledge__move", "mcp__noesis__assign_meeting"];

/// Nobody reads a headless run, so a question would hang until the budget runs out.
pub const DEFAULT_PROMPT_TEMPLATE: &str = "Process the meeting recording at {folder} (project {project}, meeting id {meeting_id}, title: {title}). {instructions} Draft only: leave everything uncommitted for review. This run is unattended: do not ask questions; if this meeting is already covered, stop without changes.";

pub fn find_target<'a>(targets: &'a [PostProcessingTarget], project: &str) -> Option<&'a PostProcessingTarget> {
    targets.iter().find(|t| t.project == project && !t.directory.trim().is_empty())
}

/// Substitutes the placeholders, collapses runs of spaces, keeps unknown braces as typed.
/// An empty template, or one that renders empty, uses the built-in template.
pub fn render_prompt(template: &str, target: &PostProcessingTarget, ctx: &HookContext) -> String {
    let t = if template.trim().is_empty() { DEFAULT_PROMPT_TEMPLATE } else { template };
    let out = t.replace("{instructions}", target.instructions.trim());
    let out = out.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    // collapse before the path-like values go in, so their own spaces survive
    let out = out
        .replace("{folder}", &ctx.folder.display().to_string())
        .replace("{project}", &ctx.project)
        .replace("{title}", &ctx.title)
        .replace("{meeting_id}", &ctx.meeting_id);
    if out.is_empty() && t != DEFAULT_PROMPT_TEMPLATE {
        return render_prompt(DEFAULT_PROMPT_TEMPLATE, target, ctx);
    }
    out
}

pub fn format_budget(usd: f64) -> String {
    let s = format!("{:.2}", usd.max(0.0));
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn clean(list: &[String]) -> Vec<String> {
    list.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// An empty allowed list omits `--allowedTools`: in `-p` mode Claude then gets file edits and nothing else.
pub fn claude_args(prompt: &str, budget_usd: f64, allowed: &[String], denied: &[String]) -> Vec<String> {
    let mut a: Vec<String> = vec!["-p".into(), prompt.into(), "--permission-mode".into(), "acceptEdits".into()];
    let (allowed, denied) = (clean(allowed), clean(denied));
    if !allowed.is_empty() {
        a.push("--allowedTools".into());
        a.extend(allowed);
    }
    if !denied.is_empty() {
        a.push("--disallowedTools".into());
        a.extend(denied);
    }
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

/// A configured path must exist; empty means auto-detect. A wrong path is reported, not papered over.
pub fn resolve_agent(agent_path: &str) -> Option<PathBuf> {
    let p = agent_path.trim();
    if p.is_empty() {
        return locate_claude();
    }
    let path = expand_home(p);
    path.is_file().then_some(path)
}

fn note(folder: &Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(folder.join("hook.log")) {
        let _ = writeln!(f, "=== {} {}", chrono::Local::now().to_rfc3339(), line);
    }
}

/// Run Claude Code in the target directory for this meeting. Errors are also noted in hook.log.
/// `Ok(false)` means skipped (`hook.done` exists and `force` is off, or a run is in flight).
/// `then` runs after the child exits, once the marker is written.
pub fn run(prefs: &RecordingPreferences, target: &PostProcessingTarget, ctx: &HookContext, force: bool, then: Option<OnExit>) -> Result<bool> {
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
        note(&ctx.folder, &format!("directory {} not found", dir.display()));
        bail!("directory {} not found", dir.display());
    }
    let Some(claude) = resolve_agent(&prefs.agent_path) else {
        let msg = if prefs.agent_path.trim().is_empty() {
            "Claude Code (claude) not found in ~/.local/bin, /opt/homebrew/bin, /usr/local/bin or PATH".to_string()
        } else {
            format!("agent binary {} not found", prefs.agent_path.trim())
        };
        note(&ctx.folder, &msg);
        bail!("{}", msg);
    };
    let prompt = render_prompt(&prefs.agent_prompt_template, target, ctx);
    let args = claude_args(&prompt, prefs.post_processing_budget_usd, &prefs.agent_allowed_tools, &prefs.agent_denied_tools);
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
            warn!("Post-processing failed to start for {}: {}", ctx.meeting_id, e)
        })
        .map(|_| true)
}

/// Where the settings card says Claude Code was found.
#[tauri::command]
pub async fn locate_claude_cli(agent_path: String) -> Option<String> {
    resolve_agent(&agent_path).map(|p| p.display().to_string())
}

/// Native folder picker, used for post-processing directories and the recordings folder.
#[tauri::command]
pub async fn select_directory<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
        .await
        .map_err(|e| e.to_string())?;
    Ok(picked.map(|p| p.to_string()))
}

/// The built-in template, shown when `agent_prompt_template` is empty.
#[tauri::command]
pub fn get_default_agent_prompt_template() -> String {
    DEFAULT_PROMPT_TEMPLATE.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_with_dir(dir: &str) -> PostProcessingTarget {
        PostProcessingTarget { project: "dex".into(), directory: dir.into(), instructions: String::new(), skill: String::new() }
    }

    fn prefs() -> RecordingPreferences {
        RecordingPreferences::default()
    }

    fn ctx(folder: &Path) -> HookContext {
        HookContext { folder: folder.to_path_buf(), project: "dex".into(), title: "Weekly sync".into(), meeting_id: "m-1".into() }
    }

    #[test]
    fn find_target_matches_project_and_skips_blank_directories() {
        let blank = PostProcessingTarget { project: "dex".into(), directory: "  ".into(), instructions: "x".into(), skill: String::new() };
        let flo = PostProcessingTarget { project: "flo".into(), directory: "/w/flo".into(), instructions: String::new(), skill: String::new() };
        let t = vec![blank, target_with_dir("/w/dex"), flo];
        assert_eq!(find_target(&t, "dex").map(|t| t.directory.as_str()), Some("/w/dex"));
        assert_eq!(find_target(&t, "flo").map(|t| t.instructions.as_str()), Some(""));
        assert!(find_target(&t, "_inbox").is_none());
    }

    fn target_with(instructions: &str) -> PostProcessingTarget {
        PostProcessingTarget { project: "dex".into(), directory: "/w/dex".into(), instructions: instructions.into(), skill: String::new() }
    }

    #[test]
    fn default_template_renders_with_and_without_instructions() {
        let c = ctx(Path::new("/r/dex/2026-10-27_16-18 weekly"));
        let with = render_prompt("", &target_with("Use the dex-case-tracking skill."), &c);
        assert_eq!(with, "Process the meeting recording at /r/dex/2026-10-27_16-18 weekly (project dex, meeting id m-1, title: Weekly sync). Use the dex-case-tracking skill. Draft only: leave everything uncommitted for review. This run is unattended: do not ask questions; if this meeting is already covered, stop without changes.");
        let without = render_prompt("", &target_with("  "), &c);
        assert_eq!(without, "Process the meeting recording at /r/dex/2026-10-27_16-18 weekly (project dex, meeting id m-1, title: Weekly sync). Draft only: leave everything uncommitted for review. This run is unattended: do not ask questions; if this meeting is already covered, stop without changes.");
    }

    #[test]
    fn custom_template_keeps_unknown_placeholders_and_falls_back_when_empty() {
        let c = ctx(Path::new("/r/x"));
        assert_eq!(render_prompt("Summarize {folder} for {who}: {instructions}", &target_with("be brief"), &c), "Summarize /r/x for {who}: be brief");
        let spaced = ctx(Path::new("/r/two  spaces"));
        assert!(render_prompt("Read {folder}  now", &target_with(""), &spaced).contains("/r/two  spaces now"));
        assert_eq!(render_prompt("{instructions}", &target_with(""), &c), render_prompt("", &target_with(""), &c), "an empty render uses the built-in template");
    }

    #[test]
    fn budget_formats_without_trailing_zeros() {
        assert_eq!(format_budget(5.0), "5");
        assert_eq!(format_budget(2.5), "2.5");
        assert_eq!(format_budget(0.75), "0.75");
        assert_eq!(format_budget(0.0), "0");
    }

    #[test]
    fn claude_args_carry_the_flags_and_omit_empty_tool_lists() {
        let allowed = vec!["mcp__open-knowledge".to_string(), "mcp__noesis".to_string()];
        let denied = vec!["mcp__open-knowledge__delete".to_string()];
        assert_eq!(claude_args("hello", 5.0, &allowed, &denied), vec![
            "-p", "hello", "--permission-mode", "acceptEdits",
            "--allowedTools", "mcp__open-knowledge", "mcp__noesis",
            "--disallowedTools", "mcp__open-knowledge__delete",
            "--max-budget-usd", "5",
        ]);
        assert_eq!(claude_args("hi", 2.5, &[], &[" ".to_string()]), vec!["-p", "hi", "--permission-mode", "acceptEdits", "--max-budget-usd", "2.5"]);
    }

    #[test]
    fn resolve_agent_prefers_an_existing_configured_path() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("my-agent");
        std::fs::write(&bin, "").unwrap();
        assert_eq!(resolve_agent(bin.to_str().unwrap()), Some(bin.clone()));
        assert_eq!(resolve_agent(dir.path().join("missing").to_str().unwrap()), None, "a wrong path is an error, not a silent fallback");
        assert_eq!(resolve_agent("   "), locate_claude(), "empty means auto-detect");
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
        assert!(!run(&prefs(), &target_with_dir(dir.path().to_str().unwrap()), &c, false, None).unwrap());
        running().lock().unwrap().remove("m-running");
        let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
        assert!(log.contains("still in progress"), "{}", log);
        assert!(!dir.path().join("hook.done").exists());
    }

    #[test]
    fn run_skips_when_hook_done_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hook.done"), "x").unwrap();
        assert!(!run(&prefs(), &target_with_dir(dir.path().to_str().unwrap()), &ctx(dir.path()), false, None).unwrap());
        let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
        assert!(log.contains("skipped: hook.done exists"), "{}", log);
    }

    #[test]
    fn forced_run_ignores_hook_done() {
        // Gets past the marker check and fails on the missing wiki, so no claude is started.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hook.done"), "x").unwrap();
        let err = run(&prefs(), &target_with_dir("/nonexistent/wiki"), &ctx(dir.path()), true, None).unwrap_err();
        assert!(err.to_string().contains("/nonexistent/wiki"), "{}", err);
        let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
        assert!(!log.contains("skipped"), "{}", log);
    }

    #[test]
    fn run_fails_cleanly_when_the_wiki_directory_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(&prefs(), &target_with_dir("/nonexistent/wiki"), &ctx(dir.path()), false, None).unwrap_err();
        assert!(err.to_string().contains("/nonexistent/wiki"), "{}", err);
        let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
        assert!(log.contains("/nonexistent/wiki"), "{}", log);
        assert!(!dir.path().join("hook.done").exists());
    }
}
