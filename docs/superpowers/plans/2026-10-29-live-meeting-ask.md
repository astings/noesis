# Live Meeting Ask Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ask a local model what has been discussed in the meeting being recorded (or in a finished meeting) from an Ask box inside Noesis, and let Claude Code read live transcripts through the MCP server.

**Architecture:** One Rust Tauri command `ask_meeting` in a new module `audio/ask.rs` turns transcript segments (live from the recording manager, finished from SQLite) into `[mm:ss] Name: text` lines, trims them to a token budget and sends them with the question to the configured LLM through the existing `summary::llm_client::generate_summary` path. One React component `AskPanel` is mounted on the home page and on the meeting details page. Two new preferences pick an ask model separate from the summary model. The MCP server gains a `recording` quality value; it already lists and reads recording meetings.

**Tech Stack:** Rust (Tauri 2, tokio, sqlx, serde), TypeScript (Next.js 14, React 18, shadcn primitives, lucide-react, sonner), Node 22 test runner for the MCP package.

Spec: `docs/superpowers/specs/2026-10-29-live-meeting-ask-design.md`. Read it first.

## Global Constraints

- Branch `enhance/live-meeting-ask` off `main`. One commit per task, messages in the repo's `feat:`/`fix:`/`test:`/`docs:` style.
- Changes are additive: new modules where possible, small marked edits in existing files (CLAUDE.md upstream policy). Never build on `audio_v2/`, `lib_old_complex.rs`, `audio/stt.rs` or `whisper_engine/parallel_processor.rs`.
- Contracts: folder layout, `metadata.json` v2 and `transcript.md` do not change. The MCP `quality` field gains the value `recording`, placed first in every list of values.
- Preferences `ask_provider` and `ask_model` default to the empty string, which means "same as the summary model". Existing `recording_preferences.json` files must load unchanged.
- Token budget 24 000 (rough count, `summary::processor::rough_token_count`), timeout 90 seconds, one ask at a time, nothing written to disk, not a job in the job queue.
- System prompt, verbatim: `You answer questions about a meeting using only the transcript below. Quote or closely paraphrase the transcript; do not invent details. If the transcript does not cover the question, say so in one sentence. Speaker names are approximate. Answer in the language of the question. Be brief: a few sentences or at most five bullet points.`
- Preset questions, verbatim: `Recap what has been discussed so far.` and `What was said in the last 10 minutes?` (the second with `last_minutes: 10`).
- UI copy, verbatim: placeholder `Ask about this meeting… ⌘K`, chips `Recap so far` and `Last 10 min`, busy label `Thinking…`, settings group `Ask during meetings`, settings row `Model`, option `Same as summary model`.
- Toolchain (from the project memory): Rust tests run with `cargo test --lib` in `frontend/src-tauri` and need `frontend/src-tauri/binaries/llama-helper-aarch64-apple-darwin` present (gitignored; copy it from the main checkout into a fresh worktree). One pre-existing failing upstream test, `audio::device_detection::tests::test_builtin_mic_detection`, is not ours. Type check with `frontend/node_modules/.bin/tsc --noEmit -p tsconfig.json` from `frontend/` (install deps with `npx pnpm@10 install --frozen-lockfile` if `node_modules` is missing). MCP tests: `npm test` in `mcp/`. Before editing Rust, check no `tauri:dev` from another session is hot-rebuilding this tree (`pgrep -fl "tauri dev"`).

## File structure

| File | Responsibility |
|---|---|
| `frontend/src-tauri/src/audio/recording_preferences.rs` (modify) | two new preference fields with serde defaults |
| `frontend/src/components/RecordingSettings.tsx` (modify) | the TypeScript `RecordingPreferences` interface and its default object |
| `frontend/src-tauri/src/audio/ask.rs` (create) | pure helpers (window, render, budget, model resolution) and the `ask_meeting` command |
| `frontend/src-tauri/src/audio/mod.rs`, `frontend/src-tauri/src/lib.rs` (modify) | module declaration and command registration |
| `frontend/src-tauri/src/api/api.rs` (modify) | `api_save_api_key` command |
| `frontend/src/components/ModelsSettings.tsx` (modify) | "Ask during meetings" settings group |
| `frontend/src/components/AskPanel.tsx` (create) | the Ask box, answer card, presets, Cmd+K |
| `frontend/src/app/page.tsx`, `frontend/src/app/meeting-details/page-content.tsx` (modify) | mount points |
| `mcp/src/library.ts`, `mcp/src/index.ts`, `mcp/test/library.test.ts`, `mcp/README.md`, `mcp/package.json` (modify) | `recording` quality, descriptions, test, docs, version 0.3.0 |
| `CLAUDE.md` (modify) | one pointer sentence |

---

### Task 1: Ask model preferences

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs` (struct around lines 14-66, `impl Default` around lines 127-152, tests module at the end)
- Modify: `frontend/src/components/RecordingSettings.tsx:15-36` (interface) and `:43-62` (default state object)

**Interfaces:**
- Produces: `RecordingPreferences.ask_provider: String` and `RecordingPreferences.ask_model: String` (Rust), `ask_provider: string; ask_model: string` (TypeScript). Empty strings mean "use the summary model".

- [ ] **Step 1: Write the failing test**

Append inside the existing `#[cfg(test)] mod tests { ... }` at the end of `recording_preferences.rs`:

