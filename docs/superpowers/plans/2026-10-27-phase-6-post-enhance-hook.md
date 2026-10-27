# Phase 6: Post-Enhance Hook Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When an enhance job finishes, the app runs one user-configured shell command with the meeting's folder, project, title and id in the environment, so a wiki workflow can start without the owner doing anything. A reference script maps the project to the right OpenKnowledge wiki and runs `claude -p` there, draft-only.

**Architecture:** One preference, `post_enhance_command` (empty means off), with a text input in Recording settings. A new small module `audio/post_enhance_hook.rs` spawns `sh -c <command>` detached from the meeting folder, with `MEETILY_FOLDER`, `MEETILY_PROJECT`, `MEETILY_TITLE`, `MEETILY_MEETING_ID` set and stdout plus stderr appended to `hook.log` in the meeting folder. The enhance queue calls it once, right after it writes the `done` status, and only when the job produced at least one segment. The wiki side is a script in `scripts/post-enhance.sh`, outside the app's logic.

**Tech Stack:** Rust (std::process, serde), TypeScript (one settings input), POSIX sh.

Spec: `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md`, section 4.7.

## Global Constraints

- Additive changes marked `// spec 4.7`. The hook never blocks or fails the enhance job: spawn errors are logged with `warn!`, the status stays `done`.
- The command runs through `sh -c`, detached (the child is spawned and not waited on; stdin is null), with the meeting folder as working directory and these environment variables set on top of the app's environment: `MEETILY_FOLDER` (absolute path), `MEETILY_PROJECT`, `MEETILY_TITLE`, `MEETILY_MEETING_ID`. Stdout and stderr are appended to `<folder>/hook.log`, preceded by one header line `=== <RFC 3339 local time> sh -c <command>`.
- The hook fires only on status `done` with at least one segment, never on `failed`, never on a cancelled job, and never for a silent recording (zero segments).
- Preference `post_enhance_command: String`, default `""`, `#[serde(default)]`. The settings input saves on blur or Enter, trimmed; whitespace-only saves as empty.
- The reference script `scripts/post-enhance.sh` is POSIX sh, executable, dispatches on `MEETILY_PROJECT` to a wiki directory and a skill, exits 0 silently for `_inbox` and unknown projects, skips when `<folder>/hook.done` exists and writes that marker after a successful run, supports `DRY_RUN=1` (print the command instead of running it), sets `PATH` to include `$HOME/.local/bin`, `/opt/homebrew/bin` and `/usr/local/bin` (the app launched from Finder has a minimal PATH), and runs `claude -p` with `--permission-mode acceptEdits --allowedTools mcp__open-knowledge mcp__meetily`. The prompt asks for a draft only: transcript doc and a draft log entry; no scoring, tracker or Blueprint writes.
- No em-dashes in any text, code comment, UI string or script.
- Rust tests: `cd frontend/src-tauri && cargo test --lib <module>`; the full suite on `main` is 288 passed, 3 ignored, 19 upstream warnings, add none. Type check: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json`. Never run pnpm.
- Branch `phase-6/post-enhance-hook`, one commit per task, no push.

## File Structure

| File | Responsibility |
| --- | --- |
| `frontend/src-tauri/src/audio/recording_preferences.rs` (modify) | `post_enhance_command` |
| `frontend/src/components/RecordingSettings.tsx` (modify) | Input for the command |
| `frontend/src-tauri/src/audio/post_enhance_hook.rs` (create) | `HookContext`, `read_context(folder, meeting_id)`, `spawn(command, ctx)` |
| `frontend/src-tauri/src/audio/mod.rs` (modify) | Declare the module |
| `frontend/src-tauri/src/audio/retranscription.rs` (modify) | Call the hook after `STATUS_DONE` |
| `scripts/post-enhance.sh` (create) | Reference wiki dispatcher |
| `CLAUDE.md` (modify) | One bullet in the fork section pointing at the hook and the script |

---

### Task 1: Preference `post_enhance_command` and its settings input

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs`
- Modify: `frontend/src/components/RecordingSettings.tsx`

**Interfaces:**
- Produces: `RecordingPreferences.post_enhance_command: String` (default empty); TS `post_enhance_command: string`.

- [ ] **Step 1: Rust**

Add after `owner_name` in the struct:

```rust
    // spec 4.7: shell command run when an enhance job finishes; empty means off
    #[serde(default)]
    pub post_enhance_command: String,
```

