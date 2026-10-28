# Phase 7: Wiki Automation Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The post-enhance hook without a script: a "Wiki automation" card in Recording settings with a switch, a table of projects (project, wiki directory, optional skill) and a cost cap. When an enhance job finishes, the app itself runs Claude Code in the matching wiki directory with a draft-only prompt and the safe flags, logging to `hook.log` and marking `hook.done`. The custom command of Phase 6 stays as the advanced fallback.

**Architecture:** Preferences gain `wiki_automation`, `wiki_targets` and `wiki_budget_usd`. A new pure-ish module `audio/wiki_automation.rs` finds the target, composes the prompt and the `claude` arguments, locates the binary, and runs it through a shared `spawn_logged` helper extracted from `post_enhance_hook.rs` (header line, `hook.log`, reaper thread, optional `hook.done` on success). The enhance queue tries the built-in run first and falls back to the custom command. Two Tauri commands serve the UI: `locate_claude_cli` and `select_wiki_directory` (native folder dialog through `tauri-plugin-dialog`, already registered in `lib.rs`). A new React component `WikiAutomationSettings.tsx` renders the card.

**Tech Stack:** Rust (std::process, serde, `which` 6, `tauri-plugin-dialog` 2.3), TypeScript/React (existing `Switch`, `Input`, `Button`, `ProjectSelect`).

Spec: `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md`, section 4.8 (and 4.7 for the hook it extends).

## Global Constraints

- Additive, marked `// spec 4.8`. Phase 6 behaviour is unchanged when `wiki_automation` is off or no target matches: the custom command runs exactly as before, and nothing else happens.
- Preferences: `wiki_automation: bool` (default `false`), `wiki_targets: Vec<WikiTarget>` (default empty) with `WikiTarget { project: String, directory: String, skill: String }` (`skill` may be empty), `wiki_budget_usd: f64` (default `5.0`). All `#[serde(default)]` so older files load.
- The built-in run executes, from `directory`, the binary found by `locate_claude()` with exactly these arguments: `-p <prompt> --permission-mode acceptEdits --allowedTools mcp__open-knowledge mcp__meetily --disallowedTools mcp__open-knowledge__delete mcp__open-knowledge__move mcp__meetily__assign_meeting --max-budget-usd <cap>` where `<cap>` is the budget formatted with at most two decimals (`5`, `2.5`). `PATH` is set to `$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:` plus the inherited `PATH` (the MCP servers need `node`). The four `MEETILY_*` variables are set as in 4.7. stdin null, stdout and stderr appended to `hook.log` after a header line `=== <RFC 3339 local time> <program> <args joined by spaces>` (the prompt may contain spaces; that is fine for a log line).
- Prompt with a skill: `Use the <skill> skill to process the new meeting recording at <folder> (project <project>, meeting id <id>, title: <title>). Draft only: write the transcript document and a draft log entry; do not score, do not update trackers or the Blueprint. Leave everything uncommitted for review.` Without a skill: `Process the new meeting recording at <folder> (project <project>, meeting id <id>, title: <title>). Draft only: write the transcript document and a draft log entry; leave everything uncommitted for review.`
- `hook.done` in the meeting folder: the built-in run is skipped when it exists (one line appended to `hook.log`: `=== <time> skipped: hook.done exists`), and written (RFC 3339 time) by the reaper thread when the child exits successfully. The custom-command path never reads or writes the marker (the reference script manages its own).
- Failure handling: missing `claude` binary, missing directory, or spawn error are logged to `hook.log` (one line) and `warn!`-logged; the enhance status stays `done`.
- `locate_claude()` checks, in order, `$HOME/.local/bin/claude`, `/opt/homebrew/bin/claude`, `/usr/local/bin/claude`, then `which::which("claude")`.
- No em-dashes anywhere (code, comments, UI strings). Never run pnpm. Rust tests: `cd frontend/src-tauri && cargo test --lib <module>`; the full suite on `main` is 292 passed, 3 ignored, 19 upstream warnings, add none. Type check: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json`.
- Branch `phase-7/wiki-automation`, one commit per task, no push.

## File Structure

| File | Responsibility |
| --- | --- |
| `frontend/src-tauri/src/audio/recording_preferences.rs` (modify) | `WikiTarget`, the three preferences |
| `frontend/src-tauri/src/audio/post_enhance_hook.rs` (modify) | Extract `spawn_logged(command, header, folder, meeting_id, mark_done)`; `spawn` uses it |
| `frontend/src-tauri/src/audio/wiki_automation.rs` (create) | `find_target`, `build_prompt`, `claude_args`, `format_budget`, `locate_claude`, `run`, Tauri commands `locate_claude_cli`, `select_wiki_directory` |
| `frontend/src-tauri/src/audio/mod.rs` (modify) | Declare the module |
| `frontend/src-tauri/src/lib.rs` (modify) | Register the two commands |
| `frontend/src-tauri/src/audio/retranscription.rs` (modify) | Built-in run first, custom command as fallback |
| `frontend/src/components/WikiAutomationSettings.tsx` (create) | The card |
| `frontend/src/components/RecordingSettings.tsx` (modify) | Types, defaults, render the card, relabel the command field |
| `CLAUDE.md`, `scripts/post-enhance.sh` (modify) | Docs: the script is now the advanced example |

---

### Task 1: Preferences

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs`
- Modify: `frontend/src/components/RecordingSettings.tsx` (interface and initial state only)

