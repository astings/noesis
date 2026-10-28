# Phase 9: auto titles, tags, search and audio cleanup: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `docs/superpowers/specs/2026-10-28-titles-search-audio-cleanup-design.md` (read it first; it is the source of truth for copy, defaults and rules).

**Architecture:** Rust owns a block-level FTS5 index (Task 1), the post-enhance title/tags step (Task 2) and the audio cleanup loop (Task 3). The MCP server gets its own in-memory FTS5 search over the files (Task 4). The frontend adds settings, manual triggers, status tags and tag chips (Task 5).

**Tech Stack:** Rust (Tauri 2, sqlx 0.8 sqlite, serde_json, chrono, tokio), Next.js 14 / React 18 / TypeScript, Node 26 `node:sqlite` for MCP.

## Global Constraints

- Work in the worktree `/Users/lainealexandre/Code/Perso/noesis/.claude/worktrees/phase-9`, branch `phase-9/search-and-cleanup`. One commit per task (fix commits allowed).
- Git: the RTK hook rewrites `git` to `rtk git`, which the worktree guard refuses. Call `/usr/bin/git ...` from the worktree root.
- Rust gates, from `frontend/src-tauri`: `CARGO_TARGET_DIR=/Users/lainealexandre/Code/Perso/noesis/target cargo check` and `CARGO_TARGET_DIR=/Users/lainealexandre/Code/Perso/noesis/target cargo test --lib <filter>`. The shared target dir avoids a full rebuild; a lock wait is normal if the owner's dev app is building.
- Frontend gate, from `frontend`: `./node_modules/.bin/tsc --noEmit` exits 0. Do NOT run `pnpm build`, do NOT launch or kill the app.
- MCP gate, from `mcp`: `npm test`.
- Never read, move or write the owner's real data (`~/Library/Application Support/com.noesis.app`, `~/Movies/noesis-recordings`). Tests use temp dirs and in-memory pools.
- Dead code untouched (`audio_v2/`, `lib_old_complex.rs`, `audio/stt.rs`, `whisper_engine/parallel_processor.rs`). Contract changes only the additive ones of spec section 5.
- No new dependencies (Rust, frontend or MCP).
- UI uses the reskin tokens only (`text-muted-foreground`, `text-primary`, `text-destructive`, `text-success`, `bg-accent`, `border-border`), works in light and dark, lucide icons.
- Defaults: `auto_title_tags` true, `delete_audio_after_enhance` true, `audio_retention_hours` 24.
- Every post-enhance failure is logged and never changes the enhance result.

---

### Task 1: Search index (Rust)

**Files:**
- Create: `frontend/src-tauri/migrations/20261028000000_add_search_index.sql`
- Create: `frontend/src-tauri/src/audio/search_index.rs` (register in `audio/mod.rs`)
- Modify: `frontend/src-tauri/src/database/repositories/transcript.rs:89-125` (`search_transcripts`), the `TranscriptSearchResult` struct (find it with `grep -rn "struct TranscriptSearchResult" frontend/src-tauri/src`)
- Modify: `frontend/src-tauri/src/audio/library.rs` (`assign`, after the DB update), `frontend/src-tauri/src/lib.rs:603-610` (after reconcile), the meeting delete path (`database/repositories/meeting.rs:18` `delete_meeting`), the recording save path (`api_save_transcript`, find with `grep -rn "fn api_save_transcript" frontend/src-tauri/src`), `audio/retranscription.rs` (after `STATUS_DONE`)

**Interfaces:**
- Produces:
  - `search_index::Seg { start: f64, end: f64, text: String }`
  - `search_index::build_blocks(segs: &[Seg]) -> Vec<Block>` with `Block { start: f64, end: f64, text: String }`
  - `search_index::fts_query(input: &str) -> Option<String>`
  - `search_index::metadata_tags(folder: &Path) -> Vec<String>`
  - `search_index::reindex_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<(), String>`
  - `search_index::remove_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<(), String>`
  - `search_index::rebuild(pool: &SqlitePool) -> Result<usize, String>`
  - `search_index::search(pool: &SqlitePool, query: &str, limit: usize) -> Result<Vec<MeetingHit>, String>` with `MeetingHit { meeting_id: String, title: String, tags: Vec<String>, score: f64, hits: Vec<Hit> }`, `Hit { start_seconds: f64, end_seconds: f64, snippet: String }`

- [ ] **Step 1: FTS5 availability test (step zero of the spec).** Add to `search_index.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::query(SCHEMA).execute(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn fts5_is_available() {
        let p = pool().await;
        sqlx::query("INSERT INTO search_index (meeting_id, start_seconds, end_seconds, title, tags, text) VALUES ('m', 0, 1, 't', '', 'réunion budget')")
            .execute(&p).await.unwrap();
        let n: (i64,) = sqlx::query_as("SELECT count(*) FROM search_index WHERE search_index MATCH '\"reunion\"'")
            .fetch_one(&p).await.unwrap();
        assert_eq!(n.0, 1);
    }
}
```

with `pub(crate) const SCHEMA: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(meeting_id UNINDEXED, start_seconds UNINDEXED, end_seconds UNINDEXED, title, tags, text, tokenize='unicode61 remove_diacritics 2')";`. The migration file contains the same statement followed by `;`.

Run: `cargo test --lib search_index`. If it fails with `no such module: fts5`, STOP and report BLOCKED with the error: the spec's fallback (rank in Rust) needs an owner decision on scope.

- [ ] **Step 2: Failing tests for the pure parts.**

```rust
    fn seg(start: f64, text: &str) -> Seg { Seg { start, end: start + 1.0, text: text.to_string() } }

    #[test]
    fn blocks_group_eight_segments_with_bounds() {
        let segs: Vec<Seg> = (0..10).map(|i| seg(i as f64 * 10.0, &format!("s{}", i))).collect();
        let b = build_blocks(&segs);
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].start, b[0].end), (0.0, 71.0));
        assert_eq!(b[0].text, "s0\ns1\ns2\ns3\ns4\ns5\ns6\ns7");
        assert_eq!((b[1].start, b[1].end), (80.0, 91.0));
        assert!(build_blocks(&[]).is_empty());
    }

    #[test]
    fn query_quotes_terms_and_prefixes_the_last() {
        assert_eq!(fts_query("  "), None);
        assert_eq!(fts_query("budget"), Some("\"budget\"*".to_string()));
        assert_eq!(fts_query("q3 \"plan\" NEAR -x"), Some("\"q3\" AND \"\"\"plan\"\"\" AND \"NEAR\" AND \"-x\"*".to_string()));
    }

    #[test]
    fn tags_from_metadata_ignore_non_strings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("metadata.json"), r#"{"tags":["budget",3,"wd"]}"#).unwrap();
        assert_eq!(metadata_tags(dir.path()), vec!["budget", "wd"]);
        assert!(metadata_tags(&dir.path().join("missing")).is_empty());
    }
```