and `post_enhance_command: String::new(),` in `impl Default`. Test:

```rust
    #[test]
    fn post_enhance_command_defaults_to_empty() {
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert_eq!(p.post_enhance_command, "");
    }
```

Run: `cd frontend/src-tauri && cargo test --lib recording_preferences` (all pass, 5 tests).

- [ ] **Step 2: TypeScript**

Interface: `post_enhance_command: string; // spec 4.7`. Initial state: `post_enhance_command: '',`. Handler next to `commitOwnerName`:

```ts
  // spec 4.7
  const [hookDraft, setHookDraft] = useState<string | null>(null);
  const commitHookCommand = async () => {
    const command = (hookDraft ?? preferences.post_enhance_command).trim();
    setHookDraft(null);
    if (command === preferences.post_enhance_command) return;
    const next = { ...preferences, post_enhance_command: command };
    setPreferences(next);
    await savePreferences(next);
  };
```

JSX, directly after the `{/* spec 4.1: enhance after recording */}` block (before the Zoom block):

```tsx
      {/* spec 4.7: post-enhance hook */}
      <div className="p-4 border rounded-lg">
        <div className="font-medium">Run after enhance</div>
        <div className="text-sm text-gray-600 mb-2">
          Shell command started when a transcript is enhanced. It gets MEETILY_FOLDER, MEETILY_PROJECT, MEETILY_TITLE and MEETILY_MEETING_ID; output goes to hook.log in the meeting folder. Empty means off.
        </div>
        <Input
          aria-label="Run after enhance"
          className="font-mono text-sm"
          placeholder="~/Code/Perso/ok-meetily/scripts/post-enhance.sh"
          value={hookDraft ?? preferences.post_enhance_command}
          onChange={(e) => setHookDraft(e.target.value)}
          onBlur={commitHookCommand}
          onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur(); }}
          disabled={saving}
        />
      </div>
```

Run: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json` (silent).

- [ ] **Step 3: Commit**

```bash
git add frontend/src-tauri/src/audio/recording_preferences.rs frontend/src/components/RecordingSettings.tsx
git commit -m "feat(hook): post_enhance_command preference and settings input"
```

---

### Task 2: `post_enhance_hook.rs` and the call after `done`

**Files:**
- Create: `frontend/src-tauri/src/audio/post_enhance_hook.rs`
- Modify: `frontend/src-tauri/src/audio/mod.rs` (after `pub mod speaker_energy;`)
- Modify: `frontend/src-tauri/src/audio/retranscription.rs` (the `Ok(res)` branch of the queue wrapper, right after `set_enhance_status(..., STATUS_DONE, None)`)

**Interfaces:**
- Produces: `pub struct HookContext { pub folder: PathBuf, pub project: String, pub title: String, pub meeting_id: String }`; `pub fn read_context(folder: &Path, meeting_id: &str) -> HookContext` (project and title from `metadata.json`, `_inbox` and `""` when missing); `pub fn spawn(command: &str, ctx: &HookContext) -> anyhow::Result<()>`.

- [ ] **Step 1: Failing tests**

```rust
//! spec 4.7: run a user command when an enhance job finishes. The app only hands over
//! the meeting folder, project, title and id; what happens next is the user's script.

