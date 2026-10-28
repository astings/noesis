//! spec phase 9 section 3: delete audio once the enhanced transcript is safely on disk.

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

pub const ENHANCE_FIRST: &str = "Enhance first: the audio is the only full copy of this meeting.";

fn has_enhanced_transcript(md: &str) -> bool {
    let Some(rest) = md.strip_prefix("---\n") else { return false };
    let Some(end) = rest.find("\n---\n") else { return false };
    rest[..end].lines().any(|l| l.trim() == "quality: enhanced")
        && rest[end + 5..].lines().any(|l| l.starts_with('['))
}

/// `retention_hours: None` is the manual path: no retention window, same enhance rule.
pub fn check(meta: &serde_json::Value, transcript_md: Option<&str>, now: DateTime<Utc>, retention_hours: Option<u32>) -> Result<(), String> {
    if meta.get("audio_deleted_at").and_then(|v| v.as_str()).is_some() {
        return Err("Audio already removed.".into());
    }
    let enhanced = &meta["transcription"]["enhanced"];
    if enhanced["status"].as_str() != Some("done") {
        return Err(ENHANCE_FIRST.into());
    }
    if !transcript_md.map(has_enhanced_transcript).unwrap_or(false) {
        return Err(ENHANCE_FIRST.into());
    }
    if let Some(hours) = retention_hours {
        let at = enhanced["at"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .ok_or("No enhance time recorded")?;
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

fn invalid(msg: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
}

/// Remove the audio file and `.checkpoints`, then mark metadata.json. metadata.json is read
/// first (missing is fine, unreadable is not) so a failure leaves the audio in place.
pub fn delete_audio(folder: &Path, now: DateTime<Utc>) -> std::io::Result<u64> {
    let meta_path = folder.join("metadata.json");
    let mut meta: serde_json::Value = match std::fs::read_to_string(&meta_path) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| invalid(format!("metadata.json is not valid JSON: {}", e)))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(e),
    };
    let obj = meta.as_object_mut().ok_or_else(|| invalid("metadata.json root is not an object".into()))?;

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
    obj.insert("audio_file".into(), "".into());
    obj.insert("audio_deleted_at".into(), now.to_rfc3339().into());
    let tmp = folder.join(".metadata.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&meta)?)?;
    std::fs::rename(&tmp, &meta_path)?;
    Ok(freed)
}

fn read_meta(folder: &Path) -> serde_json::Value {
    std::fs::read_to_string(folder.join("metadata.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Delete audio for every meeting that passes `check` with the retention window. Returns the count.
pub async fn run_once(pool: &sqlx::SqlitePool, now: DateTime<Utc>, retention_hours: u32) -> usize {
    let rows: Vec<(String, String)> = match sqlx::query_as("SELECT id, folder_path FROM meetings WHERE folder_path IS NOT NULL")
        .fetch_all(pool)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::warn!("audio cleanup: could not list meetings: {}", e);
            return 0;
        }
    };
    let running = super::retranscription::running_meeting_id();
    let mut deleted = 0;
    for (id, folder_path) in rows {
        if running.as_deref() == Some(id.as_str()) || super::library::is_live_folder(Some(&folder_path)).await {
            continue;
        }
        let folder = PathBuf::from(&folder_path);
        if !folder.is_dir() {
            continue;
        }
        let md = std::fs::read_to_string(folder.join("transcript.md")).ok();
        if check(&read_meta(&folder), md.as_deref(), now, Some(retention_hours)).is_err() {
            continue;
        }
        match delete_audio(&folder, now) {
            Ok(n) => {
                log::info!("audio cleanup: {} freed {} bytes", id, n);
                deleted += 1;
            }
            Err(e) => log::warn!("audio cleanup: {} failed: {}", id, e),
        }
    }
    deleted
}

/// Startup run, then hourly.
pub async fn run<R: Runtime>(app: AppHandle<R>) {
    loop {
        match super::recording_preferences::load_recording_preferences(&app).await {
            Ok(prefs) if prefs.delete_audio_after_enhance => {
                if let Some(state) = app.try_state::<crate::state::AppState>() {
                    run_once(state.db_manager.pool(), Utc::now(), prefs.audio_retention_hours).await;
                }
            }
            Ok(_) => {}
            Err(e) => log::warn!("audio cleanup: could not load preferences: {}", e),
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

async fn folder_of<R: Runtime>(app: &AppHandle<R>, meeting_id: &str) -> Result<Option<String>, String> {
    let state = app.state::<crate::state::AppState>();
    let row: Option<(Option<String>,)> = sqlx::query_as("SELECT folder_path FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(state.db_manager.pool())
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    row.map(|r| r.0).ok_or_else(|| format!("No meeting found with id {}", meeting_id))
}

#[tauri::command]
pub async fn delete_meeting_audio<R: Runtime>(app: AppHandle<R>, meeting_id: String) -> Result<String, String> {
    let folder_path = folder_of(&app, &meeting_id).await?;
    if super::library::is_live_folder(folder_path.as_deref()).await {
        return Err("This meeting is being recorded right now.".to_string());
    }
    if super::retranscription::running_meeting_id().as_deref() == Some(meeting_id.as_str()) {
        return Err("This meeting is being enhanced right now.".to_string());
    }
    let folder = folder_path.map(PathBuf::from).filter(|p| p.is_dir()).ok_or("Folder not found on disk")?;
    let md = std::fs::read_to_string(folder.join("transcript.md")).ok();
    check(&read_meta(&folder), md.as_deref(), Utc::now(), None)?;
    let bytes = delete_audio(&folder, Utc::now()).map_err(|e| e.to_string())?;
    Ok(format!("Audio deleted ({:.1} MB freed)", bytes as f64 / 1e6))
}

#[derive(Debug, Serialize)]
pub struct MeetingExtras {
    pub tags: Vec<String>,
    pub audio_deleted_at: Option<String>,
    pub has_audio: bool,
}

#[tauri::command]
pub async fn get_meeting_extras<R: Runtime>(app: AppHandle<R>, meeting_id: String) -> Result<MeetingExtras, String> {
    let folder = folder_of(&app, &meeting_id).await?.map(PathBuf::from).filter(|p| p.is_dir());
    let Some(folder) = folder else {
        return Ok(MeetingExtras { tags: vec![], audio_deleted_at: None, has_audio: false });
    };
    Ok(MeetingExtras {
        tags: super::search_index::metadata_tags(&folder),
        audio_deleted_at: read_meta(&folder)["audio_deleted_at"].as_str().map(str::to_string),
        has_audio: super::retranscription::find_audio_file(&folder).is_ok(),
    })
}

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

    #[test]
    fn unreadable_metadata_keeps_the_audio() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path();
        std::fs::write(f.join("audio.mp4"), vec![0u8; 10]).unwrap();
        std::fs::write(f.join("metadata.json"), "{not json").unwrap();
        assert!(delete_audio(f, now()).is_err());
        assert!(f.join("audio.mp4").exists());
        assert_eq!(std::fs::read_to_string(f.join("metadata.json")).unwrap(), "{not json");
    }

    fn enhanced_folder(root: &Path, name: &str, at: &str) -> PathBuf {
        let f = root.join(name);
        std::fs::create_dir(&f).unwrap();
        std::fs::write(f.join("audio.mp4"), vec![0u8; 10]).unwrap();
        std::fs::write(f.join("transcript.md"), MD).unwrap();
        std::fs::write(f.join("metadata.json"), meta("done", at).to_string()).unwrap();
        f
    }

    #[tokio::test]
    async fn run_once_only_deletes_past_the_window() {
        let root = tempfile::tempdir().unwrap();
        let old = enhanced_folder(root.path(), "old", "2026-10-20T00:00:00Z");
        let recent = enhanced_folder(root.path(), "recent", "2026-10-28T11:00:00Z");
        let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE meetings (id TEXT PRIMARY KEY, folder_path TEXT)").execute(&pool).await.unwrap();
        for (id, f) in [("a", &old), ("b", &recent)] {
            sqlx::query("INSERT INTO meetings (id, folder_path) VALUES (?, ?)")
                .bind(id).bind(f.to_str().unwrap()).execute(&pool).await.unwrap();
        }
        assert_eq!(run_once(&pool, now(), 24).await, 1);
        assert!(!old.join("audio.mp4").exists());
        assert!(recent.join("audio.mp4").exists());
    }
}
