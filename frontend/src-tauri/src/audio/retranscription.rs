// Retranscription module - allows re-processing stored audio with different settings

use crate::audio::decoder::decode_audio_file;
use crate::audio::vad::get_speech_chunks_with_progress;
use super::common::{create_labelled_transcript_segments, split_segment_at_silence, write_transcripts_json};
use super::constants::AUDIO_EXTENSIONS;
use super::enhance_status::{self, STATUS_DONE, STATUS_FAILED, STATUS_QUEUED, STATUS_RUNNING};
use crate::config::{DEFAULT_WHISPER_MODEL, DEFAULT_PARAKEET_MODEL};
use crate::notifications::commands::NotificationManagerState;
use crate::parakeet_engine::ParakeetEngine;
use crate::state::AppState;
use crate::whisper_engine::WhisperEngine;
use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Global flag to track if retranscription is in progress
static RETRANSCRIPTION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Global flag to signal cancellation
static RETRANSCRIPTION_CANCELLED: AtomicBool = AtomicBool::new(false);

/// spec 4.1: enhance jobs run one at a time, in arrival order. A meeting that
/// stops while another is still enhancing waits here instead of being refused.
static ENHANCE_QUEUE: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));

/// spec 4.1: meeting id of the job that currently holds the in-progress flag
static RETRANSCRIPTION_RUNNING_MEETING: Lazy<StdMutex<Option<String>>> =
    Lazy::new(|| StdMutex::new(None));

/// RAII guard for RETRANSCRIPTION_IN_PROGRESS flag
/// Ensures flag is cleared even if retranscription panics or returns early
struct RetranscriptionGuard;

impl RetranscriptionGuard {
    /// Create guard and set flag atomically
    fn acquire(meeting_id: &str) -> Result<Self, String> {
        if RETRANSCRIPTION_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Retranscription already in progress".to_string());
        }
        // spec 4.1: remember which meeting is running so cancel can target it
        *RETRANSCRIPTION_RUNNING_MEETING.lock().unwrap_or_else(|e| e.into_inner()) = Some(meeting_id.to_string());
        Ok(RetranscriptionGuard)
    }
}

impl Drop for RetranscriptionGuard {
    fn drop(&mut self) {
        // spec 4.1: clear the running meeting before releasing the flag
        *RETRANSCRIPTION_RUNNING_MEETING.lock().unwrap_or_else(|e| e.into_inner()) = None;
        RETRANSCRIPTION_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

/// VAD redemption time in milliseconds - bridges natural pauses in speech
/// Batch processing needs longer redemption (2000ms) than the live pipeline
/// (500ms) because the entire file is processed at once by VAD with no
/// latency requirement, and short redemption fragments speech at every
/// natural sentence/topic pause (500ms-2s)
const VAD_REDEMPTION_TIME_MS: u32 = 2000;

/// Progress update emitted during retranscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionProgress {
    pub meeting_id: String,
    pub stage: String, // "decoding", "transcribing", "saving"
    pub progress_percentage: u32,
    pub message: String,
}

/// Result of retranscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionResult {
    pub meeting_id: String,
    pub segments_count: usize,
    pub duration_seconds: f64,
    pub language: Option<String>,
}

/// Error during retranscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionError {
    pub meeting_id: String,
    pub error: String,
}

/// Check if retranscription is currently in progress
pub fn is_retranscription_in_progress() -> bool {
    RETRANSCRIPTION_IN_PROGRESS.load(Ordering::SeqCst)
}

/// spec 4.2: the meeting whose enhance job holds the in-progress flag, if any
pub fn running_meeting_id() -> Option<String> {
    RETRANSCRIPTION_RUNNING_MEETING.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Cancel the running job, but only if it belongs to `meeting_id` (or no id is given).
/// spec 4.1: a queued meeting cannot be cancelled in this phase; the error says so.
pub fn cancel_retranscription(meeting_id: Option<&str>) -> Result<(), String> {
    let running = RETRANSCRIPTION_RUNNING_MEETING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    match (meeting_id, running) {
        (_, None) => Err("No retranscription in progress".to_string()),
        (Some(requested), Some(ref current)) if requested != current => Err(
            "This meeting is queued behind another job. Only the running job can be cancelled.".to_string(),
        ),
        _ => {
            RETRANSCRIPTION_CANCELLED.store(true, Ordering::SeqCst);
            Ok(())
        }
    }
}

/// Start retranscription of a meeting's audio.
/// spec 4.1: waits for the enhance queue, records status in metadata.json at
/// each transition, and notifies when done.
pub async fn start_retranscription<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionResult> {
    let queued_folder = PathBuf::from(&meeting_folder_path);
    let provider_name = provider.clone().unwrap_or_else(|| "whisper".to_string());
    let model_name = model.clone().unwrap_or_else(|| "configured".to_string());

    set_enhance_status(&app, &meeting_id, &queued_folder, &provider_name, &model_name, STATUS_QUEUED, None);
    if ENHANCE_QUEUE.try_lock().is_err() {
        emit_progress(&app, &meeting_id, "queued", 0, "Waiting for the current enhance job to finish...");
    }
    let _queue_slot = ENHANCE_QUEUE.lock().await;

    // spec 4.2: the meeting may have been moved while this job waited; trust the index
    let meeting_folder_path = match app.try_state::<AppState>() {
        Some(state) => {
            let row: Option<(Option<String>,)> = sqlx::query_as("SELECT folder_path FROM meetings WHERE id = ?")
                .bind(&meeting_id)
                .fetch_optional(state.db_manager.pool())
                .await
                .unwrap_or(None);
            match row {
                Some((Some(p),)) if Path::new(&p).exists() => p,
                _ => meeting_folder_path,
            }
        }
        None => meeting_folder_path,
    };
    let folder = PathBuf::from(&meeting_folder_path);

    // Acquire guard - ensures the in-progress flag is cleared even on panic/early return
    let _guard = RetranscriptionGuard::acquire(&meeting_id).map_err(|e| anyhow!(e))?;

    // Reset cancellation flag
    RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);
    set_enhance_status(&app, &meeting_id, &folder, &provider_name, &model_name, STATUS_RUNNING, None);

    let use_parakeet = provider.as_deref() == Some("parakeet");
    let result = run_retranscription(app.clone(), meeting_id.clone(), meeting_folder_path, language, model, provider).await;

    // Unload the engine after the batch job (success, failure, or cancellation)
    super::common::unload_engine_after_batch(use_parakeet).await;

    match &result {
        Ok(res) => {
            set_enhance_status(&app, &meeting_id, &folder, &provider_name, &model_name, STATUS_DONE, None);
            // spec 4.7: hand the finished transcript to the user's hook, if configured
            if res.segments_count > 0 {
                match super::recording_preferences::load_recording_preferences(&app).await {
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
                    Err(e) => warn!("Post-enhance hook skipped for {}: could not load preferences: {}", meeting_id, e),
                }
            }
            notify_enhance_complete(&app).await;
            let _ = app.emit(
                "retranscription-complete",
                serde_json::json!({
                    "meeting_id": res.meeting_id,
                    "segments_count": res.segments_count,
                    "duration_seconds": res.duration_seconds,
                    "language": res.language
                }),
            );
        }
        Err(e) => {
            set_enhance_status(&app, &meeting_id, &folder, &provider_name, &model_name, STATUS_FAILED, Some(&e.to_string()));
            let _ = app.emit(
                "retranscription-error",
                RetranscriptionError {
                    meeting_id: meeting_id.clone(),
                    error: e.to_string(),
                },
            );
        }
    }

    result
}

