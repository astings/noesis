# Generic Settings, Plan A: post-processing card, safe defaults, real recordings root

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the owner-specific "Wiki automation", the owner's baked-in defaults and the fixed recordings root into settings with safe defaults, as sections 1, 3 and 4 of `docs/superpowers/specs/2026-10-28-generic-settings-for-distribution-design.md`.

**Architecture:** Preference keys are renamed through `#[serde(alias)]` so existing files load unchanged. The wiki module becomes `audio/post_processing.rs` and renders a user-editable prompt template. One `recordings_root(app)` reads the `save_folder` preference and replaces every `get_default_recordings_folder()` runtime call. Defaults derive from the Mac (locale, account name) or flip to the safe value.

**Tech Stack:** Rust (Tauri 2, serde, tauri-plugin-store, objc2-foundation 0.3), Next.js/TypeScript settings UI, Node 22 MCP server.

## Global Constraints

- Branch: `enhance/generic-settings`, created from `main` before Task 1. Plan B continues on the same branch.
- Rust tests: `cd frontend/src-tauri && cargo test --lib <filter>`. One pre-existing failure is known and unrelated: `audio::device_detection::tests::test_builtin_mic_detection`.
- Type check: `cd frontend && ./node_modules/.bin/tsc --noEmit -p tsconfig.json`. No ESLint config exists.
- MCP tests: `cd mcp && npm test`.
- Before editing Rust, check no `tauri:dev` from another session is hot-rebuilding this tree: `pgrep -fl "tauri dev"` must print nothing.
- Preference key renames use `#[serde(alias = "<old name>")]`; the new name is what gets written. Stored values on the owner's Mac must load with identical behaviour.
- Copy: the card is called "Post-processing". Speaker label in the UI stays "Me".
- Changes are additive and live in fork-only modules; nothing upstream is deleted for taste.

---

### Task 1: Preference keys and the post-processing target

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs` (struct at lines 13-87, `Default` at 116-147, tests from line 480)

**Interfaces:**
- Produces: `pub struct PostProcessingTarget { pub project: String, pub directory: String, pub instructions: String, pub skill: String }` (skill is legacy, read only)
- Produces: fields `post_processing: bool`, `post_processing_targets: Vec<PostProcessingTarget>`, `post_processing_budget_usd: f64`, `agent_path: String`, `agent_prompt_template: String`, `agent_allowed_tools: Vec<String>`, `agent_denied_tools: Vec<String>` on `RecordingPreferences`
- Produces: `pub fn normalize_legacy_targets(targets: &mut [PostProcessingTarget])`
- Produces: `pub fn default_allowed_tools() -> Vec<String>`, `pub fn default_denied_tools() -> Vec<String>`

- [ ] **Step 1: Create the branch**

```bash
cd /Users/lainealexandre/Code/Perso/noesis && git checkout -b enhance/generic-settings
```

- [ ] **Step 2: Write the failing tests**

Replace the test `wiki_automation_defaults_off_with_empty_targets_and_five_dollars` (lines 538-549) with:

```rust
    #[test]
    fn post_processing_defaults_off_with_empty_targets_and_five_dollars() {
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert!(!p.post_processing);
        assert!(p.post_processing_targets.is_empty());
        assert_eq!(p.post_processing_budget_usd, 5.0);
        assert_eq!(p.agent_path, "");
        assert_eq!(p.agent_prompt_template, "");
        assert_eq!(p.agent_allowed_tools, vec!["mcp__open-knowledge", "mcp__noesis"]);
        assert_eq!(p.agent_denied_tools, vec!["mcp__open-knowledge__delete", "mcp__open-knowledge__move", "mcp__noesis__assign_meeting"]);
    }

    #[test]
    fn old_wiki_keys_load_through_aliases_and_skill_becomes_instructions() {
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4",
            "wiki_automation":true,"wiki_budget_usd":2.5,
            "wiki_targets":[{"project":"dex","directory":"/w/dex","skill":"dex-case-tracking"},{"project":"flo","directory":"/w/flo"}]}"#;
        let mut p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert!(p.post_processing);
        assert_eq!(p.post_processing_budget_usd, 2.5);
        assert_eq!(p.post_processing_targets.len(), 2);
        normalize_legacy_targets(&mut p.post_processing_targets);
        assert_eq!(p.post_processing_targets[0].instructions, "Use the dex-case-tracking skill.");
        assert_eq!(p.post_processing_targets[0].skill, "");
        assert_eq!(p.post_processing_targets[1].instructions, "");
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("post_processing").is_some() && json.get("wiki_automation").is_none(), "new names are written");
        assert!(json["post_processing_targets"][0].get("skill").is_none(), "skill is never written back");
    }

    #[test]
    fn explicit_instructions_win_over_a_legacy_skill() {
        let mut t = vec![PostProcessingTarget { project: "dex".into(), directory: "/w".into(), instructions: "Keep it short.".into(), skill: "x".into() }];
        normalize_legacy_targets(&mut t);
        assert_eq!(t[0].instructions, "Keep it short.");
        assert_eq!(t[0].skill, "");
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib recording_preferences::tests 2>&1 | tail -20`
Expected: compile errors naming `post_processing`, `PostProcessingTarget`, `normalize_legacy_targets`.

- [ ] **Step 4: Rename the struct and the fields**

Replace lines 13-21 (`WikiTarget`) with:

```rust
/// spec 2026-10-28 generic settings, section 1: one directory per project for the post-processing run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostProcessingTarget {
    pub project: String,
    pub directory: String,
    /// Free text appended to the prompt template, e.g. "Use the dex-case-tracking skill."
    #[serde(default)]
    pub instructions: String,
    /// Legacy (spec 4.8): read once, turned into `instructions` by `normalize_legacy_targets`, never written.
    #[serde(default, skip_serializing)]
    pub skill: String,
}

/// `skill` from an older file becomes an instructions sentence unless instructions were already set.
pub fn normalize_legacy_targets(targets: &mut [PostProcessingTarget]) {
    for t in targets.iter_mut() {
        if t.instructions.trim().is_empty() && !t.skill.trim().is_empty() {
            t.instructions = format!("Use the {} skill.", t.skill.trim());
        }
        t.skill.clear();
    }
}