(Check `tempfile` is already a dev-dependency in `frontend/src-tauri/Cargo.toml`; library.rs tests use temp dirs, mirror whatever they use.)

- [ ] **Step 3: Implement the pure parts.**

```rust
//! spec phase 9 section 2: block-level full-text index over transcripts, titles and tags.

pub const BLOCK_SIZE: usize = 8;

pub struct Seg { pub start: f64, pub end: f64, pub text: String }
pub struct Block { pub start: f64, pub end: f64, pub text: String }

pub fn build_blocks(segs: &[Seg]) -> Vec<Block> {
    segs.chunks(BLOCK_SIZE)
        .map(|c| Block {
            start: c[0].start,
            end: c[c.len() - 1].end,
            text: c.iter().map(|s| s.text.trim()).collect::<Vec<_>>().join("\n"),
        })
        .collect()
}

/// Each term quoted so FTS5 syntax is literal, AND-ed, last term prefix-matched.
pub fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input.split_whitespace().map(|t| format!("\"{}\"", t.replace('"', "\"\""))).collect();
    if terms.is_empty() { return None; }
    Some(format!("{}*", terms.join(" AND ")))
}

pub fn metadata_tags(folder: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(folder.join("metadata.json")).ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("tags").and_then(|t| t.as_array()).cloned())
        .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
        .unwrap_or_default()
}
```

Run `cargo test --lib search_index`: PASS.

- [ ] **Step 4: Failing test for index + search ranking.**

```rust
    #[tokio::test]
    async fn title_hit_ranks_above_text_hit_and_hits_carry_times() {
        let p = pool().await;
        insert_blocks(&p, "m-text", "Weekly sync", &[], &[Block { start: 30.0, end: 60.0, text: "we discussed the budget".into() }]).await.unwrap();
        insert_blocks(&p, "m-title", "Budget review", &["finance".into()], &[Block { start: 0.0, end: 20.0, text: "hello".into() }]).await.unwrap();
        let r = search(&p, "budget", 10).await.unwrap();
        assert_eq!(r.iter().map(|m| m.meeting_id.as_str()).collect::<Vec<_>>(), vec!["m-title", "m-text"]);
        assert_eq!(r[1].hits[0].start_seconds, 30.0);
        assert!(r[1].hits[0].snippet.contains("**budget**"));
        let by_tag = search(&p, "finance", 10).await.unwrap();
        assert_eq!(by_tag[0].tags, vec!["finance"]);
        assert!(search(&p, "\"", 10).await.unwrap().is_empty());
    }
```

- [ ] **Step 5: Implement index writes and search.**

```rust
use sqlx::SqlitePool;

/// Replace one meeting's rows. Tags are stored space-joined in the `tags` column.
pub async fn insert_blocks(pool: &SqlitePool, meeting_id: &str, title: &str, tags: &[String], blocks: &[Block]) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM search_index WHERE meeting_id = ?").bind(meeting_id).execute(&mut *tx).await.map_err(|e| e.to_string())?;
    let tags_col = tags.join(" ");
    for b in blocks {
        sqlx::query("INSERT INTO search_index (meeting_id, start_seconds, end_seconds, title, tags, text) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(meeting_id).bind(b.start).bind(b.end).bind(title).bind(&tags_col).bind(&b.text)
            .execute(&mut *tx).await.map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())
}

pub async fn remove_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM search_index WHERE meeting_id = ?").bind(meeting_id).execute(pool).await.map(|_| ()).map_err(|e| e.to_string())
}

/// Reads segments from the `transcripts` table, title from `meetings`, tags from metadata.json.
pub async fn reindex_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<(), String> {
    let row: Option<(String, Option<String>)> = sqlx::query_as("SELECT title, folder_path FROM meetings WHERE id = ?")
        .bind(meeting_id).fetch_optional(pool).await.map_err(|e| e.to_string())?;
    let Some((title, folder)) = row else { return remove_meeting(pool, meeting_id).await };
    let rows: Vec<(String, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT transcript, audio_start_time, audio_end_time FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time, timestamp")
        .bind(meeting_id).fetch_all(pool).await.map_err(|e| e.to_string())?;
    let segs: Vec<Seg> = rows.into_iter().map(|(text, s, e)| Seg { start: s.unwrap_or(0.0), end: e.or(s).unwrap_or(0.0), text }).collect();
    let tags = folder.map(|f| metadata_tags(std::path::Path::new(&f))).unwrap_or_default();
    // a meeting with no transcript still gets one row so title and tag searches find it
    let mut blocks = build_blocks(&segs);
    if blocks.is_empty() { blocks.push(Block { start: 0.0, end: 0.0, text: String::new() }); }
    insert_blocks(pool, meeting_id, &title, &tags, &blocks).await
}

/// ponytail: full rebuild at startup; rebuild only rows whose meetings.updated_at moved if this gets slow.
pub async fn rebuild(pool: &SqlitePool) -> Result<usize, String> {
    let ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM meetings").fetch_all(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM search_index").execute(pool).await.map_err(|e| e.to_string())?;
    for (id,) in &ids {
        if let Err(e) = reindex_meeting(pool, id).await { log::warn!("search index: skip {}: {}", id, e); }
    }
    Ok(ids.len())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Hit { pub start_seconds: f64, pub end_seconds: f64, pub snippet: String }
#[derive(Debug, Clone, serde::Serialize)]
pub struct MeetingHit { pub meeting_id: String, pub title: String, pub tags: Vec<String>, pub score: f64, pub hits: Vec<Hit> }

pub async fn search(pool: &SqlitePool, query: &str, limit: usize) -> Result<Vec<MeetingHit>, String> {
    let Some(q) = fts_query(query) else { return Ok(vec![]) };
    let rows: Vec<(String, f64, f64, String, String, String, f64)> = sqlx::query_as(
        "SELECT meeting_id, start_seconds, end_seconds, title, tags, \
         snippet(search_index, 5, '**', '**', '…', 24), bm25(search_index, 0, 0, 0, 5.0, 3.0, 1.0) AS score \
         FROM search_index WHERE search_index MATCH ? ORDER BY score LIMIT 500")
        .bind(&q).fetch_all(pool).await
        // search-as-you-type: an FTS5 syntax error (e.g. a lone quote) is "no results", not an error
        .unwrap_or_else(|e| { log::debug!("search index: query {:?} failed: {}", q, e); vec![] });
    let mut out: Vec<MeetingHit> = Vec::new();
    for (id, start, end, title, tags, snip, score) in rows {
        let hit = Hit { start_seconds: start, end_seconds: end, snippet: snip };
        match out.iter_mut().find(|m| m.meeting_id == id) {
            Some(m) => if m.hits.len() < 3 { m.hits.push(hit) },
            None => out.push(MeetingHit {
                meeting_id: id, title, score,
                tags: tags.split_whitespace().map(String::from).collect(),
                hits: vec![hit],
            }),
        }
    }
    out.truncate(limit);
    Ok(out)
}
```