/// Write the status to metadata.json and tell the UI. Failures to write are
/// logged, never fatal: the transcript matters more than the badge.
fn set_enhance_status<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    folder: &Path,
    provider: &str,
    model: &str,
    status: &str,
    error: Option<&str>,
) {
    if let Err(e) = enhance_status::write_enhance_status(folder, provider, model, status, error) {
        warn!("Could not write enhance status '{}' for {}: {}", status, meeting_id, e);
    }
    let _ = app.emit(
        "enhance-status-changed",
        serde_json::json!({ "meeting_id": meeting_id, "status": status }),
    );
}

/// OS notification through the existing NotificationManager. Respects the
/// user's `show_transcription_complete` preference because the manager checks it.
async fn notify_enhance_complete<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<NotificationManagerState<R>>() {
        let guard = state.read().await;
        if let Some(manager) = guard.as_ref() {
            if let Err(e) = manager.show_transcription_complete(None).await {
                warn!("Enhance complete notification failed: {}", e);
            }
        }
    }
}

/// spec 4.4: cut a VAD segment at absolute millisecond positions (16 kHz samples).
/// Boundaries at or outside the segment edges are ignored.
fn split_at_boundaries(segment: &crate::audio::vad::SpeechSegment, boundaries_ms: &[f64]) -> Vec<crate::audio::vad::SpeechSegment> {
    const SR: f64 = 16000.0;
    let mut cuts: Vec<f64> = boundaries_ms
        .iter()
        .copied()
        .filter(|b| *b > segment.start_timestamp_ms && *b < segment.end_timestamp_ms)
        .collect();
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    cuts.dedup();
    if cuts.is_empty() {
        return vec![segment.clone()];
    }
    let mut parts = Vec::with_capacity(cuts.len() + 1);
    let mut prev_ms = segment.start_timestamp_ms;
    let mut prev_idx = 0usize;
    for cut in cuts {
        let idx = (((cut - segment.start_timestamp_ms) / 1000.0) * SR).round() as usize;
        let idx = idx.min(segment.samples.len()).max(prev_idx);
        if idx > prev_idx {
            parts.push(crate::audio::vad::SpeechSegment {
                samples: segment.samples[prev_idx..idx].to_vec(),
                start_timestamp_ms: prev_ms,
                end_timestamp_ms: cut,
                confidence: segment.confidence,
            });
        }
        prev_ms = cut;
        prev_idx = idx;
    }
    // The last piece ends at the real sample count, not at a rounded millisecond position
    if segment.samples.len() > prev_idx {
        parts.push(crate::audio::vad::SpeechSegment {
            samples: segment.samples[prev_idx..].to_vec(),
            start_timestamp_ms: prev_ms,
            end_timestamp_ms: segment.end_timestamp_ms,
            confidence: segment.confidence,
        });
    }
    parts
}

/// Find audio file in meeting folder
/// Tries common names first, then scans for any file with an audio extension
fn find_audio_file(folder: &Path) -> Result<PathBuf> {
    let candidates = [
        "audio.mp4", "audio.m4a", "audio.wav", "audio.mp3",
        "audio.flac", "audio.ogg", "recording.mp4",
        "audio.mkv", "audio.webm", "audio.wma",
    ];

    for name in candidates {
        let path = folder.join(name);
        if path.exists() {
            return Ok(path);
        }
    }

    // Fallback: scan folder for any file with an audio extension
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
                    return Ok(path);
                }
            }
        }
    }

    Err(anyhow!("No audio file found in: {}", folder.display()))
}