**Interfaces:**
- Produces: `pub struct WikiTarget { pub project: String, pub directory: String, #[serde(default)] pub skill: String }` deriving `Debug, Clone, PartialEq, Serialize, Deserialize`; `RecordingPreferences.wiki_automation: bool`, `.wiki_targets: Vec<WikiTarget>`, `.wiki_budget_usd: f64`; TS `WikiTarget { project: string; directory: string; skill: string }` exported from `RecordingSettings.tsx`, and the three fields on `RecordingPreferences`.

- [ ] **Step 1: Rust**

Above `RecordingPreferences`:

```rust
/// spec 4.8: one wiki per project for the built-in post-enhance run
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiTarget {
    pub project: String,
    pub directory: String,
    #[serde(default)]
    pub skill: String,
}
```

After `post_enhance_command`:

```rust
    // spec 4.8: built-in Claude Code run per project
    #[serde(default)]
    pub wiki_automation: bool,
    #[serde(default)]
    pub wiki_targets: Vec<WikiTarget>,
    #[serde(default = "default_wiki_budget_usd")]
    pub wiki_budget_usd: f64,
```

`fn default_wiki_budget_usd() -> f64 { 5.0 }`; in `impl Default`: `wiki_automation: false, wiki_targets: Vec::new(), wiki_budget_usd: default_wiki_budget_usd(),`. Test:

```rust
    #[test]
    fn wiki_automation_defaults_off_with_empty_targets_and_five_dollars() {
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert!(!p.wiki_automation);
        assert!(p.wiki_targets.is_empty());
        assert_eq!(p.wiki_budget_usd, 5.0);
        let with: RecordingPreferences = serde_json::from_str(
            r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4","wiki_targets":[{"project":"dex","directory":"/w/dex"}]}"#,
        ).unwrap();
        assert_eq!(with.wiki_targets[0].skill, "", "skill is optional in the file");
    }
```

Run: `cd frontend/src-tauri && cargo test --lib recording_preferences` (6 passed).

- [ ] **Step 2: TypeScript types**

In `RecordingSettings.tsx`, above `RecordingPreferences`: `export interface WikiTarget { project: string; directory: string; skill: string } // spec 4.8`. Fields: `wiki_automation: boolean; wiki_targets: WikiTarget[]; wiki_budget_usd: number; // spec 4.8`. Initial state: `wiki_automation: false, wiki_targets: [], wiki_budget_usd: 5,`. Run tsc (silent).

- [ ] **Step 3: Commit**

```bash
git add frontend/src-tauri/src/audio/recording_preferences.rs frontend/src/components/RecordingSettings.tsx
git commit -m "feat(wiki): wiki automation preferences"
```