Run the test: PASS. If `audio_start_time`/`audio_end_time` are not REAL in the schema (check `grep -rn audio_start_time frontend/src-tauri/migrations`), adapt the tuple types.

- [ ] **Step 6: Wire it in.**
  - `TranscriptsRepository::search_transcripts(pool, query)` calls `search_index::search(pool, query, 50)` and maps each `MeetingHit` to one `TranscriptSearchResult { id, title, match_context: hits[0].snippet, timestamp: <the meeting's created_at or empty string>, tags, start_seconds: hits[0].start_seconds }`. Add `tags: Vec<String>` and `start_seconds: f64` to the struct. Keep `get_match_context` only if something else uses it; otherwise delete it.
  - `lib.rs` after reconcile (603-610): spawn `search_index::rebuild(pool.clone())` in the background with `tauri::async_runtime::spawn`, logging the count.
  - `library::assign`: after the meetings row is updated, `let _ = search_index::reindex_meeting(pool, meeting_id).await;` (log on error).
  - `MeetingsRepository::delete_meeting`: after a successful delete, `search_index::remove_meeting`.
  - After the live transcript is saved (`api_save_transcript`, once the meeting id is known) and in `retranscription.rs` right after `set_enhance_status(... STATUS_DONE ...)`: `reindex_meeting` (log on error).
- [ ] **Step 7: Gates.** `cargo check`, `cargo test --lib search_index`, `cargo test --lib library` (assign tests still pass).
- [ ] **Step 8: Commit** `feat: ranked full-text search index over titles, tags and transcript blocks`.

---

### Task 2: Auto title and tags (Rust)

**Files:**
- Create: `frontend/src-tauri/src/summary/llm_config.rs` (register in `summary/mod.rs`)
- Create: `frontend/src-tauri/src/audio/meeting_tagging.rs` (register in `audio/mod.rs`)
- Modify: `frontend/src-tauri/src/summary/service.rs:353-430` (use the helper)
- Modify: `frontend/src-tauri/src/audio/transcript_md.rs` (tags in frontmatter)
- Modify: `frontend/src-tauri/src/audio/recording_saver.rs:585`, `audio/retranscription.rs:678` (pass tags)
- Modify: `frontend/src-tauri/src/audio/retranscription.rs:162-205` (guard scope, call tagging, re-read folder for the hook)
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs` (`auto_title_tags`)
- Modify: `frontend/src-tauri/src/lib.rs` (register commands)

**Interfaces:**
- Consumes: `search_index::reindex_meeting`, `library::assign(pool, root, id, None, Some(title))`, `library::is_auto_title(&str)`, `llm_client::generate_summary(...)` (`summary/llm_client.rs:259`), `SettingsRepository::get_model_config / get_api_key / get_custom_openai_config` (`database/repositories/setting.rs`).
- Produces:
  - `llm_config::LlmConfig { provider: LLMProvider, model: String, api_key: String, ollama_endpoint: Option<String>, custom_openai_endpoint: Option<String>, max_tokens: Option<u32>, temperature: Option<f32>, top_p: Option<f32> }`
  - `llm_config::load_for(pool, provider: &str, model: &str) -> Result<LlmConfig, String>` and `llm_config::load(pool) -> Result<LlmConfig, String>` (provider and model from the `settings` row)
  - `meeting_tagging::run_step<R: Runtime>(app: &AppHandle<R>, meeting_id: &str, force_title: bool) -> Result<String, String>`
  - Commands `run_meeting_tagging(meeting_id) -> Result<String, String>`, `tag_untagged_meetings() -> Result<String, String>`, `list_tagging_statuses() -> Vec<{meeting_id, status}>`
  - Event `tagging-status-changed {meeting_id, status}` with status `running` | `done` | `failed`
  - `TranscriptMdMeta` gains `pub tags: &'a [String]`
  - Pref `auto_title_tags: bool` (default true)

- [ ] **Step 1: Pref.** Add `#[serde(default = "default_true")] pub auto_title_tags: bool` to `RecordingPreferences`, `auto_title_tags: true` in `impl Default`, and copy an existing old-JSON test asserting the default is true.

- [ ] **Step 2: Failing tests for the pure functions** in `meeting_tagging.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_normalized_deduped_and_capped() {
        let raw: Vec<String> = ["Western Digital", "western_digital", " Q3 Budget! ", "", "a", "b", "c", "d", "e"].iter().map(|s| s.to_string()).collect();
        assert_eq!(normalize_tags(&raw), vec!["western-digital", "q3-budget", "a", "b", "c", "d"]);
        assert_eq!(normalize_tags(&["Réunion".to_string()]), vec!["réunion"]);
    }

    #[test]
    fn title_is_cleaned() {
        assert_eq!(clean_title("  \"Budget review\"  "), "Budget review");
        assert_eq!(clean_title(&"x".repeat(100)).chars().count(), 80);
    }

    #[test]
    fn reply_json_is_extracted_from_chatter() {
        let r = parse_reply("Sure! {\"title\": \"WD onboarding\", \"tags\": [\"Onboarding\", \"wd\"]} Hope it helps").unwrap();
        assert_eq!(r, ("WD onboarding".to_string(), vec!["onboarding".to_string(), "wd".to_string()]));
        assert!(parse_reply("no json").is_err());
        assert!(parse_reply("{\"title\": \"\", \"tags\": [\"a\"]}").is_err());
        assert!(parse_reply("{\"title\": \"t\", \"tags\": []}").is_err());
    }

    #[test]
    fn long_transcripts_keep_head_and_tail() {
        let s = format!("{}{}", "a".repeat(20_000), "b".repeat(20_000));
        let t = prompt_text(&s);
        assert!(t.starts_with(&"a".repeat(16_000)) && t.ends_with(&"b".repeat(8_000)) && t.contains("[...]"));
        assert_eq!(prompt_text("short"), "short");
    }

    #[test]
    fn frontmatter_tags_line_is_inserted_or_replaced() {
        let md = "---\ntitle: \"t\"\nquality: enhanced\n---\n[00:00] hi\n";
        let once = set_frontmatter_tags(md, &["a".into(), "b".into()]);
        assert_eq!(once, "---\ntitle: \"t\"\nquality: enhanced\ntags: [a, b]\n---\n[00:00] hi\n");
        assert_eq!(set_frontmatter_tags(&once, &["c".into()]), "---\ntitle: \"t\"\nquality: enhanced\ntags: [c]\n---\n[00:00] hi\n");
        assert_eq!(set_frontmatter_tags("no frontmatter", &["a".into()]), "no frontmatter");
    }
}
```