```rust
    #[test]
    fn ask_model_defaults_to_the_summary_model() {
        let p = RecordingPreferences::default();
        assert_eq!(p.ask_provider, "");
        assert_eq!(p.ask_model, "");
        // a preferences file written before this feature still loads
        let old: RecordingPreferences = serde_json::from_str(
            r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#,
        )
        .unwrap();
        assert_eq!(old.ask_provider, "");
        assert_eq!(old.ask_model, "");
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run from `frontend/src-tauri`: `cargo test --lib audio::recording_preferences::tests::ask_model_defaults_to_the_summary_model`
Expected: compile error, `no field ask_provider`.

- [ ] **Step 3: Add the fields**

In the `RecordingPreferences` struct, after the `title_tags_prompt` field:

```rust
    // live meeting ask (spec 2026-10-29-live-meeting-ask-design.md, 3.4): provider and model for the
    // Ask panel. Empty means the summary model. Values follow the summary setting: "builtin-ai",
    // "ollama", "claude", "openai", "groq", "openrouter".
    #[serde(default)]
    pub ask_provider: String,
    #[serde(default)]
    pub ask_model: String,
```

In `impl Default for RecordingPreferences`, after `title_tags_prompt: String::new(),`:

```rust
            ask_provider: String::new(),
            ask_model: String::new(),
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib audio::recording_preferences`
Expected: all tests in the module PASS, including the new one.

- [ ] **Step 5: Mirror the fields in TypeScript**

In `frontend/src/components/RecordingSettings.tsx`, inside `export interface RecordingPreferences`, after `title_tags_prompt: string;`:

```ts
  // live ask spec 3.4: empty means the summary model
  ask_provider: string;
  ask_model: string;
```

In the `useState<RecordingPreferences>({ ... })` default object, after `title_tags_prompt: '',`:

```ts
    ask_provider: '',
    ask_model: '',
```

- [ ] **Step 6: Type check**

Run from `frontend/`: `node_modules/.bin/tsc --noEmit -p tsconfig.json`
Expected: no output (exit 0).

- [ ] **Step 7: Commit**

```bash
git add frontend/src-tauri/src/audio/recording_preferences.rs frontend/src/components/RecordingSettings.tsx
git commit -m "feat(prefs): ask_provider and ask_model, empty means the summary model"
```

---

### Task 2: `audio/ask.rs` pure helpers

**Files:**
- Create: `frontend/src-tauri/src/audio/ask.rs`
- Modify: `frontend/src-tauri/src/audio/mod.rs` (add `pub mod ask;` next to `pub mod meeting_tagging;`)

**Interfaces:**
- Produces (used by Task 3):
  - `pub struct AskSegment { pub start: f64, pub end: f64, pub speaker: Option<String>, pub text: String }`
  - `pub fn window(segments: &[AskSegment], last_minutes: Option<u32>) -> Vec<AskSegment>`
  - `pub fn render_lines(segments: &[AskSegment], owner_name: &str) -> Vec<(f64, String)>` returns `(start_seconds, line)`
  - `pub fn fit_budget(lines: &[(f64, String)], budget_tokens: usize) -> (String, Option<f64>, usize)` returns `(text, from_seconds, lines_kept)`
  - `pub enum ModelChoice { Summary, Explicit { provider: String, model: String } }`
  - `pub fn resolve_model(ask_provider: &str, ask_model: &str) -> ModelChoice`
  - constants `TOKEN_BUDGET: usize = 24_000`, `TIMEOUT_SECS: u64 = 90`, `SYSTEM_PROMPT: &str`
- Consumes: `crate::audio::transcript_md::format_mmss(f64) -> String`, `crate::summary::processor::rough_token_count(&str) -> usize`.

- [ ] **Step 1: Create the module with the failing tests**

Create `frontend/src-tauri/src/audio/ask.rs`:

```rust
//! Live meeting ask (spec 2026-10-29-live-meeting-ask-design.md): answer a question about the
//! recording in progress, or a finished meeting, from its transcript. Pure helpers here, the
//! Tauri command below them. Nothing is written to disk and nothing goes through the job queue.

use serde::Serialize;

pub const TOKEN_BUDGET: usize = 24_000;
pub const TIMEOUT_SECS: u64 = 90;
pub const SYSTEM_PROMPT: &str = "You answer questions about a meeting using only the transcript below. \
Quote or closely paraphrase the transcript; do not invent details. If the transcript does not cover \
the question, say so in one sentence. Speaker names are approximate. Answer in the language of the \
question. Be brief: a few sentences or at most five bullet points.";