use anyhow::{Context, Result};
use log::info;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(path: &Path) -> String {
        for _ in 0..100 {
            if let Ok(s) = std::fs::read_to_string(path) {
                if s.contains("END") {
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
        let expected = format!("dex|Weekly sync|m-1|{0}|{0}", dir.path().display());
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
```

Add to `audio/mod.rs` after `pub mod speaker_energy;`:

```rust
// spec 4.7: post-enhance hook
pub mod post_enhance_hook;
```

Run: `cd frontend/src-tauri && cargo test --lib post_enhance_hook` (compile errors).

- [ ] **Step 2: Implement**

Above the tests:

```rust
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
    writeln!(log, "=== {} sh -c {}", chrono::Local::now().to_rfc3339(), command)?;
    let stderr = log.try_clone().context("clone hook.log handle")?;
    let child = Command::new("sh")
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
    // The child is intentionally not waited on; the enhance job must not depend on it.
    drop(child);
    Ok(())
}
```

(`chrono` is already a dependency; `super::library::INBOX` exists.) In `retranscription.rs`, in the `Ok(res)` branch directly after the `STATUS_DONE` call:

```rust
            // spec 4.7: hand the finished transcript to the user's hook, if configured
            if res.segments_count > 0 {
                if let Ok(prefs) = super::recording_preferences::load_recording_preferences(&app).await {
                    let command = prefs.post_enhance_command.trim();
                    if !command.is_empty() {
                        let ctx = super::post_enhance_hook::read_context(&folder, &meeting_id);
                        if let Err(e) = super::post_enhance_hook::spawn(command, &ctx) {
                            warn!("Post-enhance hook failed to start for {}: {}", meeting_id, e);
                        }
                    }
                }
            }
```

(`load_recording_preferences` takes `&AppHandle<R>` and is async; `folder` and `meeting_id` are in scope there.)

- [ ] **Step 3: Verify and commit**

Run: `cd frontend/src-tauri && cargo test --lib post_enhance_hook && cargo test --lib retranscription && cargo check 2>&1 | grep -E "^error|^warning: unused"`
Expected: 3 hook tests pass, retranscription unchanged, no errors or new warnings.

```bash
git add frontend/src-tauri/src/audio/post_enhance_hook.rs frontend/src-tauri/src/audio/mod.rs frontend/src-tauri/src/audio/retranscription.rs
git commit -m "feat(hook): run the post-enhance command detached after a successful enhance"
```

---

### Task 3: Reference script and docs

**Files:**
- Create: `scripts/post-enhance.sh` (executable)
- Modify: `CLAUDE.md` (fork section, one bullet)

- [ ] **Step 1: The script**

```sh
#!/bin/sh
# spec 4.7: reference post-enhance hook for Meetily. Set the "Run after enhance" preference to this file's path.
# The app runs it with MEETILY_FOLDER, MEETILY_PROJECT, MEETILY_TITLE and MEETILY_MEETING_ID set,
# from inside the meeting folder, and appends this script's output to hook.log there.
# DRY_RUN=1 prints the claude command instead of running it.
set -eu

# The app launched from Finder has a minimal PATH; claude lives in ~/.local/bin on this machine.
export PATH="$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

: "${MEETILY_FOLDER:?MEETILY_FOLDER is required}"
: "${MEETILY_PROJECT:?MEETILY_PROJECT is required}"
: "${MEETILY_MEETING_ID:?MEETILY_MEETING_ID is required}"
MEETILY_TITLE="${MEETILY_TITLE:-}"

# Project to wiki directory and skill. Add a line per project.
case "$MEETILY_PROJECT" in
  dex)        WIKI="$HOME/Code/Case/DEX/dex-case-tracking";     SKILL="dex-case-tracking" ;;
  recruiting) WIKI="$HOME/Code/Knowledge/recruiting";           SKILL="process-interview" ;;
  flo)        WIKI="$HOME/Code/Case/FLO/revenue-ai-knowledge";  SKILL="" ;;
  *)          exit 0 ;;   # _inbox and unknown projects: nothing to do
esac

MARKER="$MEETILY_FOLDER/hook.done"
if [ -e "$MARKER" ]; then
  echo "already processed ($MARKER exists), skipping"
  exit 0
fi
if [ ! -d "$WIKI" ]; then
  echo "wiki directory $WIKI not found" >&2
  exit 1
fi

if [ -n "$SKILL" ]; then
  PROMPT="Use the $SKILL skill to process the new meeting recording at $MEETILY_FOLDER (project $MEETILY_PROJECT, meeting id $MEETILY_MEETING_ID, title: $MEETILY_TITLE). Draft only: write the transcript document and a draft log entry; do not score, do not update trackers or the Blueprint. Leave everything uncommitted for review."
else
  PROMPT="Process the new meeting recording at $MEETILY_FOLDER (project $MEETILY_PROJECT, meeting id $MEETILY_MEETING_ID, title: $MEETILY_TITLE). Draft only: write the transcript document and a draft log entry; leave everything uncommitted for review."
fi

cd "$WIKI"
if [ "${DRY_RUN:-0}" = "1" ]; then
  echo "DRY_RUN in $WIKI: claude -p \"$PROMPT\" --permission-mode acceptEdits --allowedTools mcp__open-knowledge mcp__meetily"
  exit 0
fi