---

### Task 2: `spawn_logged` and `wiki_automation.rs`

**Files:**
- Modify: `frontend/src-tauri/src/audio/post_enhance_hook.rs`
- Create: `frontend/src-tauri/src/audio/wiki_automation.rs`
- Modify: `frontend/src-tauri/src/audio/mod.rs` (after `pub mod post_enhance_hook;`)

**Interfaces:**
- Consumes: `HookContext`, `read_context` (unchanged), `WikiTarget`.
- Produces in `post_enhance_hook.rs`: `pub fn spawn_logged(mut command: Command, header: &str, folder: &Path, meeting_id: &str, mark_done: bool) -> Result<()>` (opens `hook.log` append, writes `=== <time> <header>`, wires stdin/stdout/stderr, spawns, reaper thread logs the exit and writes `hook.done` when `mark_done` and the status is success). `spawn(command: &str, ctx)` builds the `sh -c` Command with cwd and env and calls `spawn_logged(cmd, &format!("sh -c {}", command.replace('\n', " ")), &ctx.folder, &ctx.meeting_id, false)`. Existing tests must keep passing unchanged.
- Produces in `wiki_automation.rs`: `pub const ALLOWED_TOOLS: [&str; 2]`, `pub const DENIED_TOOLS: [&str; 3]`, `pub fn find_target<'a>(targets: &'a [WikiTarget], project: &str) -> Option<&'a WikiTarget>` (exact match on `project`, ignoring rows whose `directory` is blank), `pub fn build_prompt(target: &WikiTarget, ctx: &HookContext) -> String`, `pub fn format_budget(usd: f64) -> String` (`5` for 5.0, `2.5` for 2.5, `0.75` for 0.75; two decimals max, trailing zeros trimmed), `pub fn claude_args(prompt: &str, budget_usd: f64) -> Vec<String>`, `pub fn locate_claude() -> Option<PathBuf>`, `pub fn hook_path_env() -> String` (`$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:` + inherited PATH), `pub fn run(target: &WikiTarget, ctx: &HookContext, budget_usd: f64) -> Result<()>`.

- [ ] **Step 1: Failing tests** (in `wiki_automation.rs`)

```rust
//! spec 4.8: the post-enhance hook without a script. The settings table says which wiki
//! belongs to which project; this module composes the Claude Code run and starts it
//! through the same logged, detached spawn as the custom hook.

use anyhow::{bail, Context, Result};
use log::warn;
use std::path::{Path, PathBuf};
use std::process::Command;
use super::post_enhance_hook::{spawn_logged, HookContext};
use super::recording_preferences::WikiTarget;

#[cfg(test)]
mod tests {
    use super::*;

    fn target(project: &str, dir: &str, skill: &str) -> WikiTarget {
        WikiTarget { project: project.into(), directory: dir.into(), skill: skill.into() }
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
        assert_eq!(with, "Use the dex-case-tracking skill to process the new meeting recording at /r/dex/2026-10-27_16-18 weekly (project dex, meeting id m-1, title: Weekly sync). Draft only: write the transcript document and a draft log entry; do not score, do not update trackers or the Blueprint. Leave everything uncommitted for review.");
        let without = build_prompt(&target("dex", "/w/dex", ""), &c);
        assert_eq!(without, "Process the new meeting recording at /r/dex/2026-10-27_16-18 weekly (project dex, meeting id m-1, title: Weekly sync). Draft only: write the transcript document and a draft log entry; leave everything uncommitted for review.");
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
            "--allowedTools", "mcp__open-knowledge", "mcp__meetily",
            "--disallowedTools", "mcp__open-knowledge__delete", "mcp__open-knowledge__move", "mcp__meetily__assign_meeting",
            "--max-budget-usd", "5",
        ]);
    }

    #[test]
    fn hook_path_puts_the_user_bins_first() {
        let p = hook_path_env();
        assert!(p.starts_with(&format!("{}/.local/bin:/opt/homebrew/bin:/usr/local/bin:", std::env::var("HOME").unwrap())), "{}", p);
    }

    #[test]
    fn run_skips_when_hook_done_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hook.done"), "x").unwrap();
        run(&target("dex", dir.path().to_str().unwrap(), ""), &ctx(dir.path()), 5.0).unwrap();
        let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
        assert!(log.contains("skipped: hook.done exists"), "{}", log);
    }

    #[test]
    fn run_fails_cleanly_when_the_wiki_directory_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(&target("dex", "/nonexistent/wiki", ""), &ctx(dir.path()), 5.0).unwrap_err();
        assert!(err.to_string().contains("/nonexistent/wiki"), "{}", err);
        let log = std::fs::read_to_string(dir.path().join("hook.log")).unwrap();
        assert!(log.contains("/nonexistent/wiki"), "{}", log);
        assert!(!dir.path().join("hook.done").exists());
    }
}
```