- [ ] **Step 3: Implement them.**

```rust
//! spec phase 9 section 1: after enhance, the summary model names untitled meetings and tags every meeting.

pub const MAX_TAGS: usize = 6;
const HEAD: usize = 16_000;
const TAIL: usize = 8_000;

pub const SYSTEM_PROMPT: &str = "You name and tag meeting transcripts. Reply with JSON only, no prose: \
{\"title\": \"<3 to 8 words, no date, no quotes>\", \"tags\": [\"<1 to 3 words>\", ...]}. \
Give 3 to 6 tags naming topics, companies, roles or decisions. Use the transcript's language.";

pub fn normalize_tags(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in raw {
        let t: String = t.trim().to_lowercase().chars()
            .map(|c| if c == ' ' || c == '_' { '-' } else { c })
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect();
        let t = t.trim_matches('-').to_string();
        if !t.is_empty() && !out.contains(&t) { out.push(t); }
        if out.len() == MAX_TAGS { break; }
    }
    out
}

pub fn clean_title(raw: &str) -> String {
    raw.trim().trim_matches(|c| c == '"' || c == '\'' || c == '“' || c == '”').trim().chars().take(80).collect()
}

pub fn parse_reply(reply: &str) -> Result<(String, Vec<String>), String> {
    let (Some(a), Some(b)) = (reply.find('{'), reply.rfind('}')) else { return Err("The model did not return JSON".into()) };
    #[derive(serde::Deserialize)]
    struct Reply { title: String, #[serde(default)] tags: Vec<String> }
    let r: Reply = serde_json::from_str(&reply[a..=b]).map_err(|e| format!("The model returned invalid JSON: {}", e))?;
    let (title, tags) = (clean_title(&r.title), normalize_tags(&r.tags));
    if title.is_empty() || tags.is_empty() { return Err("The model returned an empty title or no tags".into()); }
    Ok((title, tags))
}

pub fn prompt_text(transcript: &str) -> String {
    let n = transcript.chars().count();
    if n <= HEAD + TAIL { return transcript.to_string(); }
    let head: String = transcript.chars().take(HEAD).collect();
    let tail: String = transcript.chars().skip(n - TAIL).collect();
    format!("{}\n[...]\n{}", head, tail)
}

pub fn set_frontmatter_tags(md: &str, tags: &[String]) -> String {
    let Some(rest) = md.strip_prefix("---\n") else { return md.to_string() };
    let Some(end) = rest.find("\n---\n") else { return md.to_string() };
    let front: Vec<&str> = rest[..end].lines().filter(|l| !l.starts_with("tags:")).collect();
    format!("---\n{}\ntags: [{}]{}", front.join("\n"), tags.join(", "), &rest[end..])
}
```

Run `cargo test --lib meeting_tagging`: PASS.

- [ ] **Step 4: Tags in `transcript.md` renders.** Add `pub tags: &'a [String]` to `TranscriptMdMeta`; in `render_transcript_md`, after the speakers line: `if !meta.tags.is_empty() { out.push_str(&format!("tags: [{}]\n", meta.tags.join(", "))); }`. Both callers (`recording_saver.rs:585`, `retranscription.rs:678`) pass `tags: &tags` where `let tags = search_index::metadata_tags(folder);`. Existing tests pass `tags: &[]`; add one test that a tagged meta renders `tags: [a, b]` after `speakers:` and before `---`. Run `cargo test --lib transcript_md`.

- [ ] **Step 5: `llm_config.rs`.** Move the provider parse, API-key, Ollama-endpoint and custom-OpenAI blocks of `service.rs:353-427` into `load_for(pool, provider, model) -> Result<LlmConfig, String>`, keeping the exact error strings ("API key not found for {provider}", "Custom OpenAI provider selected but no configuration found", ...). `load(pool)` reads `SettingsRepository::get_model_config(pool)`; no row, or an empty provider or model, gives `Err("No summary model configured. Set one in Settings, Summary.")`. In `service.rs`, replace the moved code with one `match llm_config::load_for(&pool, &model_provider, &model_name).await { Ok(c) => c, Err(e) => { Self::fail_and_cleanup(&pool, &meeting_id, started_at, &e).await; return; } }` and use the fields where the old locals were used (`provider`, `final_api_key`, `ollama_endpoint`, `custom_openai_*`). The summary behaviour must not change: run `cargo test --lib summary`.