/// Internal function to run retranscription
async fn run_retranscription<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionResult> {
    let folder_path = PathBuf::from(&meeting_folder_path);
    let audio_path = find_audio_file(&folder_path)?;

    // Determine which provider to use (default to whisper)
    let use_parakeet = provider.as_deref() == Some("parakeet");

    info!(
        "Starting retranscription for meeting {} with language {:?}, model {:?}, provider {:?}",
        meeting_id, language, model, provider
    );

    // Emit progress: decoding
    emit_progress(&app, &meeting_id, "decoding", 5, "Decoding audio file...");

    // Check for cancellation
    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    // Decode the audio file (CPU-intensive, run in blocking task)
    let path_for_decode = audio_path.clone();
    let decoded = tokio::task::spawn_blocking(move || {
        decode_audio_file(&path_for_decode)
    })
    .await
    .map_err(|e| anyhow!("Decode task panicked: {}", e))??;
    let duration_seconds = decoded.duration_seconds;

    info!(
        "Decoded audio: {:.2}s, {}Hz, {} channels",
        duration_seconds, decoded.sample_rate, decoded.channels
    );

    emit_progress(&app, &meeting_id, "decoding", 15, "Converting audio format...");

    // Check for cancellation
    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    // Convert to 16kHz mono format (CPU-intensive, run in blocking task)
    let audio_samples = tokio::task::spawn_blocking(move || {
        decoded.to_whisper_format()
    })
    .await
    .map_err(|e| anyhow!("Resample task panicked: {}", e))?;
    info!("Converted to 16kHz mono format: {} samples", audio_samples.len());

    emit_progress(&app, &meeting_id, "vad", 20, "Detecting speech segments...");

    // Check for cancellation
    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    // Use VAD to find natural speech boundaries (same approach as live transcription)
    // IMPORTANT: Run VAD in a blocking task to avoid blocking the async runtime
    // For large files (35+ minutes), VAD processing can take several minutes
    let app_for_vad = app.clone();
    let meeting_id_for_vad = meeting_id.clone();

    let speech_segments = tokio::task::spawn_blocking(move || {
        get_speech_chunks_with_progress(
            &audio_samples,
            VAD_REDEMPTION_TIME_MS,
            |vad_progress, segments_found| {
                // Map VAD progress (0-100) to overall progress (20-25)
                let overall_progress = 20 + (vad_progress as f32 * 0.05) as u32;
                emit_progress(
                    &app_for_vad,
                    &meeting_id_for_vad,
                    "vad",
                    overall_progress,
                    &format!("Detecting speech segments... {}% ({} found)", vad_progress, segments_found),
                );

                // Return false to cancel if cancellation requested
                !RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst)
            },
        )
    })
    .await
    .map_err(|e| anyhow!("VAD task panicked: {}", e))?
    .map_err(|e| anyhow!("VAD processing failed: {}", e))?;

    let total_segments = speech_segments.len();
    info!("VAD detected {} speech segments (redemption_time={}ms)", total_segments, VAD_REDEMPTION_TIME_MS);

    // Diagnostic: log segment duration distribution
    if !speech_segments.is_empty() {
        let durations_ms: Vec<f64> = speech_segments.iter()
            .map(|s| s.end_timestamp_ms - s.start_timestamp_ms)
            .collect();
        let total_speech_ms: f64 = durations_ms.iter().sum();
        let avg_duration = total_speech_ms / durations_ms.len() as f64;
        let min_duration = durations_ms.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_duration = durations_ms.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        info!(
            "VAD segment stats: avg={:.0}ms, min={:.0}ms, max={:.0}ms, total_speech={:.1}s/{:.1}s ({:.0}%)",
            avg_duration, min_duration, max_duration,
            total_speech_ms / 1000.0, duration_seconds,
            (total_speech_ms / 1000.0 / duration_seconds) * 100.0
        );
        // Log first 10 segments for detailed inspection
        for (i, seg) in speech_segments.iter().take(10).enumerate() {
            let dur = seg.end_timestamp_ms - seg.start_timestamp_ms;
            debug!("  Segment {}: {:.0}ms-{:.0}ms ({:.0}ms, {} samples)",
                i, seg.start_timestamp_ms, seg.end_timestamp_ms, dur, seg.samples.len());
        }
        if total_segments > 10 {
            debug!("  ... and {} more segments", total_segments - 10);
        }
    }

    if total_segments == 0 {
        // spec 4.1: silence is not a failure. Leave the live transcript and files as they are.
        warn!("No speech detected in audio; nothing to enhance for meeting {}", meeting_id);
        emit_progress(&app, &meeting_id, "complete", 100, "No speech detected; nothing to enhance");
        return Ok(RetranscriptionResult {
            meeting_id,
            segments_count: 0,
            duration_seconds,
            language,
        });
    }

    emit_progress(&app, &meeting_id, "transcribing", 25, "Loading transcription engine...");

    // Initialize the appropriate engine once (not per-segment)
    let whisper_engine = if !use_parakeet {
        Some(get_or_init_whisper(&app, model.as_deref()).await?)
    } else {
        None
    };
    let parakeet_engine = if use_parakeet {
        Some(get_or_init_parakeet(&app, model.as_deref()).await?)
    } else {
        None
    };

    // Split very long segments at silence boundaries for better transcription quality.
    // Hard cuts at arbitrary sample positions lose words at boundaries. Instead, scan
    // for the lowest-energy window near the target split point and cut there.
    const MAX_SEGMENT_SAMPLES: usize = 25 * 16000; // 25 seconds at 16kHz

    let mut processable_segments: Vec<crate::audio::vad::SpeechSegment> = Vec::new();
    for segment in &speech_segments {
        if segment.samples.len() > MAX_SEGMENT_SAMPLES {
            debug!(
                "Splitting large segment ({:.0}ms, {} samples) at silence boundaries",
                segment.end_timestamp_ms - segment.start_timestamp_ms,
                segment.samples.len()
            );

            let sub_segments = split_segment_at_silence(segment, MAX_SEGMENT_SAMPLES);
            debug!("Split into {} sub-segments", sub_segments.len());
            processable_segments.extend(sub_segments);
        } else {
            processable_segments.push(segment.clone());
        }
    }

    // spec 4.4: split at speaker turns and remember who spoke, when the recording has an energy timeline
    let timeline = super::speaker_energy::EnergyTimeline::load(&folder_path);
    let refs = timeline.as_ref().map(|t| t.references());
    let mut labelled_segments: Vec<(crate::audio::vad::SpeechSegment, Option<String>)> = Vec::new();
    for segment in processable_segments {
        match (&timeline, refs) {
            (Some(t), Some(r)) => {
                let cuts = t.turn_boundaries(segment.start_timestamp_ms, segment.end_timestamp_ms, r, 1000);
                for part in split_at_boundaries(&segment, &cuts) {
                    let label = t.label_span(part.start_timestamp_ms, part.end_timestamp_ms, r).map(|s| s.as_str().to_string());
                    labelled_segments.push((part, label));
                }
            }
            _ => labelled_segments.push((segment, None)),
        }
    }
    let processable_count = labelled_segments.len();
    info!("Processing {} segments (after silence and speaker-turn splitting)", processable_count);

    // Process each speech segment with progress updates
    let mut all_transcripts: Vec<(String, f64, f64, Option<String>)> = Vec::new(); // (text, start_ms, end_ms, speaker)
    let mut total_confidence = 0.0f32;

    for (i, (segment, speaker)) in labelled_segments.iter().enumerate() {
        // Check for cancellation before each segment
        if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
            return Err(anyhow!("Retranscription cancelled"));
        }

        // Calculate progress (25% to 80% range for transcription)
        let progress = 25 + ((i as f32 / processable_count as f32) * 55.0) as u32;
        let segment_duration_sec = (segment.end_timestamp_ms - segment.start_timestamp_ms) / 1000.0;
        emit_progress(
            &app,
            &meeting_id,
            "transcribing",
            progress,
            &format!(
                "Transcribing segment {} of {} ({:.1}s)...",
                i + 1,
                processable_count,
                segment_duration_sec
            ),
        );

        // Skip very short segments (< 100ms of audio = 1600 samples at 16kHz)
        if segment.samples.len() < 1600 {
            debug!("Skipping short segment {} with {} samples", i, segment.samples.len());
            continue;
        }

        // Transcribe this segment
        let (text, conf) = if use_parakeet {
            let engine = parakeet_engine.as_ref().unwrap();
            let text = engine
                .transcribe_audio(segment.samples.clone())
                .await
                .map_err(|e| anyhow!("Parakeet transcription failed on segment {}: {}", i, e))?;
            (text, 0.9f32)
        } else {
            let engine = whisper_engine.as_ref().unwrap();
            let (text, conf, _) = engine
                .transcribe_audio_with_confidence(segment.samples.clone(), language.clone())
                .await
                .map_err(|e| anyhow!("Whisper transcription failed on segment {}: {}", i, e))?;
            (text, conf)
        };

        // Skip empty transcripts
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            debug!(
                "Segment {}/{}: {:.1}s, conf={:.2}, text='{}'",
                i + 1, processable_count, segment_duration_sec, conf,
                if trimmed.len() > 80 { let mut end = 80; while !trimmed.is_char_boundary(end) { end -= 1; } &trimmed[..end] } else { trimmed }
            );
            all_transcripts.push((text, segment.start_timestamp_ms, segment.end_timestamp_ms, speaker.clone()));
            total_confidence += conf;
        } else {
            debug!("Segment {}/{}: {:.1}s — empty transcription", i + 1, processable_count, segment_duration_sec);
        }
    }

    let transcribed_count = all_transcripts.len();
    let avg_confidence = if transcribed_count > 0 {
        total_confidence / transcribed_count as f32
    } else {
        0.0
    };

    info!(
        "Transcription complete: {} segments transcribed out of {}, avg confidence: {:.2}",
        transcribed_count, processable_count, avg_confidence
    );

    // Check for cancellation
    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    emit_progress(&app, &meeting_id, "saving", 80, "Saving transcripts...");

    // Create transcript segments with proper timestamps from VAD
    let segments = create_labelled_transcript_segments(&all_transcripts);

    // Save to database
    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow!("App state not available"))?;

    // Wrap delete+insert+update in a transaction to prevent data loss
    let pool = app_state.db_manager.pool();
    let mut conn = pool.acquire().await.map_err(|e| anyhow!("DB error: {}", e))?;
    let mut tx = sqlx::Connection::begin(&mut *conn)
        .await
        .map_err(|e| anyhow!("Failed to start transaction: {}", e))?;

    sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
        .bind(&meeting_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow!("Failed to delete existing transcripts: {}", e))?;

    for segment in &segments {
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&segment.id)
        .bind(&meeting_id)
        .bind(&segment.text)
        .bind(&segment.timestamp)
        .bind(segment.audio_start_time)
        .bind(segment.audio_end_time)
        .bind(segment.duration)
        .bind(&segment.speaker) // spec 4.4
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow!("Failed to insert transcript: {}", e))?;
    }

    tx.commit().await
        .map_err(|e| anyhow!("Failed to commit transaction: {}", e))?;

    info!(
        "Updated {} transcripts for meeting {} in transaction",
        segments.len(),
        meeting_id
    );

    // Write updated transcripts.json and metadata.json to the meeting folder
    emit_progress(&app, &meeting_id, "saving", 90, "Writing transcript files...");

    if let Err(e) = write_transcripts_json(&folder_path, &segments) {
        warn!("Failed to write transcripts.json: {}", e);
    }

    // spec 4.3: rewrite transcript.md with the enhanced segments
    {
        let meta_json: serde_json::Value = std::fs::read_to_string(folder_path.join("metadata.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let title = meta_json.get("title").and_then(|v| v.as_str())
            .or_else(|| meta_json.get("meeting_name").and_then(|v| v.as_str()))
            .unwrap_or("");
        let project = meta_json.get("project").and_then(|v| v.as_str()).unwrap_or(super::library::INBOX);
        let created_at = meta_json.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
        let started_at = super::transcript_md::local_iso(created_at);
        let model_label = format!("{} {}", if use_parakeet { "parakeet" } else { "whisper" }, model.as_deref().unwrap_or("configured"));
        // spec 4.4: names only when at least one segment carries a label
        let speaker_names = if segments.iter().any(|s| s.speaker.is_some()) {
            Some((
                meta_json["speakers"]["me"].as_str().unwrap_or("Me"),
                meta_json["speakers"]["them"].as_str().unwrap_or("Others"),
            ))
        } else {
            None
        };
        let md_meta = super::transcript_md::TranscriptMdMeta {
            title,
            project,
            meeting_id: &meeting_id,
            started_at: &started_at,
            duration_seconds: Some(duration_seconds),
            quality: "enhanced",
            model: &model_label,
            speaker_names, // spec 4.4
        };
        let md_segments: Vec<super::transcript_md::MdSegment> = segments
            .iter()
            .map(|s| super::transcript_md::MdSegment {
                start: s.audio_start_time.unwrap_or(0.0),
                speaker: s.speaker.clone(), // spec 4.4
                text: s.text.clone(),
            })
            .collect();
        if let Err(e) = super::transcript_md::write_transcript_md(&folder_path, &md_meta, &md_segments) {
            warn!("Failed to write transcript.md after enhance: {}", e);
        }
    }

    // Find audio filename for metadata
    let audio_filename = audio_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.mp4")
        .to_string();

    if let Err(e) = write_retranscription_metadata(
        &folder_path,
        &meeting_id,
        duration_seconds,
        &audio_filename,
    ) {
        warn!("Failed to update metadata.json: {}", e);
    }

    emit_progress(&app, &meeting_id, "complete", 100, "Retranscription complete");

    Ok(RetranscriptionResult {
        meeting_id,
        segments_count: segments.len(),
        duration_seconds,
        language,
    })
}

/// Emit progress event
fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    stage: &str,
    progress: u32,
    message: &str,
) {
    let _ = app.emit(
        "retranscription-progress",
        RetranscriptionProgress {
            meeting_id: meeting_id.to_string(),
            stage: stage.to_string(),
            progress_percentage: progress,
            message: message.to_string(),
        },
    );
}