And in `post_enhance_hook.rs` tests, add:

```rust
    #[test]
    fn spawn_logged_marks_done_only_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let mut ok = Command::new("sh");
        ok.args(["-c", "echo END"]);
        spawn_logged(ok, "sh -c echo END", dir.path(), "m-ok", true).unwrap();
        wait_for(&dir.path().join("hook.log"));
        for _ in 0..250 {
            if dir.path().join("hook.done").exists() { break; }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(dir.path().join("hook.done").exists(), "marker after a successful run");

        let dir2 = tempfile::tempdir().unwrap();
        let mut bad = Command::new("sh");
        bad.args(["-c", "echo END; exit 3"]);
        spawn_logged(bad, "sh -c exit 3", dir2.path(), "m-bad", true).unwrap();
        wait_for(&dir2.path().join("hook.log"));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!dir2.path().join("hook.done").exists(), "no marker after a failure");
    }
```

Add to `audio/mod.rs` after `pub mod post_enhance_hook;`: `// spec 4.8: built-in wiki automation` and `pub mod wiki_automation;`.

Run: `cd frontend/src-tauri && cargo test --lib wiki_automation` (compile errors).

- [ ] **Step 2: Implement `spawn_logged`** in `post_enhance_hook.rs`

```rust
/// Start `command` detached, logging to <folder>/hook.log after a header line.
/// With `mark_done`, a successful exit writes <folder>/hook.done.
pub fn spawn_logged(mut command: Command, header: &str, folder: &Path, meeting_id: &str, mark_done: bool) -> Result<()> {
    let log_path = folder.join("hook.log");
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("open {}", log_path.display()))?;
    writeln!(log, "=== {} {}", chrono::Local::now().to_rfc3339(), header)?;
    let stderr = log.try_clone().context("clone hook.log handle")?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .spawn()
        .context("spawn post-enhance hook")?;
    info!("Post-enhance hook started (pid {}) for {} in {}", child.id(), meeting_id, folder.display());
    let meeting_id = meeting_id.to_string();
    let marker = folder.join("hook.done");
    std::thread::spawn(move || match child.wait() {
        Ok(status) => {
            info!("Post-enhance hook for {} exited with {}", meeting_id, status);
            if mark_done && status.success() {
                if let Err(e) = std::fs::write(&marker, format!("{}\n", chrono::Local::now().to_rfc3339())) {
                    warn!("Could not write {}: {}", marker.display(), e);
                }
            }
        }
        Err(e) => warn!("Post-enhance hook for {} could not be waited on: {}", meeting_id, e),
    });
    Ok(())
}
```

`spawn` becomes:

```rust
pub fn spawn(command: &str, ctx: &HookContext) -> Result<()> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(&ctx.folder)
        .env("MEETILY_FOLDER", &ctx.folder)
        .env("MEETILY_PROJECT", &ctx.project)
        .env("MEETILY_TITLE", &ctx.title)
        .env("MEETILY_MEETING_ID", &ctx.meeting_id);
    spawn_logged(cmd, &format!("sh -c {}", command.replace('\n', " ")), &ctx.folder, &ctx.meeting_id, false)
}
```

- [ ] **Step 3: Implement `wiki_automation.rs`**