echo "processing $MEETILY_MEETING_ID in $WIKI"
claude -p "$PROMPT" --permission-mode acceptEdits --allowedTools mcp__open-knowledge mcp__meetily
date > "$MARKER"
echo "done"
```

`chmod +x scripts/post-enhance.sh`. Verify:

```bash
d=$(mktemp -d); DRY_RUN=1 MEETILY_FOLDER="$d" MEETILY_PROJECT=dex MEETILY_TITLE="Weekly sync" MEETILY_MEETING_ID=m-1 sh scripts/post-enhance.sh
# prints one DRY_RUN line naming the DEX wiki and the dex-case-tracking skill
MEETILY_FOLDER="$d" MEETILY_PROJECT=_inbox MEETILY_MEETING_ID=m-1 sh scripts/post-enhance.sh; echo "exit $?"
# prints nothing, exit 0
touch "$d/hook.done"; DRY_RUN=1 MEETILY_FOLDER="$d" MEETILY_PROJECT=dex MEETILY_MEETING_ID=m-1 sh scripts/post-enhance.sh
# prints "already processed"
```

- [ ] **Step 2: CLAUDE.md**

Add a bullet at the end of the "This fork" list:

```markdown
- Post-enhance hook (spec 4.7): the `post_enhance_command` preference runs after a successful enhance with `MEETILY_FOLDER`, `MEETILY_PROJECT`, `MEETILY_TITLE` and `MEETILY_MEETING_ID` set; output lands in `hook.log` in the meeting folder. `scripts/post-enhance.sh` is the reference dispatcher to the wikis.
```

- [ ] **Step 3: Commit**

```bash
git add scripts/post-enhance.sh CLAUDE.md
git commit -m "feat(hook): reference post-enhance script dispatching to the wikis"
```

---

### Task 4: Manual acceptance (owner)

- [ ] **Step 1:** In Settings, Recording, set "Run after enhance" to `/Users/lainealexandre/Code/Perso/ok-meetily/scripts/post-enhance.sh`.
- [ ] **Step 2:** Dry run first: edit the preference to `DRY_RUN=1 /Users/lainealexandre/Code/Perso/ok-meetily/scripts/post-enhance.sh`, record 20 seconds in project `dex`, stop, wait for the enhance. Expected: `hook.log` in the meeting folder holds a header line and one `DRY_RUN in .../dex-case-tracking: claude -p ...` line; no `hook.done`.
- [ ] **Step 3:** Same in `_inbox`: `hook.log` holds only the header line.
- [ ] **Step 4:** Real run: set the preference back to the plain script path, re-enhance the `dex` meeting from the app (or record a new one). Expected: `hook.log` shows "processing", then Claude's output, then "done"; `hook.done` exists; the DEX wiki has an uncommitted draft transcript doc and log entry. If `claude` complains about permissions, adjust the `--allowedTools` list in the script.
- [ ] **Step 5:** Finish with `superpowers:finishing-a-development-branch` (squash merge to `main`).

---

## Self-review against the spec

- 4.7 preference, `sh -c`, detached, four env vars, `hook.log`: Tasks 1 and 2. Reference script mapping project to wiki, `claude -p`, draft-only recommendation: Task 3. Rejected alternative (launchd watcher) stays rejected.
- Fires once per enhance with the right arguments; the script's `hook.done` marker guards re-enhance.
- Names used across tasks: `post_enhance_command`, `HookContext`, `read_context`, `spawn`, `hook.log`, `hook.done`, `DRY_RUN`.

## Outcome

Approved by the owner on 2026-10-27 and squash-merged into `main`. The hook is off by default; the real `claude -p` run is exercised by Phase 7's acceptance, which builds the structured wiki automation on top of this hook. Review fixes folded in: the hook child is reaped by a background thread; `claude` failures are reported in `hook.log` with the exit code; the unattended run denies `delete`, `move` and `assign_meeting`; a budget cap; the spec's unused `hook` metadata key was dropped.

Deferred follow-ups (none blocks daily use):
- Settings description says "when a transcript is enhanced"; the spec wording is "when an enhance job finishes". No `aria-describedby` link between the description and the input (same for the owner-name input).
- The hook shares the app's process group, so Ctrl-C under `tauri dev` kills a running hook; `process_group(0)` would detach it. Quitting the app from Finder leaves it running, which is right.
- The hook tests do not assert the header line's content.
- `scripts/post-enhance.sh` writes `hook.done` relative to `MEETILY_FOLDER`, which the app always passes absolute.
- A headless `claude -p` is allowed the two MCP servers only; a wiki skill that reads the meeting folder directly or shells out needs extra allow rules.