#[derive(Debug, Clone, PartialEq)]
pub struct AskSegment {
    pub start: f64,
    pub end: f64,
    /// "me", "them", "both" or None
    pub speaker: Option<String>,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct AskAnswer {
    pub answer: String,
    pub from_seconds: f64,
    pub to_seconds: f64,
    pub segments: usize,
}

/// Which model answers: the summary model, or the explicit ask preference.
#[derive(Debug, PartialEq)]
pub enum ModelChoice {
    Summary,
    Explicit { provider: String, model: String },
}

/// Empty `ask_provider` (or empty `ask_model`) means the summary model.
pub fn resolve_model(ask_provider: &str, ask_model: &str) -> ModelChoice {
    let (p, m) = (ask_provider.trim(), ask_model.trim());
    if p.is_empty() || m.is_empty() {
        ModelChoice::Summary
    } else {
        ModelChoice::Explicit { provider: p.to_string(), model: m.to_string() }
    }
}

/// The segments that start within `last_minutes` of the newest segment's end. None keeps all.
pub fn window(segments: &[AskSegment], last_minutes: Option<u32>) -> Vec<AskSegment> {
    let Some(mins) = last_minutes else { return segments.to_vec() };
    let newest_end = segments.iter().map(|s| s.end).fold(0.0_f64, f64::max);
    let from = newest_end - 60.0 * mins as f64;
    segments.iter().filter(|s| s.start >= from).cloned().collect()
}

/// `[mm:ss] Name: text`, the transcript.md naming rule: "me" is the owner, "them" is Others,
/// "both" and unlabelled segments carry no name. Segments with no words are skipped.
pub fn render_lines(segments: &[AskSegment], owner_name: &str) -> Vec<(f64, String)> {
    segments
        .iter()
        .filter_map(|s| {
            let one_line = s.text.split_whitespace().collect::<Vec<_>>().join(" ");
            if one_line.is_empty() {
                return None;
            }
            let stamp = super::transcript_md::format_mmss(s.start);
            let line = match s.speaker.as_deref() {
                Some("me") => format!("[{}] {}: {}", stamp, owner_name, one_line),
                Some("them") => format!("[{}] Others: {}", stamp, one_line),
                _ => format!("[{}] {}", stamp, one_line),
            };
            Some((s.start, line))
        })
        .collect()
}

/// Drop the oldest lines until the rough token count fits. Returns the text, the start of the
/// first line kept (None when nothing is left) and how many lines were kept.
pub fn fit_budget(lines: &[(f64, String)], budget_tokens: usize) -> (String, Option<f64>, usize) {
    let mut skip = 0;
    loop {
        let kept = &lines[skip.min(lines.len())..];
        let text = kept.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n");
        if kept.is_empty() {
            return (text, None, 0);
        }
        if crate::summary::processor::rough_token_count(&text) <= budget_tokens {
            return (text, Some(kept[0].0), kept.len());
        }
        skip += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64, speaker: Option<&str>, text: &str) -> AskSegment {
        AskSegment { start, end, speaker: speaker.map(String::from), text: text.to_string() }
    }

    #[test]
    fn window_keeps_the_last_minutes_from_the_newest_end() {
        let s = vec![seg(0.0, 5.0, None, "a"), seg(500.0, 505.0, None, "b"), seg(700.0, 710.0, None, "c")];
        let kept = window(&s, Some(5)); // 710 - 300 = 410: b and c
        assert_eq!(kept.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), vec!["b", "c"]);
        assert_eq!(window(&s, None).len(), 3);
        assert!(window(&[], Some(10)).is_empty());
    }

    #[test]
    fn render_lines_names_me_and_them_only() {
        let s = vec![
            seg(0.0, 2.0, Some("me"), "Hello   there"),
            seg(65.0, 70.0, Some("them"), "Hi"),
            seg(80.0, 81.0, Some("both"), "Yes"),
            seg(90.0, 91.0, None, "   "),
        ];
        let lines = render_lines(&s, "Alexandre");
        assert_eq!(lines.len(), 3, "blank segment skipped");
        assert_eq!(lines[0], (0.0, "[00:00] Alexandre: Hello there".to_string()));
        assert_eq!(lines[1].1, "[01:05] Others: Hi");
        assert_eq!(lines[2].1, "[01:20] Yes");
    }

    #[test]
    fn fit_budget_drops_the_oldest_lines_first() {
        // rough_token_count is ceil(chars * 0.35); each line is 21 chars ("[00:0N] " + "x" padded to 13)
        let lines: Vec<(f64, String)> = (0..10).map(|i| (i as f64 * 10.0, format!("[00:{:02}] {:13}", i, "x"))).collect();
        let (text, from, kept) = fit_budget(&lines, 31);
        assert_eq!(kept, 4, "4 lines + 3 newlines = 87 chars = 31 tokens; 5 lines = 109 chars = 39 tokens");
        assert_eq!(from, Some(60.0));
        assert!(text.starts_with("[00:06]"));
        let (_, none, zero) = fit_budget(&[], 100);
        assert_eq!((none, zero), (None, 0));
        let (_, all, n) = fit_budget(&lines, 10_000);
        assert_eq!((all, n), (Some(0.0), 10));
    }