/// Get or initialize the Whisper engine, auto-loading the model if needed
/// If `requested_model` is provided, ensures that specific model is loaded
async fn get_or_init_whisper<R: Runtime>(
    app: &AppHandle<R>,
    requested_model: Option<&str>,
) -> Result<Arc<WhisperEngine>> {
    use crate::whisper_engine::commands::WHISPER_ENGINE;

    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().cloned()
    };

    match engine {
        Some(e) => {
            // Determine which model to use
            let target_model = match requested_model {
                Some(model) => model.to_string(),
                None => get_configured_whisper_model(app).await?,
            };

            // Check if the correct model is already loaded
            let current_model = e.get_current_model().await;
            let needs_load = match &current_model {
                Some(loaded) => loaded != &target_model,
                None => true,
            };

            if needs_load {
                info!(
                    "Loading Whisper model '{}' (current: {:?})",
                    target_model, current_model
                );

                // Discover available models first (populates the internal cache)
                info!("Discovering available Whisper models...");
                if let Err(discover_err) = e.discover_models().await {
                    warn!("Error during model discovery (continuing anyway): {}", discover_err);
                }

                match e.load_model(&target_model).await {
                    Ok(_) => {
                        info!("Whisper model '{}' loaded successfully", target_model);
                        Ok(e)
                    }
                    Err(load_err) => {
                        error!("Failed to load Whisper model '{}': {}", target_model, load_err);
                        Err(anyhow!("Failed to load Whisper model '{}': {}", target_model, load_err))
                    }
                }
            } else {
                info!("Whisper model '{}' already loaded", target_model);
                Ok(e)
            }
        }
        None => Err(anyhow!("Whisper engine not initialized")),
    }
}

