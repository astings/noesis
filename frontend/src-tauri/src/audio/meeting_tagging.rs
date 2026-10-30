//! spec phase 9 section 1: after enhance, the summary model names untitled meetings and tags every meeting.

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const MAX_TAGS: usize = 6;
const HEAD: usize = 16_000;
const TAIL: usize = 8_000;

pub const RUNNING: &str = "running";
pub const DONE: &str = "done";
pub const FAILED: &str = "failed";
pub const CANCELLED: &str = "cancelled";

pub const SYSTEM_PROMPT: &str = "You name and tag meeting transcripts. Reply with JSON only, no prose: \
{\"title\": \"<3 to 8 words, no date, no quotes>\", \"tags\": [\"<1 to 3 words>\", ...]}. \
Give 3 to 6 tags naming topics, companies, roles or decisions. Use the transcript's language.";

pub fn normalize_tags(raw: &[String]) -> Vec<String> {
    normalize_tags_up_to(raw, MAX_TAGS)
}

/// Lowercase, dashes for spaces, deduped; `cap` keeps the first ones.
pub fn normalize_tags_up_to(raw: &[String], cap: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in raw {
        let t: String = t.trim().to_lowercase().chars()
            .map(|c| if c == ' ' || c == '_' { '-' } else { c })
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect();
        let t = t.trim_matches('-').to_string();
        if !t.is_empty() && !out.contains(&t) { out.push(t); }
        if out.len() == cap { break; }
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

// In-memory `running` and `failed`; `done` is a non-empty `tags` array in metadata.json.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaggingStatusEntry {
    pub meeting_id: String,
    pub status: String,
}

static MEMORY: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();

fn memory() -> std::sync::MutexGuard<'static, HashMap<String, &'static str>> {
    MEMORY.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn set<R: Runtime>(app: &AppHandle<R>, meeting_id: &str, status: &'static str, error: Option<&str>) {
    if status == DONE {
        memory().remove(meeting_id);
    } else {
        memory().insert(meeting_id.to_string(), status);
    }
    let _ = app.emit(
        "tagging-status-changed",
        serde_json::json!({ "meeting_id": meeting_id, "status": status, "error": error }),
    );
}

fn status_for(meeting_id: &str, folder: Option<&Path>) -> Option<&'static str> {
    let in_memory = memory().get(meeting_id).copied();
    in_memory.or_else(|| folder.filter(|f| !super::search_index::metadata_tags(f).is_empty()).map(|_| DONE))
}

pub(crate) const CANCELLED_MSG: &str = "Cancelled";

/// Bridges a job's cancel flag to a token the provider call understands: HTTP providers race
/// it and the built-in sidecar is shut down, so no stale reply is left for the next request.
/// The caller aborts the poll task once the call returns. Only the model wait takes the token,
/// never the writes after it, so a cancel cannot split a rename.
fn cancel_token_for(job: Option<&str>) -> Option<(tokio_util::sync::CancellationToken, tokio::task::JoinHandle<()>)> {
    let job = job?.to_string();
    let token = tokio_util::sync::CancellationToken::new();
    let t = token.clone();
    let poll = tokio::spawn(async move {
        while !crate::jobs::is_cancel_requested(&job) {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        t.cancel();
    });
    Some((token, poll))
}

async fn run_inner<R: Runtime>(app: &AppHandle<R>, meeting_id: &str, force_title: bool, job: Option<&str>) -> Result<String, String> {
    let state = app.state::<crate::state::AppState>();
    let pool = state.db_manager.pool();

    let row: Option<(String, Option<String>)> = sqlx::query_as("SELECT title, folder_path FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    let Some((title, folder_path)) = row else { return Err(format!("No meeting found with id {}", meeting_id)) };
    if super::library::is_live_folder(folder_path.as_deref()).await {
        return Err("This meeting is being recorded right now.".to_string());
    }
    let folder = folder_path.map(PathBuf::from).filter(|p| p.is_dir()).ok_or("Folder not found on disk")?;
    // automatic runs keep hand-edited tags; the explicit action (force_title) regenerates them
    let keep_tags = !force_title && tags_edited(&folder);
    let needs_title = force_title || title.trim().is_empty() || super::library::is_auto_title(&title);
    if keep_tags && !needs_title {
        return Ok("Kept the hand-edited tags".to_string());
    }

    let lines: Vec<(String,)> = sqlx::query_as("SELECT transcript FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time, timestamp")
        .bind(meeting_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    let text = lines.into_iter().map(|(t,)| t).collect::<Vec<_>>().join("\n");
    if text.trim().is_empty() {
        return Err("No transcript yet.".to_string());
    }

    let custom_prompt = super::recording_preferences::load_recording_preferences(app)
        .await
        .map(|p| p.title_tags_prompt.trim().to_string())
        .unwrap_or_default();
    let system_prompt = if custom_prompt.is_empty() { SYSTEM_PROMPT } else { custom_prompt.as_str() };
    let cfg = crate::summary::llm_config::load(pool).await?;
    let app_data_dir = app.path().app_data_dir().ok();
    let cancel = cancel_token_for(job);
    let result = crate::summary::llm_client::generate_summary(
        &reqwest::Client::new(),
        &cfg.provider,
        &cfg.model,
        &cfg.api_key,
        system_prompt,
        &prompt_text(&text),
        cfg.ollama_endpoint.as_deref(),
        cfg.custom_openai_endpoint.as_deref(),
        cfg.max_tokens,
        cfg.temperature,
        cfg.top_p,
        app_data_dir.as_ref(),
        cancel.as_ref().map(|(t, _)| t),
    )
    .await;
    let was_cancelled = cancel.as_ref().is_some_and(|(t, _)| t.is_cancelled()) || job.is_some_and(crate::jobs::is_cancel_requested);
    if let Some((_, poll)) = &cancel {
        poll.abort();
    }
    let completion = match result {
        Ok(c) => c,
        Err(_) if was_cancelled => return Err(CANCELLED_MSG.to_string()),
        Err(e) => return Err(e.to_string()),
    };
    let (new_title, tags) = parse_reply(&completion.content)?;

    if !keep_tags {
        write_tags(&folder, &tags, false)?;
    }

    if needs_title {
        match super::library::assign(pool, &super::recording_preferences::recordings_root(app), meeting_id, None, Some(&new_title)).await {
            // the meeting page refetches its folder on this event
            Ok(_) => {
                let _ = app.emit("meeting-moved", serde_json::json!({ "meeting_id": meeting_id }));
            }
            Err(e) if e.starts_with("Folder not found on disk") => log::warn!("tagging: rename skipped for {}: {}", meeting_id, e),
            Err(e) => return Err(e),
        }
    }
    if let Err(e) = super::search_index::reindex_meeting(pool, meeting_id).await {
        log::warn!("search index: reindex after tagging failed for {}: {}", meeting_id, e);
    }
    Ok("Title and tags updated".to_string())
}

/// True once the user edited the tags on the meeting page (`tags_edited_at` in metadata.json).
pub fn tags_edited(folder: &Path) -> bool {
    std::fs::read_to_string(folder.join("metadata.json")).ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .is_some_and(|v| v.get("tags_edited_at").is_some_and(|t| t.is_string()))
}

/// `tags` and `tagged_at` into metadata.json, and the tags line into transcript.md when it exists.
/// `by_hand` sets `tags_edited_at`; a model write clears it.
fn write_tags(folder: &Path, tags: &[String], by_hand: bool) -> Result<(), String> {
    super::metadata_file::update_metadata(folder, |obj| {
        let now = serde_json::json!(chrono::Utc::now().to_rfc3339());
        obj.insert("tags".into(), serde_json::json!(tags));
        if by_hand {
            obj.insert("tags_edited_at".into(), now);
        } else {
            obj.insert("tagged_at".into(), now);
            obj.remove("tags_edited_at");
        }
        Ok(())
    })
    .map_err(|e| format!("Could not update metadata.json: {}", e))?;

    let md_path = folder.join("transcript.md");
    if let Ok(md) = std::fs::read_to_string(&md_path) {
        let tmp = folder.join(".transcript.md.tmp");
        std::fs::write(&tmp, set_frontmatter_tags(&md, tags)).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &md_path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Name (when untitled or `force_title`) and tag one meeting with the summary model.
pub async fn run_step<R: Runtime>(app: &AppHandle<R>, meeting_id: &str, force_title: bool, job: Option<&str>) -> Result<String, String> {
    set(app, meeting_id, RUNNING, None);
    let result = run_inner(app, meeting_id, force_title, job).await;
    match &result {
        Ok(_) => set(app, meeting_id, DONE, None),
        Err(e) if e == CANCELLED_MSG => set(app, meeting_id, CANCELLED, None),
        Err(e) => {
            log::warn!("tagging failed for {}: {}", meeting_id, e);
            set(app, meeting_id, FAILED, Some(e));
        }
    }
    result
}

/// The automatic step after enhance. With no summary model configured it does nothing
/// (auto titles are on by default), so the meeting is not marked failed.
pub async fn run_after_enhance<R: Runtime>(app: &AppHandle<R>, meeting_id: &str, job: Option<&str>) -> Result<String, String> {
    let state = app.state::<crate::state::AppState>();
    if let Err(e) = crate::summary::llm_config::load(state.db_manager.pool()).await {
        if e == crate::summary::llm_config::NO_SUMMARY_MODEL {
            log::info!("tagging: skipped for {}: {}", meeting_id, e);
            return Ok("Skipped: no summary model configured".to_string());
        }
    }
    run_step(app, meeting_id, false, job).await
}

#[tauri::command]
pub async fn run_meeting_tagging<R: Runtime>(app: AppHandle<R>, meeting_id: String) -> Result<String, String> {
    use crate::jobs::{self, Control, JobKind, Outcome};
    let job_id = uuid::Uuid::new_v4().to_string();
    jobs::add(&job_id, JobKind::Tagging, Some(&meeting_id), "", Control::Tagging, true);
    let result = run_step(&app, &meeting_id, true, Some(&job_id)).await;
    jobs::finish(&job_id, match &result {
        Ok(_) => Outcome::Done,
        Err(e) => Outcome::Failed(e.clone()),
    });
    result
}

/// Hand-edited tags from the meeting page: normalized like the model's, no count cap.
#[tauri::command]
pub async fn set_meeting_tags(state: tauri::State<'_, crate::state::AppState>, meeting_id: String, tags: Vec<String>) -> Result<Vec<String>, String> {
    let pool = state.db_manager.pool();
    let folder: Option<(Option<String>,)> = sqlx::query_as("SELECT folder_path FROM meetings WHERE id = ?")
        .bind(&meeting_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    let folder_path = folder.and_then(|(f,)| f);
    if super::library::is_live_folder(folder_path.as_deref()).await {
        return Err("This meeting is being recorded right now.".to_string());
    }
    let folder = folder_path.map(PathBuf::from).filter(|p| p.is_dir()).ok_or("Folder not found on disk")?;
    let tags = normalize_tags_up_to(&tags, usize::MAX);
    write_tags(&folder, &tags, true)?;
    if let Err(e) = super::search_index::reindex_meeting(pool, &meeting_id).await {
        log::warn!("search index: reindex after tag edit failed for {}: {}", meeting_id, e);
    }
    Ok(tags)
}

/// Tag every meeting that has a transcript but no tags yet, one after the other.
#[tauri::command]
pub async fn tag_untagged_meetings<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    use crate::jobs::{self, Control, JobKind, Outcome};
    let job_id = uuid::Uuid::new_v4().to_string();
    jobs::add(&job_id, JobKind::Tagging, None, "Untagged meetings", Control::Tagging, true);
    let result = tag_untagged(&app, &job_id).await;
    jobs::finish(&job_id, match &result {
        Ok(_) if jobs::is_cancel_requested(&job_id) => Outcome::Cancelled,
        Ok(_) => Outcome::Done,
        Err(e) => Outcome::Failed(e.clone()),
    });
    result
}

/// The batch behind `tag_untagged_meetings`; stops between meetings when the job is cancelled.
async fn tag_untagged<R: Runtime>(app: &AppHandle<R>, job_id: &str) -> Result<String, String> {
    let state = app.state::<crate::state::AppState>();
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT m.id, m.folder_path FROM meetings m WHERE m.folder_path IS NOT NULL \
         AND EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = m.id)",
    )
    .fetch_all(state.db_manager.pool())
    .await
    .map_err(|e| format!("Database error: {}", e))?;
    let todo: Vec<String> = rows
        .into_iter()
        .filter(|(_, f)| f.as_deref().map(Path::new).is_some_and(|f| super::search_index::metadata_tags(f).is_empty() && !tags_edited(f)))
        .map(|(id, _)| id)
        .collect();
    let mut ok = 0;
    for (i, id) in todo.iter().enumerate() {
        if crate::jobs::is_cancel_requested(job_id) {
            break;
        }
        crate::jobs::progress(job_id, (i * 100 / todo.len()) as u8, Some(format!("{} / {}", i, todo.len())));
        if run_step(app, id, false, Some(job_id)).await.is_ok() {
            ok += 1;
        }
    }
    Ok(format!("Tagged {} of {} meetings", ok, todo.len()))
}

/// The built-in system prompt, shown when `title_tags_prompt` is empty.
#[tauri::command]
pub fn get_default_title_tags_prompt() -> String {
    SYSTEM_PROMPT.to_string()
}

#[tauri::command]
pub async fn list_tagging_statuses(state: tauri::State<'_, crate::state::AppState>) -> Result<Vec<TaggingStatusEntry>, String> {
    let meetings = crate::database::repositories::meeting::MeetingsRepository::get_meetings(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to list meetings: {}", e))?;
    Ok(meetings
        .into_iter()
        .filter_map(|m| {
            let status = status_for(&m.id, m.folder_path.as_deref().map(Path::new))?;
            Some(TaggingStatusEntry { meeting_id: m.id, status: status.to_string() })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_token_for_cancels_the_token_when_the_job_is_cancelled() {
        assert!(cancel_token_for(None).is_none());
        crate::jobs::add("tag-c", crate::jobs::JobKind::Tagging, Some("m"), "", crate::jobs::Control::Tagging, true);
        let (token, poll) = cancel_token_for(Some("tag-c")).unwrap();
        assert!(!token.is_cancelled());
        crate::jobs::request_cancel("tag-c");
        tokio::time::timeout(std::time::Duration::from_secs(1), token.cancelled()).await.unwrap();
        poll.abort();
        crate::jobs::finish("tag-c", crate::jobs::Outcome::Cancelled);
    }

    #[test]
    fn tags_are_normalized_deduped_and_capped() {
        let raw: Vec<String> = ["Western Digital", "western_digital", " Q3 Budget! ", "", "a", "b", "c", "d", "e"].iter().map(|s| s.to_string()).collect();
        assert_eq!(normalize_tags(&raw), vec!["western-digital", "q3-budget", "a", "b", "c", "d"]);
        assert_eq!(normalize_tags(&["Réunion".to_string()]), vec!["réunion"]);
        assert_eq!(normalize_tags_up_to(&raw, usize::MAX).len(), 7);
    }

    #[test]
    fn write_tags_preserves_existing_keys_and_updates_transcript_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("metadata.json"), r#"{"title":"T","version":2,"tags":["old"]}"#).unwrap();
        std::fs::write(dir.path().join("transcript.md"), "---\ntitle: \"T\"\n---\n[00:00] hi\n").unwrap();
        write_tags(dir.path(), &["a".into(), "b".into()], false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join("metadata.json")).unwrap()).unwrap();
        assert_eq!(v["title"], "T");
        assert_eq!(v["version"], 2);
        assert_eq!(v["tags"], serde_json::json!(["a", "b"]));
        assert!(v["tagged_at"].is_string());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("transcript.md")).unwrap(),
            "---\ntitle: \"T\"\ntags: [a, b]\n---\n[00:00] hi\n"
        );
    }

    #[test]
    fn write_tags_refuses_unparseable_or_unreadable_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("metadata.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(write_tags(dir.path(), &["a".into()], false).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ not json");
        std::fs::write(&p, [0xff, 0xfe, 0xfd]).unwrap();
        assert!(write_tags(dir.path(), &["a".into()], false).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), vec![0xff, 0xfe, 0xfd]);
    }

    #[test]
    fn write_tags_creates_metadata_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        write_tags(dir.path(), &["a".into()], false).unwrap();
        assert!(dir.path().join("metadata.json").exists());
    }

    #[test]
    fn hand_edits_are_marked_and_a_model_write_clears_the_mark() {
        let dir = tempfile::tempdir().unwrap();
        write_tags(dir.path(), &["mine".into()], true).unwrap();
        assert!(tags_edited(dir.path()));
        write_tags(dir.path(), &["model".into()], false).unwrap();
        assert!(!tags_edited(dir.path()));
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