    #[test]
    fn resolve_model_falls_back_to_summary_when_either_half_is_empty() {
        assert_eq!(resolve_model("", ""), ModelChoice::Summary);
        assert_eq!(resolve_model("ollama", "  "), ModelChoice::Summary);
        assert_eq!(resolve_model("", "qwen3.5:4b"), ModelChoice::Summary);
        assert_eq!(
            resolve_model(" ollama ", "qwen3:8b"),
            ModelChoice::Explicit { provider: "ollama".into(), model: "qwen3:8b".into() }
        );
    }
}
```

- [ ] **Step 2: Declare the module and run the tests to verify they fail**

In `frontend/src-tauri/src/audio/mod.rs`, after `pub mod meeting_tagging;`, add:

```rust
pub mod ask; // live meeting ask (spec 2026-10-29)
```

Run from `frontend/src-tauri`: `cargo test --lib audio::ask`
Expected: the four tests PASS. If `fit_budget_drops_the_oldest_lines_first` fails, recompute the expected counts with `rough_token_count` (`ceil(chars * 0.35)`) and fix the test's budget constant, not the implementation.

- [ ] **Step 3: Commit**

```bash
git add frontend/src-tauri/src/audio/ask.rs frontend/src-tauri/src/audio/mod.rs
git commit -m "feat(ask): window, render, budget and model resolution helpers with tests"
```

---

### Task 3: `ask_meeting` command

**Files:**
- Modify: `frontend/src-tauri/src/audio/ask.rs` (append the command below the helpers)
- Modify: `frontend/src-tauri/src/lib.rs` (command list around line 920, after `audio::library::assign_meeting,`)

**Interfaces:**
- Consumes: helpers from Task 2; `crate::audio::recording_commands::get_transcript_history() -> Result<Vec<TranscriptSegment>, String>` (fields `text`, `audio_start_time: f64`, `audio_end_time: f64`, `speaker: Option<String>`); `crate::audio::recording_preferences::load_recording_preferences(&app) -> anyhow::Result<RecordingPreferences>` (fields `owner_name`, `ask_provider`, `ask_model`); `crate::summary::llm_config::{load(pool), load_for(pool, provider, model)} -> Result<LlmConfig, String>` with fields `provider, model, api_key, ollama_endpoint, custom_openai_endpoint, max_tokens, temperature, top_p`; `crate::summary::llm_client::generate_summary(client, &provider, &model, &api_key, system, user, ollama_endpoint, custom_openai_endpoint, max_tokens, temperature, top_p, app_data_dir, cancellation_token) -> Result<LlmCompletion, String>` with `completion.content: String`; `crate::state::AppState` with `state.db_manager.pool()`.
- Produces: Tauri command `ask_meeting(question: String, meeting_id: Option<String>, last_minutes: Option<u32>) -> Result<AskAnswer, String>`; frontend invokes it as `invoke('ask_meeting', { question, meetingId, lastMinutes })`.

- [ ] **Step 1: Append the command to `ask.rs`**

Add these imports at the top of the file, under `use serde::Serialize;`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};
```

Append after `fit_budget` (before `#[cfg(test)]`):

```rust
static BUSY: AtomicBool = AtomicBool::new(false);

/// Clears the busy flag when the ask finishes, however it finishes.
struct BusyGuard;
impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// Answer `question` from the live recording (`meeting_id` None) or a finished meeting.
/// `last_minutes` restricts the context to the newest minutes of audio time.
#[tauri::command]
pub async fn ask_meeting<R: Runtime>(
    app: AppHandle<R>,
    question: String,
    meeting_id: Option<String>,
    last_minutes: Option<u32>,
) -> Result<AskAnswer, String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("An answer is already being generated.".to_string());
    }
    let _guard = BusyGuard;
    ask_inner(&app, question, meeting_id, last_minutes).await
}

async fn ask_inner<R: Runtime>(
    app: &AppHandle<R>,
    question: String,
    meeting_id: Option<String>,
    last_minutes: Option<u32>,
) -> Result<AskAnswer, String> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err("Type a question first.".to_string());
    }
    let state = app.state::<crate::state::AppState>();
    let pool = state.db_manager.pool();

    let segments: Vec<AskSegment> = match meeting_id.as_deref() {
        // the live recording: the meeting has no SQLite row until stop
        None => crate::audio::recording_commands::get_transcript_history()
            .await?
            .into_iter()
            .map(|s| AskSegment { start: s.audio_start_time, end: s.audio_end_time, speaker: s.speaker, text: s.text })
            .collect(),
        Some(id) => {
            let rows: Vec<(String, Option<f64>, Option<f64>, Option<String>)> = sqlx::query_as(
                "SELECT transcript, audio_start_time, audio_end_time, speaker FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time, timestamp",
            )
            .bind(id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Database error: {}", e))?;
            rows.into_iter()
                .map(|(text, start, end, speaker)| {
                    let start = start.unwrap_or(0.0);
                    AskSegment { start, end: end.unwrap_or(start), speaker, text }
                })
                .collect()
        }
    };

    let prefs = super::recording_preferences::load_recording_preferences(app)
        .await
        .map_err(|e| format!("Failed to load preferences: {}", e))?;
    let kept = window(&segments, last_minutes);
    let lines = render_lines(&kept, &prefs.owner_name);
    let (text, from, count) = fit_budget(&lines, TOKEN_BUDGET);
    let Some(from_seconds) = from else { return Err("No transcript yet.".to_string()) };
    let to_seconds = kept.iter().map(|s| s.end).fold(0.0_f64, f64::max);

    let cfg = match resolve_model(&prefs.ask_provider, &prefs.ask_model) {
        ModelChoice::Summary => crate::summary::llm_config::load(pool).await?,
        ModelChoice::Explicit { provider, model } => crate::summary::llm_config::load_for(pool, &provider, &model).await?,
    };
    let app_data_dir = app.path().app_data_dir().ok();

    // The providers race the token (HTTP) or drop the sidecar request when it fires, so the
    // timeout is a cancel, not an abandoned future.
    let token = tokio_util::sync::CancellationToken::new();
    let t = token.clone();
    let timer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(TIMEOUT_SECS)).await;
        t.cancel();
    });
    let user_prompt = format!("{}\n\nQuestion: {}", text, question);
    let result = crate::summary::llm_client::generate_summary(
        &reqwest::Client::new(),
        &cfg.provider,
        &cfg.model,
        &cfg.api_key,
        SYSTEM_PROMPT,
        &user_prompt,
        cfg.ollama_endpoint.as_deref(),
        cfg.custom_openai_endpoint.as_deref(),
        cfg.max_tokens,
        cfg.temperature,
        cfg.top_p,
        app_data_dir.as_ref(),
        Some(&token),
    )
    .await;
    timer.abort();
    let completion = match result {
        Ok(c) => c,
        Err(_) if token.is_cancelled() => return Err("The model took too long.".to_string()),
        Err(e) => return Err(e),
    };
    Ok(AskAnswer { answer: completion.content.trim().to_string(), from_seconds, to_seconds, segments: count })
}
```