- [ ] **Step 6: The step.** In `meeting_tagging.rs`:
  - Status: copy the 20-line in-memory map of `hook_status.rs` (`set`, `memory_status`), with `done` = metadata.json has a non-empty `tags` array (`search_index::metadata_tags(folder)` non-empty). `list_tagging_statuses` mirrors `list_hook_statuses`. Every transition emits `tagging-status-changed`.
  - `run_step(app, meeting_id, force_title)`:
    1. `let state = app.state::<AppState>(); let pool = state.db_manager.pool();`
    2. Load `(title, folder_path)` from `meetings`. Refuse "This meeting is being recorded right now." when the folder is the live folder (reuse the check `library::assign` uses near `library.rs:335`). No folder: "Folder not found on disk".
    3. Transcript text: `SELECT transcript FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time, timestamp`, joined with `\n`. Empty: `Err("No transcript yet.")`.
    4. `let cfg = llm_config::load(pool).await?;` then `llm_client::generate_summary(&reqwest::Client::new(), &cfg.provider, &cfg.model, &cfg.api_key, SYSTEM_PROMPT, &prompt_text(&text), cfg.ollama_endpoint.as_deref(), cfg.custom_openai_endpoint.as_deref(), cfg.max_tokens, cfg.temperature, cfg.top_p, app_data_dir.as_ref(), None)`. Get `app_data_dir` the same way `summary/service.rs` does for the built-in model (`grep -n app_data_dir frontend/src-tauri/src/summary/service.rs`). Then `parse_reply(&completion.content)?`.
    5. Write `metadata.json`: read the JSON, set `tags` and `tagged_at` (UTC RFC 3339), write through a temp file and rename (as `enhance_status::write_enhance_status` does).
    6. If `transcript.md` exists, rewrite it with `set_frontmatter_tags`.
    7. Title: `if force_title || title.trim().is_empty() || library::is_auto_title(&title)`, call `library::assign(pool, &crate::audio::get_default_recordings_folder(), meeting_id, None, Some(&new_title))`. An `Err` starting with "Folder not found on disk" is logged and ignored; any other `Err` is returned.
    8. `search_index::reindex_meeting` (log on error). Return `Ok("Title and tags updated".into())`.
    9. The wrapper sets status `running` before and `done`/`failed` after, and logs failures with the meeting id.
  - `run_meeting_tagging(app, meeting_id)`: `run_step(&app, &meeting_id, true).await`.
  - `tag_untagged_meetings(app)`: for every meeting with a folder whose `metadata_tags` is empty and that has transcript rows, sequentially `run_step(&app, id, false)`; return `"Tagged {ok} of {n} meetings"`.
  - Register the three commands in `lib.rs`'s `generate_handler!`.

- [ ] **Step 7: Enhance flow.** In `retranscription.rs` `start_retranscription`:
  - Scope the guard to the transcription work so `library::assign` can rename afterwards. Keep `_queue_slot` held to the end of the function, so post-processing is serialized too:

```rust
    let result = {
        let _guard = RetranscriptionGuard::acquire(&meeting_id).map_err(|e| anyhow!(e))?;
        RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);
        set_enhance_status(&app, &meeting_id, &folder, &provider_name, &model_name, STATUS_RUNNING, None);
        run_retranscription(app.clone(), meeting_id.clone(), meeting_folder_path, language, model, provider).await
    };
```

    Check that `library::assign`'s "enhance holds the folder" check (near `library.rs:329`) reads `running_meeting_id()` (the guard), not the queue lock. If it reads the queue lock, report BLOCKED.
  - In `Ok(res)` after `STATUS_DONE` and the reindex of Task 1: load prefs once. If `res.segments_count > 0 && prefs.auto_title_tags`, call `meeting_tagging::run_step(&app, &meeting_id, false).await` and log its result. Then, before the hook dispatch, re-read `folder_path` from `meetings` (the title may have renamed the folder) and build the hook context from that folder.
- [ ] **Step 8: Gates.** `cargo check`, `cargo test --lib meeting_tagging`, `cargo test --lib transcript_md`, `cargo test --lib recording_preferences`, `cargo test --lib summary`, `cargo test --lib retranscription`.
- [ ] **Step 9: Commit** `feat: name and tag meetings after enhance with the summary model`.

---

### Task 3: Audio cleanup (Rust)

**Files:**
- Create: `frontend/src-tauri/src/audio/audio_cleanup.rs` (register in `audio/mod.rs`)
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs`, `audio/retranscription.rs:298-326` (`find_audio_file` → `pub(crate)`, clearer error), `lib.rs` (spawn the loop after the DB is ready, near 603-617; register commands)

**Interfaces:**
- Consumes: `retranscription::find_audio_file(folder: &Path) -> Result<PathBuf>`, `retranscription::running_meeting_id()`, `search_index::metadata_tags`.
- Produces:
  - `audio_cleanup::check(meta: &serde_json::Value, transcript_md: Option<&str>, now: DateTime<Utc>, retention_hours: Option<u32>) -> Result<(), String>` (`None` = manual, no retention window)
  - `audio_cleanup::delete_audio(folder: &Path, now: DateTime<Utc>) -> std::io::Result<u64>` (bytes freed)
  - `audio_cleanup::run_once(pool: &SqlitePool, now: DateTime<Utc>, retention_hours: u32) -> usize`
  - `audio_cleanup::run<R: Runtime>(app: AppHandle<R>)` (loop)
  - Commands `delete_meeting_audio(meeting_id) -> Result<String, String>` and `get_meeting_extras(meeting_id) -> Result<MeetingExtras, String>` with `MeetingExtras { tags: Vec<String>, audio_deleted_at: Option<String>, has_audio: bool }`
  - Prefs `delete_audio_after_enhance: bool` (default true), `audio_retention_hours: u32` (default 24)

- [ ] **Step 1: Prefs.** `#[serde(default = "default_true")] delete_audio_after_enhance`, `#[serde(default = "default_audio_retention_hours")] audio_retention_hours: u32` with `fn default_audio_retention_hours() -> u32 { 24 }`, both in `impl Default`, plus a default test.

- [ ] **Step 2: Failing tests.**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    const MD: &str = "---\ntitle: \"t\"\nquality: enhanced\n---\n[00:00] hi\n";
    fn now() -> DateTime<Utc> { Utc.with_ymd_and_hms(2026, 10, 28, 12, 0, 0).unwrap() }
    fn meta(status: &str, at: &str) -> serde_json::Value { json!({"transcription": {"enhanced": {"status": status, "at": at}}}) }

    #[test]
    fn deletes_only_enhanced_old_and_transcribed() {
        assert!(check(&meta("done", "2026-10-27T11:00:00Z"), Some(MD), now(), Some(24)).is_ok());
        assert!(check(&meta("done", "2026-10-27T13:00:00Z"), Some(MD), now(), Some(24)).is_err());
        assert!(check(&meta("running", "2026-10-20T00:00:00Z"), Some(MD), now(), Some(24)).is_err());
        assert!(check(&meta("done", "2026-10-20T00:00:00Z"), None, now(), Some(24)).is_err());
        assert!(check(&meta("done", "2026-10-20T00:00:00Z"), Some("---\nquality: live\n---\n[00:00] hi\n"), now(), Some(24)).is_err());
        assert!(check(&meta("done", "2026-10-20T00:00:00Z"), Some("---\nquality: enhanced\n---\n"), now(), Some(24)).is_err());
        assert!(check(&json!({}), Some(MD), now(), Some(24)).is_err());
        let mut gone = meta("done", "2026-10-20T00:00:00Z");
        gone["audio_deleted_at"] = json!("2026-10-21T00:00:00Z");
        assert_eq!(check(&gone, Some(MD), now(), Some(24)).unwrap_err(), "Audio already removed.");
    }

    #[test]
    fn manual_skips_the_window_but_not_the_enhance_rule() {
        assert!(check(&meta("done", "2026-10-28T11:59:00Z"), Some(MD), now(), None).is_ok());
        assert_eq!(check(&json!({}), Some(MD), now(), None).unwrap_err(), "Enhance first: the audio is the only full copy of this meeting.");
    }

    #[test]
    fn delete_removes_audio_and_checkpoints_and_marks_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path();
        std::fs::write(f.join("audio.mp4"), vec![0u8; 1000]).unwrap();
        std::fs::create_dir(f.join(".checkpoints")).unwrap();
        std::fs::write(f.join(".checkpoints").join("audio_chunk_000.mp4"), vec![0u8; 10]).unwrap();
        std::fs::write(f.join("metadata.json"), r#"{"audio_file":"audio.mp4","title":"t"}"#).unwrap();
        std::fs::write(f.join("transcript.md"), MD).unwrap();
        assert_eq!(delete_audio(f, now()).unwrap(), 1010);
        assert!(!f.join("audio.mp4").exists() && !f.join(".checkpoints").exists() && f.join("transcript.md").exists());
        let m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(f.join("metadata.json")).unwrap()).unwrap();
        assert_eq!(m["audio_file"], "");
        assert_eq!(m["title"], "t");
        assert!(m["audio_deleted_at"].is_string());
    }
}
```

- [ ] **Step 3: Implement.**

```rust
//! spec phase 9 section 3: delete audio once the enhanced transcript is safely on disk.