/// Get the configured Whisper model name from the database
async fn get_configured_whisper_model<R: Runtime>(app: &AppHandle<R>) -> Result<String> {
    debug!("Getting configured Whisper model from database...");

    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| {
            error!("App state not available");
            anyhow!("App state not available")
        })?;

    debug!("Querying transcript_settings table...");

    // Query the transcript settings from the database - get both provider and model
    let result: Option<(String, String)> = sqlx::query_as(
        "SELECT provider, model FROM transcript_settings WHERE id = '1'"
    )
    .fetch_optional(app_state.db_manager.pool())
    .await
    .map_err(|e| {
        error!("Failed to query transcript config: {}", e);
        anyhow!("Failed to query transcript config: {}", e)
    })?;

    match result {
        Some((provider, model)) => {
            info!("Found transcript config: provider={}, model={}", provider, model);

            // Check if provider is Whisper-based
            if provider == "localWhisper" || provider == "whisper" {
                Ok(model)
            } else {
                error!("Retranscription requires Whisper provider, but configured provider is: {}", provider);
                Err(anyhow!("Retranscription requires Whisper. Current provider '{}' does not support retranscription with language selection.", provider))
            }
        },
        None => {
            // Default to configured Whisper model if no config exists
            warn!("No transcript config found, using default model '{}'", DEFAULT_WHISPER_MODEL);
            Ok(DEFAULT_WHISPER_MODEL.to_string())
        }
    }
}