- [ ] **Step 2: Register the command**

In `frontend/src-tauri/src/lib.rs`, inside `tauri::generate_handler![ ... ]`, after the line `audio::library::assign_meeting,` add:

```rust
            audio::ask::ask_meeting, // live meeting ask (spec 2026-10-29)
```

- [ ] **Step 3: Compile and run the module tests**

Run from `frontend/src-tauri`: `cargo check` then `cargo test --lib audio::ask`
Expected: `cargo check` finishes with no errors (warnings about unused `AskAnswer` fields are not expected since the struct is serialised; if `generate_summary` is reported private, it is `pub(crate)` and reachable, so re-read the path `crate::summary::llm_client::generate_summary`). The four tests PASS.

The command has no UI yet; `window.__TAURI__` is not exposed in this app, so Task 5 is where it is exercised end to end.

- [ ] **Step 4: Commit**

```bash
git add frontend/src-tauri/src/audio/ask.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(ask): ask_meeting command over live or saved segments through the configured LLM"
```

---

### Task 4: Settings, "Ask during meetings"

**Files:**
- Modify: `frontend/src-tauri/src/api/api.rs` (add `api_save_api_key` right after `api_get_api_key`, around line 630)
- Modify: `frontend/src-tauri/src/lib.rs` (register after `api::api_get_api_key,` around line 800)
- Modify: `frontend/src/components/ModelsSettings.tsx` (state near line 172, handlers near line 207, JSX after the `Summarization` group near line 365)

**Interfaces:**
- Produces: Tauri command `api_save_api_key(provider: String, api_key: String) -> Result<(), String>`, invoked as `invoke('api_save_api_key', { provider, apiKey })`.
- Consumes: `api_get_api_key(provider) -> string` (existing, empty string when none), `get_recording_preferences` / `set_recording_preferences` through the existing `prefs` state and `applyPrefs` helper, `SUMMARY_PROVIDERS`, `KEYED_PROVIDERS`, `builtinRows` (rows with `engine === 'builtin'`, `state.kind === 'ready'` when downloaded), `SettingsGroup`, `SettingRow`, `Select*`, `Input`, `Button` already imported in the file.

- [ ] **Step 1: Add the Rust command**

In `frontend/src-tauri/src/api/api.rs`, directly after the closing brace of `api_get_api_key`:

```rust
/// live ask spec 3.4: save a provider key on its own. Until now a key could only be saved
/// together with the summary model through `api_save_model_config`.
#[tauri::command]
pub async fn api_save_api_key<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    api_key: String,
) -> Result<(), String> {
    log_info!("api_save_api_key called (native) for provider '{}'", &provider);
    SettingsRepository::save_api_key(&state.db_manager.pool(), &provider, &api_key)
        .await
        .map_err(|e| e.to_string())
}
```

In `lib.rs`, after `api::api_get_api_key,` add:

```rust
            api::api_save_api_key, // live ask spec 3.4
```

Run from `frontend/src-tauri`: `cargo check`
Expected: no errors.

- [ ] **Step 2: Add the settings state and handler**

In `ModelsSettings.tsx`, inside `ModelsSettings()`, after the line that declares `const [taggingAll, setTaggingAll] = useState(false);`:

```tsx
  // Ask during meetings (live ask spec 3.4): provider and model preferences; keys are shared per provider
  const [askDraft, setAskDraft] = useState<{ provider: string; model: string; apiKey: string } | null>(null);
  const [askHasKey, setAskHasKey] = useState(false);
  const askProvider = askDraft?.provider ?? prefs?.ask_provider ?? '';
  useEffect(() => {
    if (!KEYED_PROVIDERS.includes(askProvider)) { setAskHasKey(false); return; }
    invoke<string>('api_get_api_key', { provider: askProvider }).then((k) => setAskHasKey(!!k)).catch(() => setAskHasKey(false));
  }, [askProvider]);
```

After the `tagUntagged` handler (before `if (!prefs) return ...`):

```tsx
  const saveAsk = async () => {
    if (!prefs || !askDraft) return;
    const key = askDraft.apiKey.trim();
    try {
      if (key) await invoke('api_save_api_key', { provider: askDraft.provider, apiKey: key });
      await applyPrefs({ ...prefs, ask_provider: askDraft.provider, ask_model: askDraft.model.trim() });
      if (key) setAskHasKey(true);
      setAskDraft(null);
      toast.success('Ask model saved');
    } catch (e) {
      toast.error(String(e));
    }
  };
```

After the line `const languages = prefs.transcription_languages ?? [];` (below the early return, so `prefs` is non-null):

```tsx
  const ask = askDraft ?? { provider: prefs.ask_provider, model: prefs.ask_model, apiKey: '' };
  const askKnown = SUMMARY_PROVIDERS.find((p) => p.value === ask.provider);
  const readyBuiltin = builtinRows.filter((r) => r.state.kind === 'ready');
```

- [ ] **Step 3: Add the group**

In the returned JSX, after the closing `</SettingsGroup>` of the `Summarization` group and before the final `</div>`:

```tsx
      <SettingsGroup title="Ask during meetings">
        <SettingRow
          label="Model"
          hint="Answers questions about the transcript from the Ask box. Built-in and Ollama stay on this Mac; the others send the transcript to their API."
        >
          <div className="flex flex-wrap items-center gap-2">
            <Select value={ask.provider || 'summary'} onValueChange={(v) => setAskDraft({ provider: v === 'summary' ? '' : v, model: '', apiKey: '' })}>
              <SelectTrigger aria-label="Ask provider" className="h-8 w-52 text-[13px]"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="summary">Same as summary model</SelectItem>
                <SelectItem value="builtin-ai">Built-in</SelectItem>
                {SUMMARY_PROVIDERS.map((p) => <SelectItem key={p.value} value={p.value}>{p.label}</SelectItem>)}
              </SelectContent>
            </Select>
            {ask.provider === 'builtin-ai' && (
              <Select value={ask.model} onValueChange={(v) => setAskDraft({ ...ask, model: v })}>
                <SelectTrigger aria-label="Ask built-in model" className="h-8 w-52 text-[13px]"><SelectValue placeholder="Pick a downloaded model" /></SelectTrigger>
                <SelectContent>
                  {readyBuiltin.map((r) => <SelectItem key={r.name} value={r.name}>{r.label}</SelectItem>)}
                </SelectContent>
              </Select>
            )}
            {ask.provider && ask.provider !== 'builtin-ai' && (
              <Input aria-label="Ask model" className="h-8 w-48 text-[13px]" value={ask.model} placeholder={askKnown?.placeholder}
                onChange={(e) => setAskDraft({ ...ask, model: e.target.value })} />
            )}
            {KEYED_PROVIDERS.includes(ask.provider) && (
              <Input aria-label="Ask API key" type="password" autoComplete="off" className="h-8 w-40 text-[13px]" value={ask.apiKey}
                placeholder={askHasKey ? 'Saved' : 'Paste a key'} onChange={(e) => setAskDraft({ ...ask, apiKey: e.target.value })} />
            )}
            {askDraft && (
              <Button size="sm" variant="outline" disabled={saving || (!!ask.provider && !ask.model.trim())} onClick={saveAsk}>
                Save
              </Button>
            )}
          </div>
        </SettingRow>
      </SettingsGroup>
```

- [ ] **Step 4: Type check**

Run from `frontend/`: `node_modules/.bin/tsc --noEmit -p tsconfig.json`
Expected: exit 0. If `useEffect` is not imported, it already is (`import { useCallback, useEffect, useState } from 'react';`).

- [ ] **Step 5: Check in the app**

Run `./clean_run.sh`, open Settings, Models. Expected: a third group "Ask during meetings" with the select showing "Same as summary model". Pick "Ollama", type `qwen3:8b`, Save: toast "Ask model saved"; `~/Library/Application Support/com.noesis.app/recording_preferences.json` now has `"ask_provider": "ollama", "ask_model": "qwen3:8b"`. Pick "Same as summary model", Save: both keys are back to `""`.

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/src/api/api.rs frontend/src-tauri/src/lib.rs frontend/src/components/ModelsSettings.tsx
git commit -m "feat(settings): Ask during meetings model, separate from the summary model; api_save_api_key"
```

---

### Task 5: `AskPanel` component and mount points

**Files:**
- Create: `frontend/src/components/AskPanel.tsx`
- Modify: `frontend/src/app/page.tsx` (import near the top; JSX between `<TranscriptPanel .../>` and the recording controls block)
- Modify: `frontend/src/app/meeting-details/page-content.tsx` (import; JSX after the `<div className="flex flex-1 min-w-0 overflow-hidden">...</div>` block, inside the `motion.div`)

**Interfaces:**
- Consumes: `invoke('ask_meeting', { question, meetingId, lastMinutes })` returning `{ answer: string; from_seconds: number; to_seconds: number; segments: number }` (Task 3); `Input` (forwardRef) and `Button` (`variant: 'ghost' | 'outline'`, `size: 'sm'`) from `@/components/ui`; icons `Copy`, `Loader2`, `X` from `lucide-react`.
- Produces: `export function AskPanel({ meetingId, hasSegments, disabled }: { meetingId?: string; hasSegments: boolean; disabled?: boolean })`.

- [ ] **Step 1: Create the component**

```tsx
'use client';

// Live meeting ask (spec 2026-10-29-live-meeting-ask-design.md, 3.2): one input, two presets,
// the latest answer in a card. Mounted on the home page (live) and the meeting details page.
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Copy, Loader2, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';

interface AskAnswer { answer: string; from_seconds: number; to_seconds: number; segments: number }
interface AskResult { question: string; lastMinutes?: number; answer?: AskAnswer; error?: string }

const RECAP = 'Recap what has been discussed so far.';
const LAST_10 = 'What was said in the last 10 minutes?';

function mmss(seconds: number): string {
  const t = Math.max(0, Math.floor(seconds));
  return `${String(Math.floor(t / 60)).padStart(2, '0')}:${String(t % 60).padStart(2, '0')}`;
}

function errorHint(error: string): string {
  if (error.startsWith('No summary model') || error.includes('API key not found')) return ' Pick one in Settings, Models.';
  if (error.startsWith('The model took too long')) return ' Try Last 10 min.';
  return '';
}