use chrono::{DateTime, Utc};
use std::path::Path;

pub const ENHANCE_FIRST: &str = "Enhance first: the audio is the only full copy of this meeting.";

fn has_enhanced_transcript(md: &str) -> bool {
    let Some(rest) = md.strip_prefix("---\n") else { return false };
    let Some(end) = rest.find("\n---\n") else { return false };
    rest[..end].lines().any(|l| l.trim() == "quality: enhanced")
        && rest[end + 5..].lines().any(|l| l.starts_with('['))
}

pub fn check(meta: &serde_json::Value, transcript_md: Option<&str>, now: DateTime<Utc>, retention_hours: Option<u32>) -> Result<(), String> {
    if meta.get("audio_deleted_at").and_then(|v| v.as_str()).is_some() { return Err("Audio already removed.".into()); }
    let enhanced = &meta["transcription"]["enhanced"];
    if enhanced["status"].as_str() != Some("done") { return Err(ENHANCE_FIRST.into()); }
    if !transcript_md.map(has_enhanced_transcript).unwrap_or(false) { return Err(ENHANCE_FIRST.into()); }
    if let Some(hours) = retention_hours {
        let at = enhanced["at"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).ok_or("No enhance time recorded")?;
        if now.signed_duration_since(at.with_timezone(&Utc)) < chrono::Duration::hours(hours as i64) {
            return Err("Audio still inside the retention window".into());
        }
    }
    Ok(())
}

fn size_of(p: &Path) -> u64 {
    if p.is_dir() {
        std::fs::read_dir(p).map(|d| d.flatten().map(|e| size_of(&e.path())).sum()).unwrap_or(0)
    } else {
        std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
    }
}

pub fn delete_audio(folder: &Path, now: DateTime<Utc>) -> std::io::Result<u64> {
    let mut freed = 0;
    if let Ok(audio) = super::retranscription::find_audio_file(folder) {
        freed += size_of(&audio);
        std::fs::remove_file(&audio)?;
    }
    let cp = folder.join(".checkpoints");
    if cp.exists() {
        freed += size_of(&cp);
        std::fs::remove_dir_all(&cp)?;
    }
    let meta_path = folder.join("metadata.json");
    let mut meta: serde_json::Value = std::fs::read_to_string(&meta_path).ok()
        .and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| serde_json::json!({}));
    meta["audio_file"] = "".into();
    meta["audio_deleted_at"] = now.to_rfc3339().into();
    let tmp = folder.join(".metadata.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&meta)?)?;
    std::fs::rename(&tmp, &meta_path)?;
    Ok(freed)
}
```

Adapt `find_audio_file`'s signature if it differs (it lives at `retranscription.rs:298`; make it `pub(crate)`). Run `cargo test --lib audio_cleanup`: PASS.

- [ ] **Step 4: `run_once`, loop, commands.**
  - `run_once(pool, now, retention_hours)`: for each `(id, folder_path)` in `SELECT id, folder_path FROM meetings WHERE folder_path IS NOT NULL`: skip when `Some(id) == retranscription::running_meeting_id()` or the folder is the live recording folder (same check as Task 2 step 6.2). Read metadata and `transcript.md`. When `check(..., Some(retention_hours))` passes, run `delete_audio` and `info!("audio cleanup: {} freed {} bytes", id, n)`. Failures are `warn!`. Return the count deleted.
  - `run(app)`: `loop { let prefs = load_recording_preferences(&app).await; if enabled { run_once(...) }; tokio::time::sleep(Duration::from_secs(3600)).await; }`. Startup run first, then hourly. Spawn it in `lib.rs` after the DB is ready and reconcile ran, with `tauri::async_runtime::spawn`, mirroring the `call_detector::run` spawn (not macOS-gated).
  - `delete_meeting_audio(app, meeting_id)`: live → "This meeting is being recorded right now."; running enhance → "This meeting is being enhanced right now."; then `check(meta, md, Utc::now(), None)?`, `delete_audio`, return `format!("Audio deleted ({:.1} MB freed)", bytes as f64 / 1e6)`.
  - `get_meeting_extras(app, meeting_id)`: folder from `meetings`; `tags = metadata_tags`, `audio_deleted_at` from metadata, `has_audio = find_audio_file(folder).is_ok()`.
  - `retranscription.rs`: where a missing audio file becomes an error, check metadata `audio_deleted_at` first and return `anyhow!("Audio was removed after enhance")`.
  - Register both commands in `lib.rs`.
- [ ] **Step 5: `run_once` test** on a temp folder plus an in-memory pool with the real migrations (`sqlx::migrate!("./migrations").run(&pool)`, as library.rs tests do; mirror them). Two meetings: an eligible one, and one enhanced one hour ago. Only the first loses its audio, and the function returns 1.
- [ ] **Step 6: Gates.** `cargo check`, `cargo test --lib audio_cleanup`, `cargo test --lib recording_preferences`, `cargo test --lib retranscription`.
- [ ] **Step 7: Commit** `feat: delete audio after enhance, hourly with a retention window, and on demand`.

---

### Task 4: MCP search (Node)

**Files:**
- Create: `mcp/src/search.ts`, `mcp/test/search.test.ts`
- Modify: `mcp/src/library.ts` (`MeetingInfo.tags`, export `readSegments`), `mcp/src/index.ts` (tool, `list_meetings` tag filter), `mcp/README.md`, `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md` sections 5.2 to 5.4

**Interfaces:**
- Consumes: `scanMeetings(root)`, `readSegments(info)` (now exported; returns `{start, text, speaker, raw}`; end time is `raw.audio_end_time` when numeric, else `start`).
- Produces: `ftsQuery(input: string): string | null`, `buildBlocks(segs, size = 8)`, `searchMeetings(root, opts): SearchResult[]`

- [ ] **Step 0:** `cd mcp && npm ci` if `node_modules` is missing in the worktree.
- [ ] **Step 1: Failing tests** in `mcp/test/search.test.ts`. Reuse the fixture style of `library.test.ts`: a v2 folder `dex/2026-10-27_12-03 wd` with `metadata.json` (`title: 'WD onboarding'`, `tags: ['onboarding','western-digital']`) and 10 segments, segment 9 saying "the budget is approved"; a second v2 folder in `flowers` titled "Budget review" without the word in its text; a legacy root folder whose `transcripts.json` says "legacy budget line".

```ts
test('ftsQuery quotes, ANDs and prefixes', () => {
  assert.equal(ftsQuery('  '), null);
  assert.equal(ftsQuery('budget'), '"budget"*');
  assert.equal(ftsQuery('a "b"'), '"a" AND """b"""*');
});

