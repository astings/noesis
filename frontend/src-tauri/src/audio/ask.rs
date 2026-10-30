//! Live meeting ask (spec 2026-10-29-live-meeting-ask-design.md): answer a question about the
//! recording in progress, or a finished meeting, from its transcript. Pure helpers here, the
//! Tauri command below them. Nothing is written to disk and nothing goes through the job queue.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

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