```rust
pub const ALLOWED_TOOLS: [&str; 2] = ["mcp__open-knowledge", "mcp__meetily"];
pub const DENIED_TOOLS: [&str; 3] = ["mcp__open-knowledge__delete", "mcp__open-knowledge__move", "mcp__meetily__assign_meeting"];

pub fn find_target<'a>(targets: &'a [WikiTarget], project: &str) -> Option<&'a WikiTarget> {
    targets.iter().find(|t| t.project == project && !t.directory.trim().is_empty())
}

pub fn build_prompt(target: &WikiTarget, ctx: &HookContext) -> String {
    let where_ = format!(
        "the new meeting recording at {} (project {}, meeting id {}, title: {})",
        ctx.folder.display(), ctx.project, ctx.meeting_id, ctx.title
    );
    let skill = target.skill.trim();
    if skill.is_empty() {
        format!("Process {}. Draft only: write the transcript document and a draft log entry; leave everything uncommitted for review.", where_)
    } else {
        format!("Use the {} skill to process {}. Draft only: write the transcript document and a draft log entry; do not score, do not update trackers or the Blueprint. Leave everything uncommitted for review.", skill, where_)
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
pub fn run(target: &WikiTarget, ctx: &HookContext, budget_usd: f64) -> Result<()> {
    if ctx.folder.join("hook.done").exists() {
        note(&ctx.folder, "skipped: hook.done exists");
        return Ok(());
    }
    let dir = PathBuf::from(target.directory.trim());
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
        .env("MEETILY_FOLDER", &ctx.folder)
        .env("MEETILY_PROJECT", &ctx.project)
        .env("MEETILY_TITLE", &ctx.title)
        .env("MEETILY_MEETING_ID", &ctx.meeting_id);
    let header = format!("{} {}", claude.display(), args.join(" "));
    spawn_logged(cmd, &header, &ctx.folder, &ctx.meeting_id, true)
        .inspect_err(|e| warn!("Wiki automation failed to start for {}: {}", ctx.meeting_id, e))
}
```

(If `Result::inspect_err` is unavailable on this toolchain, use `.map_err(|e| { warn!(...); e })`.)

- [ ] **Step 4: Verify and commit**

Run: `cd frontend/src-tauri && cargo test --lib wiki_automation && cargo test --lib post_enhance_hook && cargo check 2>&1 | grep -E "^error|^warning: unused"`
Expected: 7 wiki tests and 4 hook tests pass; no errors or new warnings.

```bash
git add frontend/src-tauri/src/audio/post_enhance_hook.rs frontend/src-tauri/src/audio/wiki_automation.rs frontend/src-tauri/src/audio/mod.rs
git commit -m "feat(wiki): compose and run Claude Code per project through the logged hook spawn"
```

---

### Task 3: Dispatch and Tauri commands

**Files:**
- Modify: `frontend/src-tauri/src/audio/retranscription.rs` (the `Ok(res)` hook block)
- Modify: `frontend/src-tauri/src/audio/wiki_automation.rs` (two commands at the end, above the tests)
- Modify: `frontend/src-tauri/src/lib.rs` (register in `generate_handler!`, next to `audio::library::list_projects_command`)

**Interfaces:**
- Produces: `#[tauri::command] pub async fn locate_claude_cli() -> Option<String>` and `#[tauri::command] pub async fn select_wiki_directory<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String>`.

- [ ] **Step 1: Dispatch**

Replace the body inside `if res.segments_count > 0 { match load_recording_preferences(...) { Ok(prefs) => { ... } ... } }` with:

```rust
                    Ok(prefs) => {
                        let ctx = super::post_enhance_hook::read_context(&folder, &meeting_id);
                        // spec 4.8: the built-in run wins when the project has a wiki row
                        let built_in = prefs.wiki_automation
                            && match super::wiki_automation::find_target(&prefs.wiki_targets, &ctx.project) {
                                Some(target) => {
                                    if let Err(e) = super::wiki_automation::run(target, &ctx, prefs.wiki_budget_usd) {
                                        warn!("Wiki automation did not start for {}: {}", meeting_id, e);
                                    }
                                    true
                                }
                                None => false,
                            };
                        let command = prefs.post_enhance_command.trim();
                        if !built_in && !command.is_empty() {
                            if let Err(e) = super::post_enhance_hook::spawn(command, &ctx) {
                                warn!("Post-enhance hook failed to start for {}: {}", meeting_id, e);
                            }
                        }
                    }
```