test('title hit ranks first, text hits carry block times, legacy is searched', () => {
  const r = searchMeetings(root, { query: 'budget' });
  assert.equal(r[0].title, 'Budget review');
  const wd = r.find((m) => m.title === 'WD onboarding')!;
  assert.equal(wd.hits[0].start_seconds, 80);        // second block starts at segment 8
  assert.match(wd.hits[0].snippet, /\*\*budget\*\*/);
  assert.ok(r.some((m) => m.project === '_inbox'));
});

test('filters: tags, project, since', () => {
  assert.deepEqual(searchMeetings(root, { query: 'budget', tags: ['onboarding'] }).map((m) => m.title), ['WD onboarding']);
  assert.ok(searchMeetings(root, { query: 'budget', project: 'flowers' }).every((m) => m.project === 'flowers'));
  assert.equal(searchMeetings(root, { query: 'budget', since: '2099-01-01' }).length, 0);
});

test('list_meetings info carries tags', () => {
  const m = scanMeetings(root).find((x) => x.title === 'WD onboarding')!;
  assert.deepEqual(m.tags, ['onboarding', 'western-digital']);
});
```

(Segment `i` in the fixture: `audio_start_time: i * 10`, `audio_end_time: i * 10 + 5`.) Run `npm test`: FAIL.

- [ ] **Step 2: Implement.** `library.ts`: add `tags: string[]` to `MeetingInfo`, read it in `infoFrom` (`Array.isArray(meta.tags) ? meta.tags.filter((t) => typeof t === 'string') : []`), and export `readSegments`. `search.ts`:

```ts
import { DatabaseSync } from 'node:sqlite';
import { scanMeetings, readSegments, type MeetingInfo } from './library.ts';

export type SearchOpts = { query: string; project?: string; tags?: string[]; since?: string; until?: string; limit?: number };
export type SearchResult = Pick<MeetingInfo, 'meeting_id' | 'project' | 'title' | 'started_at' | 'tags' | 'path'>
  & { score: number; hits: { start_seconds: number; end_seconds: number; snippet: string }[] };

export function ftsQuery(input: string): string | null {
  const terms = input.split(/\s+/).filter(Boolean).map((t) => `"${t.replaceAll('"', '""')}"`);
  return terms.length ? `${terms.join(' AND ')}*` : null;
}

export function buildBlocks(segs: { start: number; end: number; text: string }[], size = 8) {
  const out: { start: number; end: number; text: string }[] = [];
  for (let i = 0; i < segs.length; i += size) {
    const c = segs.slice(i, i + size);
    out.push({ start: c[0].start, end: c[c.length - 1].end, text: c.map((s) => s.text.trim()).join('\n') });
  }
  return out;
}