export function AskPanel({ meetingId, hasSegments, disabled }: { meetingId?: string; hasSegments: boolean; disabled?: boolean }) {
  const [question, setQuestion] = useState('');
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<AskResult | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Cmd+K (Ctrl+K elsewhere) focuses the box from anywhere on the page
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        inputRef.current?.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const ask = useCallback(async (q: string, lastMinutes?: number) => {
    const text = q.trim();
    if (!text || busy) return;
    setBusy(true);
    setResult(null);
    try {
      const answer = await invoke<AskAnswer>('ask_meeting', { question: text, meetingId: meetingId ?? null, lastMinutes: lastMinutes ?? null });
      setResult({ question: text, lastMinutes, answer });
    } catch (e) {
      setResult({ question: text, lastMinutes, error: String(e) });
    } finally {
      setBusy(false);
    }
  }, [busy, meetingId]);

  const coverage = result?.answer
    ? result.lastMinutes
      ? `Last ${result.lastMinutes} min, ${mmss(result.answer.from_seconds)} to ${mmss(result.answer.to_seconds)}`
      : `Whole meeting, ${result.answer.segments} segments`
    : '';

  return (
    <div className="border-t border-border px-4 py-2.5 space-y-2">
      {result && (
        <div className="rounded-lg border border-border bg-card p-3 text-sm" role="status">
          <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
            <span className="truncate">{result.error ? result.question : coverage}</span>
            <div className="flex items-center gap-1">
              {result.answer && (
                <Button variant="ghost" size="sm" aria-label="Copy answer" onClick={() => navigator.clipboard.writeText(result.answer!.answer)}>
                  <Copy />
                </Button>
              )}
              <Button variant="ghost" size="sm" aria-label="Close answer" onClick={() => setResult(null)}>
                <X />
              </Button>
            </div>
          </div>
          <div className={`mt-1 whitespace-pre-wrap ${result.error ? 'text-red-600 dark:text-red-400' : ''}`}>
            {result.error ? result.error + errorHint(result.error) : result.answer!.answer}
          </div>
        </div>
      )}
      <form className="flex items-center gap-2" onSubmit={(e) => { e.preventDefault(); ask(question); }}>
        <Input
          ref={inputRef}
          aria-label="Ask about this meeting"
          className="h-8 text-[13px]"
          placeholder="Ask about this meeting… ⌘K"
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Escape') { setResult(null); e.currentTarget.blur(); } }}
          disabled={disabled || busy}
        />
        {busy ? (
          <span className="flex flex-none items-center gap-1 text-xs text-muted-foreground">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
            Thinking…
          </span>
        ) : (
          <>
            <Button type="button" variant="outline" size="sm" className="flex-none text-[13px]" disabled={disabled || !hasSegments}
              onClick={() => { setQuestion(RECAP); ask(RECAP); }}>
              Recap so far
            </Button>
            <Button type="button" variant="outline" size="sm" className="flex-none text-[13px]" disabled={disabled || !hasSegments}
              onClick={() => { setQuestion(LAST_10); ask(LAST_10, 10); }}>
              Last 10 min
            </Button>
          </>
        )}
      </form>
    </div>
  );
}
```

- [ ] **Step 2: Mount on the home page**

In `frontend/src/app/page.tsx`, add the import after `import { TranscriptPanel } from './_components/TranscriptPanel';`:

```tsx
import { AskPanel } from '@/components/AskPanel'; // live ask spec 3.2
```

In the JSX, directly after the `<TranscriptPanel ... />` element and before the `{/* Recording controls ... */}` comment:

```tsx
        {/* live ask spec 3.2: while recording, or while live segments are still on screen */}
        {(recordingState.isRecording || transcripts.length > 0) && (
          <AskPanel hasSegments={transcripts.length > 0} disabled={isProcessingStop || status === RecordingStatus.SAVING} />
        )}
```

- [ ] **Step 3: Mount on the meeting details page**

In `frontend/src/app/meeting-details/page-content.tsx`, add after the `MeetingDetailsSplitView` import:

```tsx
import { AskPanel } from '@/components/AskPanel'; // live ask spec 3.2
```

In the JSX, after the closing `</div>` of `<div className="flex flex-1 min-w-0 overflow-hidden">` and before `</motion.div>`:

```tsx
      <AskPanel meetingId={meeting.id} hasSegments={(totalCount ?? meetingData.transcripts.length) > 0} />
```

- [ ] **Step 4: Type check**

Run from `frontend/`: `node_modules/.bin/tsc --noEmit -p tsconfig.json`
Expected: exit 0.

- [ ] **Step 5: Check in the app**

Run `./clean_run.sh`. Home, idle: no Ask bar. Start a recording and speak for 30 seconds: the bar appears under the transcript, chips enable once the first segment lands, Cmd+K focuses the input. Click "Recap so far": "Thinking…" then a card headed "Whole meeting, N segments" within about 10 seconds. Click "Last 10 min": header "Last 10 min, 00:00 to mm:ss". Type a question, Enter: answer. Esc clears the card. Stop the recording, open a finished meeting from the sidebar: the bar sits under the transcript; a topic question answers from the saved transcript. With the model set to a keyed provider and no key saved: the card shows `API key not found for claude Pick one in Settings, Models.` in red.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/AskPanel.tsx frontend/src/app/page.tsx frontend/src/app/meeting-details/page-content.tsx
git commit -m "feat(ui): Ask panel on the home and meeting pages with Recap and Last 10 min presets"
```

---

### Task 6: MCP, live meetings carry quality `recording`