(A matched target counts as handled even when the run fails to start, so the custom command does not double-process.)

- [ ] **Step 2: Commands**

```rust
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
```

(`FilePath` implements `Display`; if `to_string` does not compile, use `p.into_path().map_err(|e| e.to_string())?.display().to_string()`.) Register both in `lib.rs`:

```rust
            audio::wiki_automation::locate_claude_cli,
            audio::wiki_automation::select_wiki_directory,
```

- [ ] **Step 3: Verify and commit**

Run: `cd frontend/src-tauri && cargo check 2>&1 | grep -E "^error|^warning: unused" ; cargo test --lib retranscription && cargo test --lib wiki_automation`
Expected: no errors or new warnings; 18 and 7 passed.

```bash
git add frontend/src-tauri/src/audio/retranscription.rs frontend/src-tauri/src/audio/wiki_automation.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(wiki): built-in run first, custom command as fallback; locate and folder-picker commands"
```

---

### Task 4: Settings card

**Files:**
- Create: `frontend/src/components/WikiAutomationSettings.tsx`
- Modify: `frontend/src/components/RecordingSettings.tsx` (render the card after the enhance block, before the "Run after enhance" card; relabel that card)

**Interfaces:**
- Consumes: `RecordingPreferences`, `WikiTarget` (Task 1), `ProjectSelect` (`value`, `onChange`, `compact`), `Switch`, `Input`, `Button` from `@/components/ui/*`, `invoke` from `@tauri-apps/api/core`.
- Produces: `export function WikiAutomationSettings({ preferences, onChange, disabled }: { preferences: RecordingPreferences; onChange: (next: RecordingPreferences) => Promise<void> | void; disabled?: boolean })`.

- [ ] **Step 1: Component**