pub fn default_allowed_tools() -> Vec<String> {
    crate::audio::post_processing::ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect()
}

pub fn default_denied_tools() -> Vec<String> {
    crate::audio::post_processing::DENIED_TOOLS.iter().map(|s| s.to_string()).collect()
}
```

Replace the four wiki fields (lines 61-68) with:

```rust
    // generic settings section 1: built-in Claude Code run per project (was spec 4.8 "wiki automation")
    #[serde(default, alias = "wiki_automation")]
    pub post_processing: bool,
    #[serde(default, alias = "wiki_targets")]
    pub post_processing_targets: Vec<PostProcessingTarget>,
    #[serde(default = "default_wiki_budget_usd", alias = "wiki_budget_usd")]
    pub post_processing_budget_usd: f64,
    /// Path to the agent binary; empty means auto-detect `claude`.
    #[serde(default)]
    pub agent_path: String,
    /// Prompt template with {folder} {project} {title} {meeting_id} {instructions}; empty means built-in.
    #[serde(default)]
    pub agent_prompt_template: String,
    #[serde(default = "default_allowed_tools")]
    pub agent_allowed_tools: Vec<String>,
    #[serde(default = "default_denied_tools")]
    pub agent_denied_tools: Vec<String>,
```

In `Default::default()` replace the three wiki lines with:

```rust
            post_processing: false,
            post_processing_targets: Vec::new(),
            post_processing_budget_usd: default_wiki_budget_usd(),
            agent_path: String::new(),
            agent_prompt_template: String::new(),
            agent_allowed_tools: default_allowed_tools(),
            agent_denied_tools: default_denied_tools(),