/// Get or initialize the Parakeet engine, auto-loading the model if needed
async fn get_or_init_parakeet<R: Runtime>(
    app: &AppHandle<R>,
    requested_model: Option<&str>,
) -> Result<Arc<ParakeetEngine>> {
    use crate::parakeet_engine::commands::PARAKEET_ENGINE;

    let engine = {
        let guard = PARAKEET_ENGINE.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().cloned()
    };

    match engine {
        Some(e) => {
            // Determine which model to use
            let target_model = match requested_model {
                Some(model) => model.to_string(),
                None => get_configured_parakeet_model(app).await?,
            };

            // Check if the correct model is already loaded
            let current_model = e.get_current_model().await;
            let needs_load = match &current_model {
                Some(loaded) => loaded != &target_model,
                None => true,
            };

            if needs_load {
                info!(
                    "Loading Parakeet model '{}' (current: {:?})",
                    target_model, current_model
                );

                // Discover available models first
                info!("Discovering available Parakeet models...");
                if let Err(discover_err) = e.discover_models().await {
                    warn!("Error during Parakeet model discovery (continuing anyway): {}", discover_err);
                }

                match e.load_model(&target_model).await {
                    Ok(_) => {
                        info!("Parakeet model '{}' loaded successfully", target_model);
                        Ok(e)
                    }
                    Err(load_err) => {
                        error!("Failed to load Parakeet model '{}': {}", target_model, load_err);
                        Err(anyhow!("Failed to load Parakeet model '{}': {}", target_model, load_err))
                    }
                }
            } else {
                info!("Parakeet model '{}' already loaded", target_model);
                Ok(e)
            }
        }
        None => Err(anyhow!("Parakeet engine not initialized")),
    }
}

/// Get the configured Parakeet model name from the database
async fn get_configured_parakeet_model<R: Runtime>(app: &AppHandle<R>) -> Result<String> {
    debug!("Getting configured Parakeet model from database...");

    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| {
            error!("App state not available");
            anyhow!("App state not available")
        })?;

    // Query the transcript settings from the database
    let result: Option<(String, String)> = sqlx::query_as(
        "SELECT provider, model FROM transcript_settings WHERE id = '1'"
    )
    .fetch_optional(app_state.db_manager.pool())
    .await
    .map_err(|e| {
        error!("Failed to query transcript config: {}", e);
        anyhow!("Failed to query transcript config: {}", e)
    })?;

    match result {
        Some((provider, model)) => {
            info!("Found transcript config: provider={}, model={}", provider, model);

            if provider == "parakeet" {
                Ok(model)
            } else {
                // Default to configured Parakeet model
                warn!("Configured provider is not Parakeet, using default model");
                Ok(DEFAULT_PARAKEET_MODEL.to_string())
            }
        },
        None => {
            // Default to configured Parakeet model if no config exists
            warn!("No transcript config found, using default Parakeet model");
            Ok(DEFAULT_PARAKEET_MODEL.to_string())
        }
    }
}

/// Write or update metadata.json for retranscription (preserves existing fields, adds retranscribed_at)
fn write_retranscription_metadata(
    folder: &Path,
    meeting_id: &str,
    duration_seconds: f64,
    audio_filename: &str,
) -> Result<()> {
    let metadata_path = folder.join("metadata.json");
    let temp_path = folder.join(".metadata.json.tmp");
    let now = chrono::Utc::now().to_rfc3339();

    // Try to read existing metadata and update it
    let json = if metadata_path.exists() {
        let existing = std::fs::read_to_string(&metadata_path)?;
        let mut value: serde_json::Value = serde_json::from_str(&existing)?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("duration_seconds".to_string(), serde_json::json!(duration_seconds));
            obj.insert("retranscribed_at".to_string(), serde_json::json!(now));
            obj.insert("status".to_string(), serde_json::json!("completed"));
            obj.insert("transcript_file".to_string(), serde_json::json!("transcripts.json"));
            obj.insert("transcript_md".to_string(), serde_json::json!("transcript.md")); // spec 4.3
            obj.remove("detected_summary_language");
        }
        value
    } else {
        serde_json::json!({
            "version": "1.0",
            "meeting_id": meeting_id,
            "created_at": now,
            "completed_at": now,
            "retranscribed_at": now,
            "duration_seconds": duration_seconds,
            "audio_file": audio_filename,
            "transcript_file": "transcripts.json",
            "status": "completed",
            "source": "retranscription"
        })
    };

    let json_string = serde_json::to_string_pretty(&json)?;
    std::fs::write(&temp_path, &json_string)?;
    std::fs::rename(&temp_path, &metadata_path)?;

    info!("Wrote metadata.json to {}", metadata_path.display());
    Ok(())
}

// Tauri commands

/// Response when retranscription is started
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionStarted {
    pub meeting_id: String,
    pub message: String,
}