```tsx
'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Switch } from '@/components/ui/switch';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { ProjectSelect } from '@/components/ProjectSelect';
import type { RecordingPreferences, WikiTarget } from '@/components/RecordingSettings';

interface Props {
  preferences: RecordingPreferences;
  onChange: (next: RecordingPreferences) => Promise<void> | void;
  disabled?: boolean;
}

/** spec 4.8: one wiki per project; the app runs Claude Code there after each enhance. */
export function WikiAutomationSettings({ preferences, onChange, disabled }: Props) {
  const [claudePath, setClaudePath] = useState<string | null | undefined>(undefined);
  const [draft, setDraft] = useState<WikiTarget[] | null>(null);
  const [budgetDraft, setBudgetDraft] = useState<string | null>(null);

  useEffect(() => {
    invoke<string | null>('locate_claude_cli').then(setClaudePath).catch(() => setClaudePath(null));
  }, []);

  const rows = draft ?? preferences.wiki_targets;
  const commitRows = async (next: WikiTarget[]) => {
    setDraft(null);
    await onChange({ ...preferences, wiki_targets: next });
  };
  const updateRow = (i: number, patch: Partial<WikiTarget>) => {
    const next = rows.map((r, j) => (j === i ? { ...r, ...patch } : r));
    setDraft(next);
    return next;
  };
  const browse = async (i: number) => {
    const picked = await invoke<string | null>('select_wiki_directory').catch(() => null);
    if (picked) await commitRows(updateRow(i, { directory: picked }));
  };
  const commitBudget = async () => {
    const value = parseFloat(budgetDraft ?? String(preferences.wiki_budget_usd));
    setBudgetDraft(null);
    if (!Number.isFinite(value) || value < 0 || value === preferences.wiki_budget_usd) return;
    await onChange({ ...preferences, wiki_budget_usd: value });
  };

  return (
    <div className="p-4 border rounded-lg space-y-3">
      <div className="flex items-center justify-between gap-4">
        <div>
          <div className="font-medium">Wiki automation</div>
          <div className="text-sm text-gray-600">
            After each enhance, run Claude Code in the project's wiki to draft the transcript document and a log entry. Nothing is committed, scored or moved.
          </div>
        </div>
        <Switch
          checked={preferences.wiki_automation}
          onCheckedChange={(on) => onChange({ ...preferences, wiki_automation: on })}
          disabled={disabled}
          aria-label="Wiki automation"
        />
      </div>
      <div className="text-xs text-gray-500">
        {claudePath === undefined ? 'Looking for Claude Code...' : claudePath ? `Claude Code: ${claudePath}` : 'Claude Code not found. Install it and sign in, then reopen settings.'}
      </div>

      <div className="space-y-2">
        {rows.map((row, i) => (
          <div key={i} className="grid grid-cols-[10rem_1fr_10rem_auto] gap-2 items-center">
            <ProjectSelect compact value={row.project} onChange={(p) => commitRows(updateRow(i, { project: p }))} disabled={disabled} />
            <div className="flex gap-1">
              <Input
                aria-label="Wiki directory"
                className="font-mono text-xs"
                placeholder="/Users/you/Code/wiki"
                value={row.directory}
                onChange={(e) => updateRow(i, { directory: e.target.value })}
                onBlur={() => draft && commitRows(draft)}
                disabled={disabled}
              />
              <Button type="button" variant="outline" size="sm" onClick={() => browse(i)} disabled={disabled}>Browse</Button>
            </div>
            <Input
              aria-label="Skill (optional)"
              className="text-xs"
              placeholder="skill (optional)"
              value={row.skill}
              onChange={(e) => updateRow(i, { skill: e.target.value })}
              onBlur={() => draft && commitRows(draft)}
              disabled={disabled}
            />
            <Button type="button" variant="ghost" size="sm" onClick={() => commitRows(rows.filter((_, j) => j !== i))} disabled={disabled} aria-label="Remove row">Remove</Button>
          </div>
        ))}
        <Button type="button" variant="outline" size="sm" onClick={() => commitRows([...rows, { project: '', directory: '', skill: '' }])} disabled={disabled}>
          Add project
        </Button>
      </div>

      <div className="flex items-center gap-2">
        <div className="text-sm text-gray-600">Cost cap per run (USD)</div>
        <Input
          aria-label="Cost cap per run in USD"
          className="w-24"
          type="number"
          min={0}
          step={0.5}
          value={budgetDraft ?? String(preferences.wiki_budget_usd)}
          onChange={(e) => setBudgetDraft(e.target.value)}
          onBlur={commitBudget}
          onKeyDown={(e) => { if (e.key === 'Enter') e.currentTarget.blur(); }}
          disabled={disabled}
        />
      </div>
    </div>
  );
}
```

Check that `Button` exists at `@/components/ui/button` with `variant` and `size` props (grep); if the `variant`/`size` names differ, use the ones the file defines.

- [ ] **Step 2: Wire it in `RecordingSettings.tsx`**

Import `WikiAutomationSettings`. Add a handler next to `commitHookCommand`:

```ts
  // spec 4.8
  const applyPreferences = async (next: RecordingPreferences) => {
    setPreferences(next);
    await savePreferences(next);
  };
```

Render, directly before the `{/* spec 4.7: post-enhance hook */}` block:

```tsx
      {/* spec 4.8: wiki automation */}
      <WikiAutomationSettings preferences={preferences} onChange={applyPreferences} disabled={saving} />
```

Relabel the 4.7 card: title `Custom command (advanced)`, description `Shell command started after an enhance for projects without a wiki row above. It gets MEETILY_FOLDER, MEETILY_PROJECT, MEETILY_TITLE and MEETILY_MEETING_ID; output goes to hook.log in the meeting folder. Empty means off.`

Run: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json` (silent).

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/WikiAutomationSettings.tsx frontend/src/components/RecordingSettings.tsx
git commit -m "feat(wiki): wiki automation settings card"
```