// ponytail: index rebuilt in memory per call; cache per folder by transcripts.json mtime past a few thousand meetings
export function searchMeetings(root: string, o: SearchOpts): SearchResult[] {
  const q = ftsQuery(o.query);
  if (!q) return [];
  const meetings = scanMeetings(root).filter((m) =>
    (!o.project || m.project === o.project)
    && (!o.since || m.started_at >= o.since)
    && (!o.until || m.started_at <= o.until)
    && (o.tags ?? []).every((t) => m.tags.includes(t)));
  const db = new DatabaseSync(':memory:');
  db.exec("CREATE VIRTUAL TABLE s USING fts5(idx UNINDEXED, start_seconds UNINDEXED, end_seconds UNINDEXED, title, tags, text, tokenize='unicode61 remove_diacritics 2')");
  const ins = db.prepare('INSERT INTO s VALUES (?, ?, ?, ?, ?, ?)');
  meetings.forEach((m, i) => {
    const segs = readSegments(m).map((s) => ({
      start: s.start,
      end: typeof s.raw.audio_end_time === 'number' ? s.raw.audio_end_time : s.start,
      text: s.text,
    }));
    const blocks = buildBlocks(segs);
    for (const b of blocks.length ? blocks : [{ start: 0, end: 0, text: '' }]) ins.run(i, b.start, b.end, m.title, m.tags.join(' '), b.text);
  });
  let rows: { idx: number; start_seconds: number; end_seconds: number; snip: string; score: number }[] = [];
  try {
    rows = db.prepare("SELECT idx, start_seconds, end_seconds, snippet(s, 5, '**', '**', '…', 24) AS snip, bm25(s, 0, 0, 0, 5.0, 3.0, 1.0) AS score FROM s WHERE s MATCH ? ORDER BY score LIMIT 500").all(q) as typeof rows;
  } catch { return []; }
  const out = new Map<number, SearchResult>();
  for (const r of rows) {
    const hit = { start_seconds: r.start_seconds, end_seconds: r.end_seconds, snippet: r.snip };
    const cur = out.get(r.idx);
    if (cur) { if (cur.hits.length < 3) cur.hits.push(hit); continue; }
    const { meeting_id, project, title, started_at, tags, path } = meetings[r.idx];
    out.set(r.idx, { meeting_id, project, title, started_at, tags, path, score: r.score, hits: [hit] });
  }
  return [...out.values()].slice(0, o.limit ?? 10);
}
```

Run `npm test`: PASS (existing library tests too, after adding `tags` where they compare whole `MeetingInfo` objects).

- [ ] **Step 3: Register the tool** in `index.ts`, following `list_meetings`:
  - Name `search_meetings`. Input: `query: z.string()`, `project`, `tags: z.array(z.string()).optional()`, `since`, `until`, `limit` (same zod style as `list_meetings`).
  - Description: "Keyword search (not semantic) over meeting titles, tags and transcript text under the Noesis recordings root. Ranked best first; each hit has start_seconds and end_seconds. Try synonyms and the other language if nothing matches, then call get_transcript with start_seconds/end_seconds around a hit to read the context."
  - Handler: `text(searchMeetings(root, args))`.
  - `list_meetings`: output each meeting's `tags`; add an optional `tag: z.string()` input that keeps meetings whose `tags` include it.
- [ ] **Step 4: Docs.**
  - `mcp/README.md`: the new tool, and that `node:sqlite` needs Node 22.13 or later. Bump `engines.node` only if it is below 22.13; it is `>=22.18` today, so leave it.
  - Main spec 5.2: add `tags`, `tagged_at`, `audio_deleted_at`.
  - Main spec 5.3: add `tags: [..]` after `speakers`.
  - Main spec 5.4: add the `search_meetings` schema from the phase 9 spec, and `tags`/`tag` on `list_meetings`.
- [ ] **Step 5: Commit** `feat: search_meetings MCP tool and tags on list_meetings`.

---

### Task 5: Frontend

**Files:**
- Modify: `frontend/src/components/RecordingSettings.tsx` (prefs type ~35, defaults ~60, switches near the hook switch ~347)
- Modify: `frontend/src/components/MeetingDetails/TranscriptButtonGroup.tsx` (buttons near Run hook ~115)
- Modify: `frontend/src/components/Sidebar/index.tsx` (context menu ~531), `frontend/src/components/Sidebar/SidebarProvider.tsx` (statuses, search result type ~271)
- Modify: `frontend/src/components/MeetingStatusTags.tsx`, the meeting page header (where `MeetingStatusTags` is rendered under `app/meeting-details`)

**Interfaces:**
- Consumes (Tauri):
  - `run_meeting_tagging({meetingId})`, `tag_untagged_meetings()`, `list_tagging_statuses()`, event `tagging-status-changed {meeting_id, status}`
  - `delete_meeting_audio({meetingId})`, `get_meeting_extras({meetingId}) -> {tags: string[], audio_deleted_at: string | null, has_audio: boolean}`
  - `api_search_transcripts` results gain `tags: string[]`, `start_seconds: number`
  - prefs `auto_title_tags`, `delete_audio_after_enhance`, `audio_retention_hours`

- [ ] **Step 0:** If `frontend/node_modules` is missing in the worktree: `cd frontend && pnpm install --frozen-lockfile`. If pnpm refuses, symlink: `ln -s /Users/lainealexandre/Code/Perso/noesis/frontend/node_modules frontend/node_modules` (it is gitignored), and say which in the report.
- [ ] **Step 1: Settings.** Add the three prefs to the `RecordingPreferences` type and defaults (true, true, 24). In the Recordings card, next to the hook switch and with the same `control={<Switch .../>}` pattern:
  - "Name and tag meetings after enhance", help "Uses your summary model. Off: run it from the meeting page or the right-click menu."
  - A "Tag meetings without tags" button that calls `tag_untagged_meetings` and toasts the returned string. While it runs, the button is disabled and shows a spinner.
  - "Delete audio after enhance", help "The transcript stays. Enhance and playback stop working for that meeting."
  - A number input "Keep audio for (hours)" (min 1, step 1). Saved through the same `applyPreferences` on blur; a value below 1 is clamped to 1.
- [ ] **Step 2: Status plumbing.** In `SidebarProvider`, fetch `list_tagging_statuses` next to `list_hook_statuses`, store `taggingStatus` on meetings and sidebar items, and listen to `tagging-status-changed` like `hook-status-changed`:
  - on `done`, refetch the meetings list, since the title may have changed;
  - on `failed`, toast "Title and tags failed".
  - Add `tags` and `start_seconds` to `TranscriptSearchResult`.
- [ ] **Step 3: Status tag.** `MeetingStatusTags` gains `taggingStatus?` with a lucide `Tags` icon. Same sizes and colors as the existing two. Labels: "Tagged" (done), "Tagging" (running), "Tagging failed" (failed). Pass it in the sidebar row and the meeting header.
- [ ] **Step 4: Meeting page.** Load `get_meeting_extras` for the meeting, and reload it on `tagging-status-changed` and after a delete.
  - **Tag chips:** under the title in the header, render `tags` as small read-only chips (`text-xs text-muted-foreground border border-border rounded px-1.5`).
  - **Title & tags button:** next to Run hook, same style, lucide `Tags` icon, label "Title & tags". It calls `run_meeting_tagging`, toasts the result or the error, and is disabled while `taggingStatus === 'running'`.
  - **Delete audio button:** lucide `Trash2`, destructive style. It opens a confirm dialog that reuses the dialog component the sidebar uses to confirm meeting deletion. Text: "Delete the audio of this meeting? The transcript stays. Enhance and playback will no longer be possible." Confirm calls `delete_meeting_audio` and toasts the result or the error.
  - **When `audio_deleted_at` is set:** disable both Enhance and Delete audio, with `title="Audio removed after enhance"`.
- [ ] **Step 5: Sidebar menu.** Above the separator before Delete, add:
  - "Regenerate title & tags", which calls `run_meeting_tagging`, disabled while tagging is running;
  - "Delete audio", which opens the same confirm dialog and then calls `delete_meeting_audio`.
  - Errors are toasted.
- [ ] **Step 6:** tsc gate. Commit `feat: title and tags, audio deletion and search tags in the UI`.

---

## Acceptance (owner, after Task 5)

1. Run the app from the worktree (`cd frontend && ./clean_run.sh`), with Meetily/Noesis main build quit.
2. Two-minute Zoom call. After enhance:
   - the folder gets a real title;
   - the header shows tags;
   - the sidebar search finds a spoken word and a tag.
3. In a wiki session: `search_meetings` returns the hit with `start_seconds`.
4. Set "Keep audio for" to 1, wait an hour (or restart the app after an hour). The audio is gone and Enhance reads "Audio removed after enhance".
5. Each manual button and menu item works.