// Start retranscription (Beta gated using configContext.betaFeatures)
#[tauri::command]
pub async fn start_retranscription_command<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionStarted, String> {

    // spec 4.1: a job that arrives while another runs waits in ENHANCE_QUEUE.

    // Clone values for the spawned task
    let meeting_id_clone = meeting_id.clone();
    let meeting_id_for_sync = meeting_id.clone();

    // Spawn the retranscription in a background task
    tauri::async_runtime::spawn(async move {
        let result = start_retranscription(
            app.clone(),
            meeting_id_clone,
            meeting_folder_path,
            language,
            model,
            provider,
        )
        .await;

        // Errors are already emitted as events in start_retranscription
        // so we just log here for debugging
        if let Err(e) = result {
            error!("Retranscription failed: {}", e);
        }

        // spec 4.2: a title set while the job ran was written in place; now that the folder is free,
        // sync the folder slug and metadata to the database title.
        if let Some(state) = app.try_state::<AppState>() {
            let root = crate::audio::get_default_recordings_folder();
            if let Err(e) = crate::audio::library::assign(state.db_manager.pool(), &root, &meeting_id_for_sync, None, None).await {
                if !e.starts_with("Folder not found on disk") {
                    warn!("Post-enhance folder sync skipped for {}: {}", meeting_id_for_sync, e);
                }
            }
        }
    });

    Ok(RetranscriptionStarted {
        meeting_id,
        message: "Retranscription started".to_string(),
    })
}

#[tauri::command]
pub async fn cancel_retranscription_command(meeting_id: Option<String>) -> Result<(), String> {
    // spec 4.1: cancel only the running job's meeting
    cancel_retranscription(meeting_id.as_deref())
}