---

### Task 5: Docs

**Files:**
- Modify: `CLAUDE.md` (the 4.7 bullet), `scripts/post-enhance.sh` (header comment)

- [ ] **Step 1:** Replace the post-enhance bullet in CLAUDE.md with:

```markdown
- Post-enhance automation (spec 4.7 and 4.8): with "Wiki automation" on, the app runs Claude Code in the wiki directory configured for the meeting's project after a successful enhance (draft only, deny list, cost cap), logging to `hook.log` and marking `hook.done`. Projects without a row fall through to the `post_enhance_command` preference, run through `sh -c` with `MEETILY_FOLDER`, `MEETILY_PROJECT`, `MEETILY_TITLE` and `MEETILY_MEETING_ID`. `scripts/post-enhance.sh` is the advanced example of such a command.
```

In `scripts/post-enhance.sh`, change the first comment line to: `# spec 4.7: reference post-enhance command for Meetily (advanced). Most users should use the "Wiki automation" table in Settings instead; this script does the same by hand.`

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md scripts/post-enhance.sh
git commit -m "docs(wiki): wiki automation replaces the script for daily use"
```

---

### Task 6: Manual acceptance (owner)

- [ ] **Step 1:** Settings, Recording: the "Wiki automation" card shows `Claude Code: /Users/.../.local/bin/claude`. Add a row: project `dex`, Browse to the DEX wiki, skill `dex-case-tracking`. Switch on. Leave the custom command empty.
- [ ] **Step 2:** Record 20 seconds in `dex`, stop, wait for the enhance. Expected: `hook.log` in the meeting folder shows the header with the `claude` path and arguments, then Claude's output; `hook.done` appears when it finishes; the DEX wiki has an uncommitted draft transcript doc and log entry.
- [ ] **Step 3:** Click Enhance again on the same meeting. Expected: one `skipped: hook.done exists` line in `hook.log`.
- [ ] **Step 4:** Record 20 seconds in `_inbox`. Expected: no `hook.log` at all (no row, no custom command).
- [ ] **Step 5:** Finish with `superpowers:finishing-a-development-branch` (squash merge to `main`).

---

## Self-review against the spec

- 4.8 preferences, table UI, Claude detection, app-composed run with the exact flags, `hook.done` skip and mark, fallback to the 4.7 command: Tasks 1 to 4.
- 4.7 unchanged when the switch is off: the dispatch only adds a branch ahead of the existing one.
- Names used across tasks: `WikiTarget`, `wiki_automation`, `wiki_targets`, `wiki_budget_usd`, `spawn_logged`, `find_target`, `build_prompt`, `format_budget`, `claude_args`, `hook_path_env`, `locate_claude`, `run`, `locate_claude_cli`, `select_wiki_directory`, `WikiAutomationSettings`, `applyPreferences`.

## Outcome

Accepted by the owner on 2026-10-27 (an existing meeting moved to `dex` and re-enhanced ran Claude Code in the DEX wiki with the skill, logged to `hook.log` and wrote `hook.done`) and squash-merged into `main`. Review fixes folded in: no empty select item for a new row (crashed the settings view); one Claude run per meeting at a time (in-process set, released after `hook.done` is written); `~` paths; positive budget only; `spawn_logged` takes an exit callback. After acceptance the prompt gained "This run is unattended: do not ask questions; if the wiki already covers this meeting, stop without changes", because the first real run ended by asking which option to take.

Deferred follow-ups (none blocks daily use):
- In the wiki rows, a blur commit can race a Browse or Remove click made right after typing; the typed text may be lost once.
- The in-flight set leaks an id only if the OS refuses to start the reaper thread (same as Phase 6).
- The custom-command path and the built-in run cannot both run for one meeting by design; a project row always wins.
- The script `scripts/post-enhance.sh` duplicates the prompt wording; keep them aligned when either changes.
- Distribution (signing, notarization, bundling the automation defaults) is a separate decision, see the discussion of 2026-10-27.