**Files:**
- Modify: `mcp/src/library.ts:8` (`Quality` type) and `qualityOf` (around line 52)
- Modify: `mcp/src/index.ts` (`list_meetings` description around line 30, `get_transcript` description around line 79)
- Modify: `mcp/test/library.test.ts` (append one test)
- Modify: `mcp/README.md` (one sentence after line 17), `mcp/package.json` and `mcp/package-lock.json` (version 0.3.0)

**Interfaces:**
- Produces: `Quality = 'recording' | 'live' | 'enhanced' | 'enhancing' | 'failed'`; `list_meetings` rows and `transcript.md` frontmatter can read `quality: recording`.

- [ ] **Step 1: Write the failing test**

Append to `mcp/test/library.test.ts`:

```ts
test('a meeting being recorded lists as quality recording and its transcript so far is readable', () => {
  const dir = path.join(root, 'dex', '2026-10-29_09-00 live');
  write(dir, {
    'metadata.json': JSON.stringify({ ...v2Meta('meeting-live', 'dex', 'Live call'), status: 'recording', created_at: '2026-10-29T09:00:00+00:00', completed_at: null, duration_seconds: null }),
    'transcripts.json': JSON.stringify(segments),
  });
  const info = findMeeting(root, { meeting_id: 'meeting-live' });
  assert.ok(info);
  assert.equal(info.quality, 'recording');
  const md = readTranscript(info, 'md');
  assert.match(md, /^quality: recording$/m);
  assert.match(md, /\[00:00\] Hello there\./);
  assert.match(md, /\[01:05\] Second line\./);
  assert.equal(scanMeetings(root)[0].meeting_id, 'meeting-live', 'the newest meeting sorts first');
});
```

- [ ] **Step 2: Run it to verify it fails**

Run from `mcp/`: `npm test`
Expected: the new test FAILS on `info.quality`: `'live' !== 'recording'`. All earlier tests pass.

- [ ] **Step 3: Implement**

In `mcp/src/library.ts`, replace the `Quality` line with:

```ts
export type Quality = 'recording' | 'live' | 'enhanced' | 'enhancing' | 'failed';
```

In `qualityOf`, add as the first statement of the function body:

```ts
  if (meta.status === 'recording') return 'recording'; // live ask spec 3.3: the recorder writes this at record start
```

In `mcp/src/index.ts`, `list_meetings` description, replace the sentence `quality is live, enhanced, enhancing or failed.` with:

```
quality is recording, live, enhanced, enhancing or failed. recording means the meeting is in progress right now; it is the newest, so it sorts first.
```

`get_transcript` description: append the sentence ` On a meeting being recorded, returns the transcript so far; call again for new lines.` after `Optional time range in seconds.`

- [ ] **Step 4: Run the tests to verify they pass**

Run from `mcp/`: `npm test`
Expected: all tests PASS. Then `npm run build` must exit 0 (the `tsc` compile that `prepack` runs).

- [ ] **Step 5: Docs and version**

In `mcp/README.md`, after the `Tools:` line (line 17), add:

```
A meeting being recorded right now is listed with quality `recording`; `get_transcript` returns its transcript so far, call again for new lines.
```

From `mcp/`: `npm version 0.3.0 --no-git-tag-version` (updates `package.json` and `package-lock.json`).

- [ ] **Step 6: Commit**

```bash
git add mcp/src/library.ts mcp/src/index.ts mcp/test/library.test.ts mcp/README.md mcp/package.json mcp/package-lock.json
git commit -m "feat(mcp): quality recording for meetings in progress; 0.3.0"
```

Owner step after merge, not for the agent: `cd mcp && npm publish --access public --otp=<code>` (2FA; pass `--otp` because the `!` prefix has no TTY for the browser flow). Running Claude Code sessions keep the old version until restarted.

---

### Task 7: Docs pointer and whole-branch verification

**Files:**
- Modify: `CLAUDE.md` (the fork paragraph "This fork: personal meeting recorder", first bullet list)

- [ ] **Step 1: Add the pointer**

In `CLAUDE.md`, after the bullet that starts `- Live code is `audio/`.`, add:

```markdown
- Live meeting ask ([spec](docs/superpowers/specs/2026-10-29-live-meeting-ask-design.md)): `audio/ask.rs` answers a question over the live or saved segments through `summary::llm_client`; `components/AskPanel.tsx` is mounted on the home and meeting pages; preferences `ask_provider` and `ask_model` (empty means the summary model). The MCP server reports `quality: recording` for a meeting in progress.
```

- [ ] **Step 2: Run everything**

From `frontend/src-tauri`: `cargo test --lib` (expect only the pre-existing `test_builtin_mic_detection` failure, if any).
From `frontend/`: `node_modules/.bin/tsc --noEmit -p tsconfig.json` and `pnpm test`.
From `mcp/`: `npm test`.
Expected: green except the known upstream test.

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: live meeting ask pointer in CLAUDE.md"
```

- [ ] **Step 4: Hand over**

Report to the owner: branch `enhance/live-meeting-ask`, commits per task, and the acceptance list from the spec, section 5, to run on a one-minute test recording:

1. "Recap so far" answers within 10 seconds while recording.
2. "Last 10 min" shows the coverage header with times.
3. Cmd+K focuses the input.
4. A finished meeting answers a topic question from its page.
5. In a Claude Code session during a recording, `list_meetings` shows the top meeting with quality `recording` and a second `get_transcript` a minute later returns more lines (needs the 0.3.0 publish, or run the checkout's `mcp/src/index.ts` directly).

Merge is the owner's call (`git merge --no-ff enhance/live-meeting-ask` on `main`, matching the repo's history).