#[tauri::command]
pub async fn is_retranscription_in_progress_command() -> bool {
    is_retranscription_in_progress()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TranscriptSegment;
    use crate::audio::common::create_labelled_transcript_segments;

    fn create_transcript_segments(transcripts: &[(String, f64, f64)]) -> Vec<TranscriptSegment> {
        let labelled: Vec<_> = transcripts
            .iter()
            .map(|(t, s, e)| (t.clone(), *s, *e, None))
            .collect();
        create_labelled_transcript_segments(&labelled)
    }

    // spec 4.4
    #[test]
    fn split_at_boundaries_cuts_samples_and_timestamps() {
        let seg = crate::audio::vad::SpeechSegment {
            samples: vec![0.1; 16000 * 3], // 3 s at 16 kHz
            start_timestamp_ms: 10_000.0,
            end_timestamp_ms: 13_000.0,
            confidence: 0.9,
        };
        let parts = split_at_boundaries(&seg, &[11_000.0, 12_000.0]);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].samples.len(), 16000);
        assert_eq!(parts[1].start_timestamp_ms, 11_000.0);
        assert_eq!(parts[2].end_timestamp_ms, 13_000.0);
        let uneven = split_at_boundaries(&seg, &[11_000.04, 12_000.0]);
        assert_eq!(uneven.iter().map(|p| p.samples.len()).sum::<usize>(), 16000 * 3, "no sample is lost to rounding");
        assert_eq!(uneven[0].samples.len(), 16001);
        assert_eq!(split_at_boundaries(&seg, &[]).len(), 1);
        assert_eq!(split_at_boundaries(&seg, &[10_000.0, 13_000.0, 500.0]).len(), 1, "boundaries at the edges or outside are ignored");
    }

    /// spec 4.1: serialises tests that touch the shared cancel and running-meeting state
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    #[test]
    fn test_create_transcript_segments_empty() {
        let transcripts: Vec<(String, f64, f64)> = vec![];
        let segments = create_transcript_segments(&transcripts);
        assert!(segments.is_empty());
    }

    #[test]
    fn test_create_transcript_segments_single() {
        let transcripts = vec![
            ("Hello world".to_string(), 0.0, 1500.0), // 0-1.5 seconds
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Hello world");
        assert_eq!(segments[0].audio_start_time, Some(0.0));
        assert_eq!(segments[0].audio_end_time, Some(1.5));
        assert_eq!(segments[0].duration, Some(1.5));
    }

    #[test]
    fn test_create_transcript_segments_multiple() {
        let transcripts = vec![
            ("First segment".to_string(), 0.0, 2000.0),      // 0-2 seconds
            ("Second segment".to_string(), 3000.0, 5000.0),  // 3-5 seconds
            ("Third segment".to_string(), 6500.0, 8000.0),   // 6.5-8 seconds
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 3);

        // First segment
        assert_eq!(segments[0].text, "First segment");
        assert_eq!(segments[0].audio_start_time, Some(0.0));
        assert_eq!(segments[0].audio_end_time, Some(2.0));
        assert_eq!(segments[0].duration, Some(2.0));

        // Second segment
        assert_eq!(segments[1].text, "Second segment");
        assert_eq!(segments[1].audio_start_time, Some(3.0));
        assert_eq!(segments[1].audio_end_time, Some(5.0));
        assert_eq!(segments[1].duration, Some(2.0));

        // Third segment
        assert_eq!(segments[2].text, "Third segment");
        assert_eq!(segments[2].audio_start_time, Some(6.5));
        assert_eq!(segments[2].audio_end_time, Some(8.0));
        assert_eq!(segments[2].duration, Some(1.5));
    }

    #[test]
    fn test_create_transcript_segments_trims_whitespace() {
        let transcripts = vec![
            ("  Hello with spaces  ".to_string(), 0.0, 1000.0),
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Hello with spaces");
    }

    #[test]
    fn test_create_transcript_segments_generates_unique_ids() {
        let transcripts = vec![
            ("Segment one".to_string(), 0.0, 1000.0),
            ("Segment two".to_string(), 1000.0, 2000.0),
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 2);
        assert_ne!(segments[0].id, segments[1].id);
        assert!(segments[0].id.starts_with("transcript-"));
        assert!(segments[1].id.starts_with("transcript-"));
    }

    #[test]
    fn test_cancellation_flag() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Reset flag to known state
        RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);
        RETRANSCRIPTION_IN_PROGRESS.store(false, Ordering::SeqCst);

        assert!(!is_retranscription_in_progress());

        // spec 4.1: cancel needs a running job, so simulate one
        *RETRANSCRIPTION_RUNNING_MEETING.lock().unwrap() = Some("x".to_string());

        // Test cancellation
        assert!(cancel_retranscription(None).is_ok());
        assert!(RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst));

        // Reset for other tests
        RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);
        *RETRANSCRIPTION_RUNNING_MEETING.lock().unwrap() = None;
    }

    // spec 4.1
    #[test]
    fn cancel_only_targets_the_running_meeting() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);
        *RETRANSCRIPTION_RUNNING_MEETING.lock().unwrap() = None;
        assert!(cancel_retranscription(Some("a")).is_err(), "nothing running");

        *RETRANSCRIPTION_RUNNING_MEETING.lock().unwrap() = Some("a".to_string());
        assert!(cancel_retranscription(Some("b")).is_err(), "b is not the running job");
        assert!(!RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst));

        assert!(cancel_retranscription(Some("a")).is_ok());
        assert!(RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst));

        RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);
        *RETRANSCRIPTION_RUNNING_MEETING.lock().unwrap() = None;
    }

    #[test]
    fn test_vad_redemption_time_constant() {
        // Batch processing uses 2000ms to bridge natural pauses in full-file VAD
        assert_eq!(VAD_REDEMPTION_TIME_MS, 2000);
    }

    #[test]
    fn test_find_audio_file_common_candidates() {
        let dir = tempfile::tempdir().unwrap();

        // No audio file → error
        assert!(find_audio_file(dir.path()).is_err());

        // Create audio.mp4 — should be found first
        std::fs::write(dir.path().join("audio.mp4"), b"fake").unwrap();
        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "audio.mp4");
    }

    #[test]
    fn test_find_audio_file_non_mp4_extensions() {
        let dir = tempfile::tempdir().unwrap();

        // Create audio.wav (imported as .wav, not .mp4)
        std::fs::write(dir.path().join("audio.wav"), b"fake").unwrap();
        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "audio.wav");
    }

    #[test]
    fn test_find_audio_file_fallback_scan() {
        let dir = tempfile::tempdir().unwrap();

        // Create a file with an audio extension but non-standard name
        std::fs::write(dir.path().join("my_recording.flac"), b"fake").unwrap();
        // Also add a non-audio file that should be ignored
        std::fs::write(dir.path().join("notes.txt"), b"text").unwrap();

        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "my_recording.flac");
    }

    #[test]
    fn test_find_audio_file_priority_order() {
        let dir = tempfile::tempdir().unwrap();

        // Create both audio.m4a and audio.mp4 — mp4 should win (listed first in candidates)
        std::fs::write(dir.path().join("audio.m4a"), b"fake").unwrap();
        std::fs::write(dir.path().join("audio.mp4"), b"fake").unwrap();
        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "audio.mp4");
    }

    #[test]
    fn test_find_audio_file_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        let result = find_audio_file(dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No audio file found"));
    }

    #[test]
    fn test_find_audio_file_nonexistent_folder() {
        let result = find_audio_file(Path::new("/nonexistent/path/12345"));
        assert!(result.is_err());
    }

    #[test]
    fn test_audio_extensions_constant() {
        // Verify all expected formats are covered
        assert!(AUDIO_EXTENSIONS.contains(&"mp4"));
        assert!(AUDIO_EXTENSIONS.contains(&"m4a"));
        assert!(AUDIO_EXTENSIONS.contains(&"wav"));
        assert!(AUDIO_EXTENSIONS.contains(&"mp3"));
        assert!(AUDIO_EXTENSIONS.contains(&"flac"));
        assert!(AUDIO_EXTENSIONS.contains(&"ogg"));
        assert!(AUDIO_EXTENSIONS.contains(&"aac"));
        // FFmpeg-backed formats
        assert!(AUDIO_EXTENSIONS.contains(&"mkv"));
        assert!(AUDIO_EXTENSIONS.contains(&"webm"));
        assert!(AUDIO_EXTENSIONS.contains(&"wma"));
        // Non-audio formats
        assert!(!AUDIO_EXTENSIONS.contains(&"txt"));
        assert!(!AUDIO_EXTENSIONS.contains(&"pdf"));
    }

    #[test]
    fn retranscription_metadata_replaces_stale_duration_without_losing_existing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let metadata_path = dir.path().join("metadata.json");
        std::fs::write(
            &metadata_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "meeting_id": "existing-meeting",
                "duration_seconds": 2523.2,
                "audio_file": "recording.m4a",
                "summary_language": "fr",
                "detected_summary_language": "en",
                "custom_field": "preserve me"
            }))
            .unwrap(),
        )
        .unwrap();

        write_retranscription_metadata(
            dir.path(),
            "ignored-for-existing-metadata",
            5046.4,
            "recording.m4a",
        )
        .unwrap();

        let metadata: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(metadata_path).unwrap()).unwrap();
        assert_eq!(metadata["duration_seconds"], 5046.4);
        assert_eq!(metadata["meeting_id"], "existing-meeting");
        assert_eq!(metadata["audio_file"], "recording.m4a");
        assert_eq!(metadata["summary_language"], "fr");
        assert_eq!(metadata["custom_field"], "preserve me");
        assert!(metadata.get("detected_summary_language").is_none());
    }

    #[tokio::test]
    async fn enhance_queue_serializes_jobs() {
        let first = ENHANCE_QUEUE.lock().await;
        assert!(
            ENHANCE_QUEUE.try_lock().is_err(),
            "a second job must wait while the first holds the slot"
        );
        drop(first);
        assert!(ENHANCE_QUEUE.try_lock().is_ok());
    }
}