```

In `load_recording_preferences`, inside the `Ok(mut p) => {` arm, add before `p`:

```rust
                normalize_legacy_targets(&mut p.post_processing_targets);
```

- [ ] **Step 5: Point the two other Rust users at the new names**

`frontend/src-tauri/src/audio/post_enhance_hook.rs` line 159:

```rust
    let target = if prefs.post_processing { super::wiki_automation::find_target(&prefs.post_processing_targets, &ctx.project) } else { None };
```

Line 182: `prefs.wiki_budget_usd` becomes `prefs.post_processing_budget_usd`.

Lines 407-408 (test):

```rust
        prefs.post_processing = true;
        prefs.post_processing_targets = vec![super::super::recording_preferences::PostProcessingTarget { project: "dex".into(), directory: "/nonexistent/wiki".into(), instructions: String::new(), skill: String::new() }];
```

`frontend/src-tauri/src/audio/wiki_automation.rs`: replace `use super::recording_preferences::WikiTarget;` with `use super::recording_preferences::PostProcessingTarget;`, rename every `WikiTarget` to `PostProcessingTarget`, and in the test helper `target()` build `PostProcessingTarget { project: project.into(), directory: dir.into(), instructions: String::new(), skill: skill.into() }`. The module itself is rewritten in Task 2; this step only keeps it compiling. `ALLOWED_TOOLS` and `DENIED_TOOLS` are referenced as `crate::audio::post_processing::...` from Task 1 Step 4, so add to `frontend/src-tauri/src/audio/mod.rs` after line 71:

```rust
// generic settings section 1: Task 2 renames the file; until then the new path is an alias
pub use self::wiki_automation as post_processing;
```

- [ ] **Step 6: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib recording_preferences::tests 2>&1 | tail -20`
Expected: all `recording_preferences::tests` pass. Then `cargo test --lib post_enhance_hook 2>&1 | tail -5` passes.

- [ ] **Step 7: Commit**

```bash
git add frontend/src-tauri/src/audio/recording_preferences.rs frontend/src-tauri/src/audio/post_enhance_hook.rs frontend/src-tauri/src/audio/wiki_automation.rs frontend/src-tauri/src/audio/mod.rs
git commit -m "feat(prefs): post_processing keys with wiki aliases, agent fields, instructions replace skill"
```

---

### Task 2: The post-processing module

**Files:**
- Rename: `frontend/src-tauri/src/audio/wiki_automation.rs` to `frontend/src-tauri/src/audio/post_processing.rs`
- Modify: `frontend/src-tauri/src/audio/mod.rs:70-72`, `frontend/src-tauri/src/audio/post_enhance_hook.rs:124-127,159,182-187`, `frontend/src-tauri/src/audio/hook_status.rs:97-99`, `frontend/src-tauri/src/jobs.rs:19` and its `STEP_WIKI` uses, `frontend/src-tauri/src/lib.rs:916-917`

**Interfaces:**
- Consumes: `PostProcessingTarget`, the `agent_*` and `post_processing_*` fields from Task 1
- Produces: `pub const DEFAULT_PROMPT_TEMPLATE: &str`
- Produces: `pub fn render_prompt(template: &str, target: &PostProcessingTarget, ctx: &HookContext) -> String`
- Produces: `pub fn claude_args(prompt: &str, budget_usd: f64, allowed: &[String], denied: &[String]) -> Vec<String>`
- Produces: `pub fn resolve_agent(agent_path: &str) -> Option<PathBuf>`
- Produces: `pub fn run(prefs: &RecordingPreferences, target: &PostProcessingTarget, ctx: &HookContext, force: bool, then: Option<OnExit>) -> Result<bool>`
- Produces: Tauri commands `locate_claude_cli` (unchanged), `select_directory` (renamed from `select_wiki_directory`), `get_default_agent_prompt_template`
- Produces: `HookKind::Agent` (renamed from `HookKind::Wiki`), `jobs::STEP_AGENT = "Agent"` (renamed from `STEP_WIKI`)

- [ ] **Step 1: Rename the file and module**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend/src-tauri && git mv src/audio/wiki_automation.rs src/audio/post_processing.rs
```

In `src/audio/mod.rs` replace lines 70-72 (the comment, `pub mod wiki_automation;` and the alias added in Task 1) with:

```rust
// generic settings section 1: built-in Claude Code run per project (was spec 4.8)
pub mod post_processing;
```

Then replace every `wiki_automation::` with `post_processing::` in `src/audio/post_enhance_hook.rs` and `src/lib.rs` (`rg -l "wiki_automation" src` must then print nothing).

- [ ] **Step 2: Write the failing tests**

In `src/audio/post_processing.rs`, replace the tests `prompt_names_the_skill_when_set_and_asks_for_a_draft` and `claude_args_carry_the_safe_flags_in_order` with:

```rust
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
        assert_eq!(render_prompt("{instructions}", &target_with(""), &c), render_prompt("", &target_with(""), &c), "an empty render uses the built-in template");
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
```

Update the remaining tests in the file: every `target("dex", <dir>, "")` call becomes `target_with_dir(<dir>)` where

```rust
    fn target_with_dir(dir: &str) -> PostProcessingTarget {
        PostProcessingTarget { project: "dex".into(), directory: dir.into(), instructions: String::new(), skill: String::new() }
    }
```

and every `run(<target>, &c, 5.0, <force>, None)` becomes `run(&prefs(), <target>, &c, <force>, None)` where

```rust
    fn prefs() -> super::recording_preferences::RecordingPreferences {
        super::recording_preferences::RecordingPreferences::default()
    }
```

`find_target_matches_project_and_skips_blank_directories` keeps its shape with `target_with_dir` and a hand-built row for the blank directory: `PostProcessingTarget { project: "dex".into(), directory: "  ".into(), instructions: "x".into(), skill: String::new() }`; its second assertion becomes `assert_eq!(find_target(&t, "flo").map(|t| t.instructions.as_str()), Some(""));` with `flo` built as `PostProcessingTarget { project: "flo".into(), directory: "/w/flo".into(), instructions: String::new(), skill: String::new() }`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib post_processing 2>&1 | tail -20`
Expected: compile errors for `render_prompt`, `resolve_agent`, the new `claude_args` and `run` signatures.

- [ ] **Step 4: Rewrite the module body**

Replace everything from the module doc comment down to (not including) `fn note(` with:

```rust
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
    let out = t
        .replace("{folder}", &ctx.folder.display().to_string())
        .replace("{project}", &ctx.project)
        .replace("{title}", &ctx.title)
        .replace("{meeting_id}", &ctx.meeting_id)
        .replace("{instructions}", target.instructions.trim());
    let out = out.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
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
```

Replace the `run` function (from its doc comment through its closing brace) with:

```rust
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
```

Replace the two commands at the end (`locate_claude_cli`, `select_wiki_directory`) with:

```rust
/// Where the settings card says Claude Code was found.
#[tauri::command]
pub async fn locate_claude_cli() -> Option<String> {
    locate_claude().map(|p| p.display().to_string())
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
```

- [ ] **Step 5: Update the callers**

`src/audio/post_enhance_hook.rs`:
- lines 124-127: `Wiki,` becomes `Agent,` in `pub enum HookKind`.
- line 182: `super::post_processing::run(target, ctx, prefs.post_processing_budget_usd, force, Some(then))` becomes `super::post_processing::run(prefs, target, ctx, force, Some(then))`.
- lines 183 and 187: `HookKind::Wiki` becomes `HookKind::Agent`.
- the test name `dispatch_prefers_the_wiki_row_and_marks_a_failed_start` becomes `dispatch_prefers_the_project_row_and_marks_a_failed_start` and its last assertion message `"the command does not run when a project row exists"`.

`src/audio/hook_status.rs` line 98:

```rust
        super::post_enhance_hook::HookKind::Agent => Ok("Post-processing started".to_string()),
```

`src/jobs.rs` line 19: `pub const STEP_WIKI: &str = "Wiki";` becomes `pub const STEP_AGENT: &str = "Agent";` and every `STEP_WIKI` in the file becomes `STEP_AGENT` (`rg STEP_WIKI src` must print nothing afterwards).

`src/lib.rs` lines 916-917:

```rust
            audio::post_processing::locate_claude_cli,
            audio::post_processing::select_directory,
            audio::post_processing::get_default_agent_prompt_template,
```

- [ ] **Step 6: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib post_processing 2>&1 | tail -20 && cargo test --lib post_enhance_hook 2>&1 | tail -5 && cargo test --lib jobs 2>&1 | tail -5`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add -A frontend/src-tauri/src
git commit -m "feat(post-processing): prompt template, editable tool lists, agent path; module renamed from wiki_automation"
```

---

### Task 3: The Post-processing settings card

**Files:**
- Rename: `frontend/src/components/WikiAutomationSettings.tsx` to `frontend/src/components/PostProcessingSettings.tsx`
- Modify: `frontend/src/components/RecordingSettings.tsx:10,15,34-41,65-71,333-334,370,376`

**Interfaces:**
- Consumes: commands `locate_claude_cli`, `select_directory`, `get_default_agent_prompt_template` from Task 2; preference fields from Task 1
- Produces: `export interface PostProcessingTarget { project: string; directory: string; instructions: string }` and the renamed fields on `RecordingPreferences`

- [ ] **Step 1: Rename the file**

```bash
cd /Users/lainealexandre/Code/Perso/noesis/frontend && git mv src/components/WikiAutomationSettings.tsx src/components/PostProcessingSettings.tsx
```

- [ ] **Step 2: Update the preferences interface**

In `RecordingSettings.tsx`:
- line 10: `import { PostProcessingSettings } from '@/components/PostProcessingSettings'; // generic settings section 1`
- line 15: `export interface PostProcessingTarget { project: string; directory: string; instructions: string } // generic settings section 1`
- lines 34, 39, 40: replace `wiki_automation: boolean; // spec 4.8`, `wiki_targets: WikiTarget[];`, `wiki_budget_usd: number;` with

```ts
  post_processing: boolean; // generic settings section 1
  post_processing_targets: PostProcessingTarget[];
  post_processing_budget_usd: number;
  agent_path: string;
  agent_prompt_template: string;
  agent_allowed_tools: string[];
  agent_denied_tools: string[];
```

- lines 65, 70, 71 (the initial state): replace the three wiki lines with

```ts
    post_processing: false,
    post_processing_targets: [],
    post_processing_budget_usd: 5,
    agent_path: '',
    agent_prompt_template: '',
    agent_allowed_tools: ['mcp__open-knowledge', 'mcp__noesis'],
    agent_denied_tools: ['mcp__open-knowledge__delete', 'mcp__open-knowledge__move', 'mcp__noesis__assign_meeting'],
```

- lines 333-334:

```tsx
      {/* generic settings section 1: post-processing */}
      <PostProcessingSettings preferences={preferences} onChange={applyPreferences} disabled={saving} />
```

- line 370 hint: replace `when wiki automation is off or the project has no wiki` with `when post-processing is off or the project has no row`.
- line 376 placeholder: `~/Code/Perso/noesis/scripts/post-enhance.sh` becomes `/path/to/post-enhance.sh`.

- [ ] **Step 3: Write the card**

Replace the whole content of `src/components/PostProcessingSettings.tsx` with:

```tsx
'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Switch } from '@/components/ui/switch';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { ProjectSelect } from '@/components/ProjectSelect';
import { SettingsGroup, SettingRow } from '@/components/SettingsGroup';
import type { RecordingPreferences, PostProcessingTarget } from '@/components/RecordingSettings';

interface Props {
  preferences: RecordingPreferences;
  onChange: (next: RecordingPreferences) => Promise<void> | void;
  disabled?: boolean;
}

const splitTools = (text: string) => text.split(/[\s,]+/).map((s) => s.trim()).filter(Boolean);

/** Generic settings section 1: one directory per project; the app runs Claude Code there after each enhance. */
export function PostProcessingSettings({ preferences, onChange, disabled }: Props) {
  const [claudePath, setClaudePath] = useState<string | null | undefined>(undefined);
  const [defaultTemplate, setDefaultTemplate] = useState('');
  const [draft, setDraft] = useState<PostProcessingTarget[] | null>(null);
  const [budgetDraft, setBudgetDraft] = useState<string | null>(null);
  const [agentDraft, setAgentDraft] = useState<string | null>(null);
  const [templateDraft, setTemplateDraft] = useState<string | null>(null);
  const [allowedDraft, setAllowedDraft] = useState<string | null>(null);
  const [deniedDraft, setDeniedDraft] = useState<string | null>(null);

  useEffect(() => {
    invoke<string | null>('locate_claude_cli').then(setClaudePath).catch(() => setClaudePath(null));
    invoke<string>('get_default_agent_prompt_template').then(setDefaultTemplate).catch(() => setDefaultTemplate(''));
  }, []);

  const rows = draft ?? preferences.post_processing_targets;
  const commitRows = async (next: PostProcessingTarget[]) => {
    setDraft(null);
    await onChange({ ...preferences, post_processing_targets: next });
  };
  const updateRow = (i: number, patch: Partial<PostProcessingTarget>) => {
    const next = rows.map((r, j) => (j === i ? { ...r, ...patch } : r));
    setDraft(next);
    return next;
  };
  const browse = async (i: number) => {
    const picked = await invoke<string | null>('select_directory').catch(() => null);
    if (picked) await commitRows(updateRow(i, { directory: picked }));
  };
  const commitBudget = async () => {
    const value = parseFloat(budgetDraft ?? String(preferences.post_processing_budget_usd));
    setBudgetDraft(null);
    if (!Number.isFinite(value) || value <= 0 || value === preferences.post_processing_budget_usd) return;
    await onChange({ ...preferences, post_processing_budget_usd: value });
  };
  const commitText = async (key: 'agent_path' | 'agent_prompt_template', value: string, reset: () => void) => {
    reset();
    if (value.trim() === preferences[key]) return;
    await onChange({ ...preferences, [key]: value.trim() });
  };
  const commitTools = async (key: 'agent_allowed_tools' | 'agent_denied_tools', text: string, reset: () => void) => {
    reset();
    const list = splitTools(text);
    if (list.join(' ') === preferences[key].join(' ')) return;
    await onChange({ ...preferences, [key]: list });
  };
  const blurOnEnter = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') e.currentTarget.blur();
  };

  return (
    <SettingsGroup title="Post-processing">
      <SettingRow
        label="Run Claude Code after enhance"
        hint={
          <>
            After each enhance, run Claude Code in the project&apos;s directory with the prompt below. Output goes to hook.log in the meeting folder.
            <span className="block mt-1 font-mono">
              {claudePath === undefined
                ? 'Looking for Claude Code...'
                : claudePath
                  ? `Claude Code: ${claudePath}`
                  : 'Claude Code not found. Install it and sign in, then reopen settings.'}
            </span>
          </>
        }
        control={
          <Switch
            checked={preferences.post_processing}
            onCheckedChange={(on) => onChange({ ...preferences, post_processing: on })}
            disabled={disabled}
            aria-label="Post-processing"
          />
        }
      />

      <SettingRow label="Claude Code path" hint="Leave empty to use the detected one">
        <Input
          aria-label="Claude Code path"
          className="h-8 font-mono text-xs"
          placeholder={claudePath ?? '/path/to/claude'}
          value={agentDraft ?? preferences.agent_path}
          onChange={(e) => setAgentDraft(e.target.value)}
          onBlur={() => commitText('agent_path', agentDraft ?? preferences.agent_path, () => setAgentDraft(null))}
          onKeyDown={blurOnEnter}
          disabled={disabled}
        />
      </SettingRow>

      <SettingRow label="Projects" hint="One directory per project, with optional instructions added to the prompt">
        <div className="space-y-2">
          {rows.map((row, i) => (
            <div key={i} className="grid grid-cols-[9rem_1fr_12rem_auto] gap-2 items-center">
              <ProjectSelect compact value={row.project} onChange={(p) => commitRows(updateRow(i, { project: p }))} disabled={disabled} />
              <div className="flex gap-1">
                <Input
                  aria-label="Directory"
                  className="h-8 font-mono text-xs"
                  placeholder="/path/to/project-notes"
                  value={row.directory}
                  onChange={(e) => updateRow(i, { directory: e.target.value })}
                  onBlur={() => draft && commitRows(draft)}
                  disabled={disabled}
                />
                <Button type="button" variant="outline" size="sm" onClick={() => browse(i)} disabled={disabled}>
                  Browse
                </Button>
              </div>
              <Input
                aria-label="Instructions (optional)"
                className="h-8 text-xs"
                placeholder="Optional, e.g. Use the dex-case-tracking skill."
                value={row.instructions}
                onChange={(e) => updateRow(i, { instructions: e.target.value })}
                onBlur={() => draft && commitRows(draft)}
                disabled={disabled}
              />
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => commitRows(rows.filter((_, j) => j !== i))}
                disabled={disabled}
                aria-label="Remove row"
              >
                Remove
              </Button>
            </div>
          ))}
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => commitRows([...rows, { project: '', directory: '', instructions: '' }])}
            disabled={disabled}
          >
            Add project
          </Button>
        </div>
      </SettingRow>

      <SettingRow
        label="Prompt template"
        hint="Placeholders: {folder} {project} {title} {meeting_id} {instructions}. Empty means the built-in template."
        control={
          <Button
            size="sm"
            variant="ghost"
            disabled={disabled || !preferences.agent_prompt_template}
            onClick={() => { setTemplateDraft(null); onChange({ ...preferences, agent_prompt_template: '' }); }}
          >
            Reset
          </Button>
        }
      >
        <Textarea
          aria-label="Prompt template"
          rows={4}
          className="font-mono text-xs"
          value={templateDraft ?? (preferences.agent_prompt_template || defaultTemplate)}
          onChange={(e) => setTemplateDraft(e.target.value)}
          onBlur={() => {
            const value = templateDraft ?? preferences.agent_prompt_template;
            // the built-in text typed back verbatim stays "empty"
            commitText('agent_prompt_template', value.trim() === defaultTemplate.trim() ? '' : value, () => setTemplateDraft(null));
          }}
          disabled={disabled}
        />
      </SettingRow>

      <SettingRow label="Allowed tools" hint="Space or comma separated. Empty means file edits only.">
        <Input
          aria-label="Allowed tools"
          className="h-8 font-mono text-xs"
          value={allowedDraft ?? preferences.agent_allowed_tools.join(' ')}
          onChange={(e) => setAllowedDraft(e.target.value)}
          onBlur={() => commitTools('agent_allowed_tools', allowedDraft ?? preferences.agent_allowed_tools.join(' '), () => setAllowedDraft(null))}
          onKeyDown={blurOnEnter}
          disabled={disabled}
        />
      </SettingRow>

      <SettingRow label="Denied tools" hint="Space or comma separated">
        <Input
          aria-label="Denied tools"
          className="h-8 font-mono text-xs"
          value={deniedDraft ?? preferences.agent_denied_tools.join(' ')}
          onChange={(e) => setDeniedDraft(e.target.value)}
          onBlur={() => commitTools('agent_denied_tools', deniedDraft ?? preferences.agent_denied_tools.join(' '), () => setDeniedDraft(null))}
          onKeyDown={blurOnEnter}
          disabled={disabled}
        />
      </SettingRow>

      <SettingRow
        label="Cost cap per run"
        hint="In US dollars, per Claude Code run"
        control={
          <Input
            aria-label="Cost cap per run in USD"
            className="h-8 w-24 text-[13px]"
            type="number"
            min={0.5}
            step={0.5}
            value={budgetDraft ?? String(preferences.post_processing_budget_usd)}
            onChange={(e) => setBudgetDraft(e.target.value)}
            onBlur={commitBudget}
            onKeyDown={blurOnEnter}
            disabled={disabled}
          />
        }
      />
    </SettingsGroup>
  );
}
```

- [ ] **Step 4: Type check**

Run: `cd frontend && ./node_modules/.bin/tsc --noEmit -p tsconfig.json`
Expected: exit 0. `rg -i "wiki" frontend/src` prints nothing.

- [ ] **Step 5: Commit**

```bash
git add -A frontend/src
git commit -m "feat(settings): Post-processing card with prompt template, tool lists, agent path and instructions"
```

---

### Task 4: Defaults derived from the Mac or made safe

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs` (default fns at lines 89-115, struct attributes for `auto_start_on_call` and `delete_audio_after_enhance`, tests)
- Modify: `frontend/src-tauri/Cargo.toml:177` (objc2-foundation features)

**Interfaces:**
- Produces: `pub const KNOWN_LANGUAGES: [&str; 7]`, `pub fn languages_from_locales(locales: &[String]) -> Vec<String>`

- [ ] **Step 1: Write the failing tests**

Replace `old_preferences_get_transcription_languages_and_builtin_prompt` (lines 493-500) and `call_detection_defaults_stop_on_and_start_on` (lines 526-536) with:

```rust
    #[test]
    fn languages_come_from_the_locales_the_app_knows() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(languages_from_locales(&l(&["fr-FR", "en-US", "zz", "FR_CA"])), vec!["fr", "en"]);
        assert_eq!(languages_from_locales(&l(&["ja-JP"])), vec!["en"], "nothing known falls back to English");
        assert_eq!(languages_from_locales(&[]), vec!["en"]);
        assert!(!RecordingPreferences::default().transcription_languages.is_empty());
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert_eq!(p.title_tags_prompt, "");
    }

    #[test]
    fn call_detection_defaults_stop_on_and_start_off() {
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert!(p.auto_stop_on_call_end);
        assert!(!p.auto_start_on_call, "a stranger opts in to auto-start");
        let d = RecordingPreferences::default();
        assert!(d.auto_stop_on_call_end);
        assert!(!d.auto_start_on_call);
        assert!(!p.calendar_meeting_name && !d.calendar_meeting_name);
        let stored = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4","auto_start_on_call":true}"#;
        assert!(serde_json::from_str::<RecordingPreferences>(stored).unwrap().auto_start_on_call, "a stored value is kept");
    }

    #[test]
    fn audio_deletion_defaults_off_and_owner_name_is_never_blank() {
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert!(!p.delete_audio_after_enhance);
        assert!(!RecordingPreferences::default().delete_audio_after_enhance);
        assert!(!RecordingPreferences::default().owner_name.trim().is_empty());
        assert!(!p.owner_name.trim().is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib recording_preferences::tests 2>&1 | tail -20`
Expected: compile error on `languages_from_locales`.

- [ ] **Step 3: Implement**

Add the `NSLocale` and `NSPathUtilities` features in `Cargo.toml` line 177:

```toml
objc2-foundation = { version = "0.3", features = ["NSDate", "NSString", "NSURL", "NSError", "NSArray", "NSPredicate", "NSLocale", "NSPathUtilities"] }
```

Replace `default_transcription_languages` (lines 89-91) and `default_owner_name` (lines 105-107) with:

```rust
/// The languages the Models tab offers; keep in sync with `LANGUAGES` in `components/ModelsSettings.tsx`.
pub const KNOWN_LANGUAGES: [&str; 7] = ["en", "fr", "de", "es", "it", "nl", "pt"];

/// ISO 639-1 codes of the given locales (`fr-FR`, `en_US`), deduplicated, known ones only; English when nothing is known.
pub fn languages_from_locales(locales: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in locales {
        let code = l.split(['-', '_']).next().unwrap_or("").to_lowercase();
        if KNOWN_LANGUAGES.contains(&code.as_str()) && !out.contains(&code) {
            out.push(code);
        }
    }
    if out.is_empty() {
        out.push("en".to_string());
    }
    out
}

#[cfg(target_os = "macos")]
fn preferred_locales() -> Vec<String> {
    // SAFETY: a class method with no arguments that returns an autoreleased array; nothing to uphold.
    unsafe { objc2_foundation::NSLocale::preferredLanguages() }.iter().map(|s| s.to_string()).collect()
}

#[cfg(not(target_os = "macos"))]
fn preferred_locales() -> Vec<String> {
    Vec::new()
}

fn default_transcription_languages() -> Vec<String> {
    languages_from_locales(&preferred_locales())
}

/// The macOS account's full name; "Me" when it is blank or on other platforms.
fn default_owner_name() -> String {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: plain Foundation getter with no arguments.
        let name = unsafe { objc2_foundation::NSFullUserName() }.to_string();
        if !name.trim().is_empty() {
            return name.trim().to_string();
        }
    }
    "Me".to_string()
}
```

If the compiler warns `unused_unsafe` on `preferredLanguages` or `NSFullUserName`, drop the `unsafe { }` wrapper around that call and its SAFETY comment.

Change the two attributes on the struct:

```rust
    /// Start a recording when a call begins. Off by default: recording consent rules differ by country.
    #[serde(default)]
    pub auto_start_on_call: bool,
```

```rust
    // phase 9 section 3: delete audio once the enhanced transcript is on disk. Off by default: destructive.
    #[serde(default)]
    pub delete_audio_after_enhance: bool,
```

and in `Default::default()`: `auto_start_on_call: false,` and `delete_audio_after_enhance: false,`.

- [ ] **Step 4: Match the frontend initial state**

In `frontend/src/components/RecordingSettings.tsx` initial state: `auto_start_on_call: false,`, `delete_audio_after_enhance: false,`, `transcription_languages: ['en'],`. These are placeholders until the store loads, so no other change.

- [ ] **Step 5: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib recording_preferences::tests 2>&1 | tail -20`
Expected: all pass. Then `cargo test --lib 2>&1 | grep -E "^test result|FAILED|panicked" | head` shows only the known `test_builtin_mic_detection` failure.

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/Cargo.toml Cargo.lock frontend/src-tauri/src/audio/recording_preferences.rs frontend/src/components/RecordingSettings.tsx
git commit -m "feat(prefs): languages from the system locale, owner name from the account, auto-start and audio deletion default off"
```

(`Cargo.lock` is at the repository root: the root `Cargo.toml` is a workspace.)

---

### Task 5: The recordings root preference becomes real

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs` (new fn next to `get_default_recordings_folder`, tests)
- Modify: `frontend/src-tauri/src/audio/recording_saver.rs:70-102,143,320-322`
- Modify: `frontend/src-tauri/src/audio/recording_manager.rs:520-523`
- Modify: `frontend/src-tauri/src/audio/recording_commands.rs:401,604`
- Modify: `frontend/src-tauri/src/lib.rs:621,629`, `frontend/src-tauri/src/audio/library.rs:456-471`, `frontend/src-tauri/src/audio/retranscription.rs:1146`, `frontend/src-tauri/src/audio/meeting_tagging.rs:177`, `frontend/src-tauri/src/api/api.rs:351,361,803,970,1058`, `frontend/src-tauri/src/summary/service.rs:551`
- Modify: `frontend/src/components/RecordingSettings.tsx:265-278`

**Interfaces:**
- Produces: `pub fn recordings_root<R: Runtime>(app: &AppHandle<R>) -> PathBuf`
- Produces: `pub fn root_from_preferences_value(value: Option<&serde_json::Value>) -> Option<PathBuf>`
- Produces: `RecordingSaver::set_root(&mut self, root: PathBuf)`, `RecordingManager::set_root(&mut self, root: PathBuf)`
- Consumes: `select_directory` command from Task 2

- [ ] **Step 1: Write the failing test**

Add to the tests in `recording_preferences.rs`:

```rust
    #[test]
    fn root_from_preferences_falls_back_when_missing_or_empty() {
        let v = serde_json::json!({"save_folder": "/Volumes/Work/recordings", "auto_save": true});
        assert_eq!(root_from_preferences_value(Some(&v)), Some(PathBuf::from("/Volumes/Work/recordings")));
        assert_eq!(root_from_preferences_value(Some(&serde_json::json!({"save_folder": "  "}))), None);
        assert_eq!(root_from_preferences_value(Some(&serde_json::json!({"auto_save": true}))), None);
        assert_eq!(root_from_preferences_value(None), None);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend/src-tauri && cargo test --lib root_from_preferences 2>&1 | tail -5`
Expected: compile error, `root_from_preferences_value` not found.

- [ ] **Step 3: Implement the root reader**

After `get_default_recordings_folder` in `recording_preferences.rs` add:

```rust
/// `save_folder` from a stored preferences object; None when absent or blank.
pub fn root_from_preferences_value(value: Option<&serde_json::Value>) -> Option<PathBuf> {
    value?
        .get("save_folder")?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Generic settings section 4: the recordings root the app records into, lists and reconciles.
/// The `save_folder` preference when set, else the platform default. Reads the store directly
/// so callers without async context can use it.
pub fn recordings_root<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.store("recording_preferences.json")
        .ok()
        .and_then(|store| root_from_preferences_value(store.get("preferences").as_ref()))
        .unwrap_or_else(get_default_recordings_folder)
}
```

- [ ] **Step 4: Give the saver a root**

`recording_saver.rs`: add a field after `project: Option<String>,` (line 77):

```rust
    /// Generic settings section 4: set from the preference at record start; None means the default folder.
    root: Option<PathBuf>,
```

initialise it in `new()` after `project: None,` with `root: None,`, add next to `set_project` (line 143):

```rust
    /// Generic settings section 4: where the meeting folder is created.
    pub fn set_root(&mut self, root: PathBuf) {
        self.root = Some(root);
    }
```

and replace line 322:

```rust
        let base_folder = self.root.clone().unwrap_or_else(super::recording_preferences::get_default_recordings_folder);
```

`recording_manager.rs` after `set_project` (line 523):

```rust
    pub fn set_root(&mut self, root: std::path::PathBuf) {
        self.recording_saver.set_root(root);
    }
```

`recording_commands.rs`: after each `manager.set_project(project.clone());` (lines 401 and 604) add:

```rust
    manager.set_root(crate::audio::recording_preferences::recordings_root(&app));
```

- [ ] **Step 5: Replace the remaining call sites**

Each `get_default_recordings_folder()` below becomes `recordings_root(<handle>)`, fully qualified as `crate::audio::recording_preferences::recordings_root(...)`:

| File:line | Handle expression |
|---|---|
| `lib.rs:621` and `lib.rs:629` | `&_app.handle()` |
| `library.rs:458` (`list_projects_command`) | add parameters: `pub async fn list_projects_command<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<Vec<String>, String>` and use `&app` |
| `library.rs:468` (`assign_meeting`) | add `app: tauri::AppHandle<R>` as the first parameter and `<R: tauri::Runtime>` on the fn; use `&app` |
| `retranscription.rs:1146` | `&app` |
| `meeting_tagging.rs:177` | `app` (`run_after_enhance` receives `app: &AppHandle<R>`) |
| `api.rs:351`, `api.rs:361` | `&_app` |
| `api.rs:803` (`api_delete_meeting`) | `&_app` |
| `api.rs:970` (`api_save_meeting_title`) | `&_app` |
| `api.rs:1058` (`api_save_transcript`) | `&app` |
| `summary/service.rs:551` | `&_app` |

`migration.rs:118` and `recording_preferences.rs:294` (`get_default_recordings_folder_path`) keep the default on purpose.

Run `rg -n "get_default_recordings_folder\(\)" frontend/src-tauri/src --glob '!*old*' --glob '!audio_v2/*'`: only `migration.rs`, `recording_preferences.rs` and tests may remain.

- [ ] **Step 6: A Change button for the folder**

In `RecordingSettings.tsx` the "Recordings folder" row (lines 265-278): change the hint to

```tsx
            hint={
              <span className="font-mono break-all">
                {preferences.save_folder || 'Default folder'} · {preferences.file_format.toUpperCase()}
                <span className="block font-sans mt-1">New recordings go here. Existing recordings stay where they are.</span>
              </span>
            }
```

and the control to

```tsx
            control={
              <div className="flex gap-1">
                <Button variant="outline" size="sm" onClick={handleChangeFolder} disabled={saving || isRecording}>
                  Change
                </Button>
                <Button variant="outline" size="sm" onClick={handleOpenFolder}>
                  <FolderOpen />
                  Open folder
                </Button>
              </div>
            }
```

Add next to `applyPreferences` (line 125):

```ts
  // generic settings section 4: the folder new recordings go into
  const handleChangeFolder = async () => {
    const picked = await invoke<string | null>('select_directory').catch(() => null);
    if (picked && picked !== preferences.save_folder) await applyPreferences({ ...preferences, save_folder: picked });
  };
```

- [ ] **Step 7: Run everything**

Run: `cd frontend/src-tauri && cargo test --lib 2>&1 | grep -E "^test result|FAILED|panicked" | head`
Expected: only the known `test_builtin_mic_detection` failure.
Run: `cd frontend && ./node_modules/.bin/tsc --noEmit -p tsconfig.json`
Expected: exit 0.

- [ ] **Step 8: Commit**

```bash
git add -A frontend/src frontend/src-tauri/src
git commit -m "feat(recordings): save_folder drives recording, listing and reconcile; Change button in Settings"
```

---

### Task 6: MCP root follows the preference

**Files:**
- Modify: `mcp/src/paths.ts:10-17`
- Modify: `mcp/test/paths.test.ts:50-60`

**Interfaces:**
- Produces: `export function prefsFileFor(home: string): string`, `export function rootFromPrefsFile(file: string): string | null`, `recordingsRoot(env?, home?, prefsFile?)`

- [ ] **Step 1: Write the failing test**

Replace the `recordingsRoot` test (lines 50-60) with:

```ts
test('recordingsRoot: env vars, then the app preference, then existing dirs, noesis by default', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'noesis-paths-'));
  const movies = (n: string) => path.join(home, 'Movies', n);
  const prefs = path.join(home, 'prefs.json');
  assert.equal(recordingsRoot({ NOESIS_RECORDINGS: '/a', MEETILY_RECORDINGS: '/b' }, home, prefs), '/a');
  assert.equal(recordingsRoot({ MEETILY_RECORDINGS: '/b' }, home, prefs), '/b');
  assert.equal(recordingsRoot({}, home, prefs), movies('noesis-recordings'));
  fs.mkdirSync(movies('meetily-recordings'), { recursive: true });
  assert.equal(recordingsRoot({}, home, prefs), movies('meetily-recordings'));
  fs.mkdirSync(movies('noesis-recordings'));
  assert.equal(recordingsRoot({}, home, prefs), movies('noesis-recordings'));
  fs.writeFileSync(prefs, JSON.stringify({ preferences: { save_folder: '/Volumes/Work/rec' } }));
  assert.equal(recordingsRoot({}, home, prefs), '/Volumes/Work/rec');
  assert.equal(recordingsRoot({ NOESIS_RECORDINGS: '/a' }, home, prefs), '/a', 'the environment still wins');
  fs.writeFileSync(prefs, JSON.stringify({ preferences: { save_folder: '  ' } }));
  assert.equal(recordingsRoot({}, home, prefs), movies('noesis-recordings'), 'blank is ignored');
  fs.writeFileSync(prefs, '{not json');
  assert.equal(recordingsRoot({}, home, prefs), movies('noesis-recordings'), 'garbage is ignored');
  assert.equal(prefsFileFor('/Users/x'), '/Users/x/Library/Application Support/com.noesis.app/recording_preferences.json');
  fs.rmSync(home, { recursive: true });
});
```

Add `prefsFileFor` to the import list at line 7.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd mcp && npm test 2>&1 | tail -15`
Expected: failure, `prefsFileFor` is not exported.

- [ ] **Step 3: Implement**

Replace lines 10-17 of `mcp/src/paths.ts` with:

```ts
/** The Tauri store the app writes its recording preferences to (macOS path; other platforms use the env vars). */
export function prefsFileFor(home: string): string {
  return path.join(home, 'Library', 'Application Support', 'com.noesis.app', 'recording_preferences.json');
}

/** `preferences.save_folder` from that store; null when the file is missing, unreadable or the value is blank. */
export function rootFromPrefsFile(file: string): string | null {
  try {
    const json = JSON.parse(fs.readFileSync(file, 'utf8'));
    const v = json?.preferences?.save_folder;
    return typeof v === 'string' && v.trim() ? v : null;
  } catch {
    return null;
  }
}

/** NOESIS_RECORDINGS, then MEETILY_RECORDINGS (legacy), then the app's save_folder preference, then ~/Movies/noesis-recordings, falling back to the legacy ~/Movies/meetily-recordings only while it exists and the new one does not. */
export function recordingsRoot(env: NodeJS.ProcessEnv = process.env, home: string = os.homedir(), prefsFile: string = prefsFileFor(home)): string {
  const fromEnv = env.NOESIS_RECORDINGS || env.MEETILY_RECORDINGS;
  if (fromEnv) return fromEnv;
  const fromPrefs = rootFromPrefsFile(prefsFile);
  if (fromPrefs) return fromPrefs;
  const next = path.join(home, 'Movies', 'noesis-recordings');
  const legacy = path.join(home, 'Movies', 'meetily-recordings');
  return !fs.existsSync(next) && fs.existsSync(legacy) ? legacy : next;
}
```

- [ ] **Step 4: Run the tests**

Run: `cd mcp && npm test 2>&1 | tail -8`
Expected: all pass.

- [ ] **Step 5: Update the README line and commit**

In `mcp/README.md`, the "Root, first match wins" sentence becomes: `Root, first match wins: `NOESIS_RECORDINGS`, `MEETILY_RECORDINGS`, the app's "Recordings folder" setting (`~/Library/Application Support/com.noesis.app/recording_preferences.json`), `~/Movies/noesis-recordings` if it exists, `~/Movies/meetily-recordings` if it exists, else `~/Movies/noesis-recordings`.`

```bash
git add mcp/src/paths.ts mcp/test/paths.test.ts mcp/README.md
git commit -m "feat(mcp): recordings root follows the app's save_folder preference"
```

---

### Task 7: Example script and docs

**Files:**
- Modify: `scripts/post-enhance.sh:25-30`
- Modify: `CLAUDE.md:15`
- Modify: `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md:180`

- [ ] **Step 1: Strip the owner's rows from the script**

Replace the `case` block (lines 25-30) with:

```sh
# Project to directory and optional instructions. One line per project; these two are examples.
case "$NOESIS_PROJECT" in
  example)   WIKI="$HOME/path/to/example-notes";   SKILL="process-meeting" ;;
  another)   WIKI="$HOME/path/to/another-notes";   SKILL="" ;;
  *)         echo "no directory configured for project $NOESIS_PROJECT"; exit 2 ;;
esac
```

and the comment at line 2 to: `# spec 4.7: reference post-enhance command for Noesis (advanced). Most users should use the "Post-processing" card in Settings instead; this script does the same by hand. To use it, set "Custom command" to this file's path.`

Run: `sh -n scripts/post-enhance.sh` (syntax check). Expected: no output.

- [ ] **Step 2: Docs**

`CLAUDE.md` line 15 becomes:

```
- Post-processing (spec 4.7 and 4.8, generalised by `docs/superpowers/specs/2026-10-28-generic-settings-for-distribution-design.md`): with "Post-processing" on, the app runs Claude Code in the directory configured for the meeting's project after a successful enhance, with a user-editable prompt template, allowed and denied tool lists and a cost cap, logging to `hook.log` and marking `hook.done`. Projects without a row fall through to the `post_enhance_command` preference, run through `sh -c` with `NOESIS_FOLDER`, `NOESIS_PROJECT`, `NOESIS_TITLE` and `NOESIS_MEETING_ID` (`MEETILY_*` equivalents still set for compatibility). `scripts/post-enhance.sh` is the advanced example of such a command. The recordings root is the `save_folder` preference, read through `recordings_root(app)`.
```

In the main spec, after the heading `### 4.8 Wiki automation settings` (line 180) insert a line:

```
Superseded for the settings shape by [generic settings for distribution](2026-10-28-generic-settings-for-distribution-design.md), section 1: the card is "Post-processing", the prompt is a template, the tool lists are editable and `skill` became per-project instructions.
```

- [ ] **Step 3: Commit**

```bash
git add scripts/post-enhance.sh CLAUDE.md docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md
git commit -m "docs: post-processing wording, generic example script"
```

---

## Manual acceptance (owner, after Task 7)

1. Launch the dev app. Settings, Recordings: the Post-processing card shows the three project rows with instructions "Use the … skill." where a skill was set, the detected Claude path, the built-in template, the two tool lists and the cost cap. Saving any field rewrites `~/Library/Application Support/com.noesis.app/recording_preferences.json` with `post_processing*` keys and no `wiki_*` keys.
2. Languages still read en and fr; auto-start is still on (stored values kept).
3. Click "Change" on the recordings folder, pick a temp folder, record a one-minute meeting: its folder lands under the temp folder, the sidebar lists it, `node mcp/src/index.ts` with no env var resolves to the temp folder (`list_projects` shows its projects). Change the folder back.
4. Run hook on a meeting: `hook.log` shows the rendered prompt with the project's instructions sentence.
