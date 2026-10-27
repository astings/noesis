//! Enhance job status stored in a meeting folder's metadata.json under
//! `transcription.enhanced` (spec 5.2). File helpers plus one Tauri command
//! that lists the status of every meeting in the database.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnhanceStatusEntry {
    pub meeting_id: String,
    pub status: String,
}

/// Merge `transcription.enhanced = {provider, model, status, at, error}` into
/// metadata.json, creating the file when it is missing. Every other key is kept.
/// Writes go through a temp file and a rename so a crash never leaves half a file.
pub fn write_enhance_status(
    folder: &Path,
    provider: &str,
    model: &str,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    let path = folder.join("metadata.json");
    let mut root: serde_json::Value = if path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&path)?)?
    } else {
        serde_json::json!({})
    };
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("metadata.json root is not an object"))?;
    let transcription = obj
        .entry("transcription")
        .or_insert_with(|| serde_json::json!({}));
    if !transcription.is_object() {
        *transcription = serde_json::json!({});
    }
    transcription.as_object_mut().unwrap().insert(
        "enhanced".to_string(),
        serde_json::json!({
            "provider": provider,
            "model": model,
            "status": status,
            "at": chrono::Utc::now().to_rfc3339(),
            "error": error,
        }),
    );
    let tmp = folder.join(".metadata.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&root)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// `transcription.enhanced.status`, or None when the folder has no metadata.json
/// or no enhance record (every folder recorded before this phase).
pub fn read_enhance_status(folder: &Path) -> Option<String> {
    let text = std::fs::read_to_string(folder.join("metadata.json")).ok()?;
    let root: serde_json::Value = serde_json::from_str(&text).ok()?;
    root.get("transcription")?
        .get("enhanced")?
        .get("status")?
        .as_str()
        .map(str::to_string)
}

/// Status of every meeting that has a folder and an enhance record. Meetings
/// recorded before this phase, or whose folder moved, are simply absent.
#[tauri::command]
pub async fn list_enhance_statuses(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<EnhanceStatusEntry>, String> {
    let pool = state.db_manager.pool();
    let meetings = crate::database::repositories::meeting::MeetingsRepository::get_meetings(pool)
        .await
        .map_err(|e| format!("Failed to list meetings: {}", e))?;
    Ok(meetings
        .into_iter()
        .filter_map(|m| {
            let folder = m.folder_path?;
            let status = read_enhance_status(Path::new(&folder))?;
            Some(EnhanceStatusEntry { meeting_id: m.id, status })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_root(dir: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("metadata.json")).unwrap()).unwrap()
    }

    #[test]
    fn write_then_read_round_trips_and_preserves_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("metadata.json"),
            r#"{"version":"1.0","meeting_name":"Meeting 1","status":"completed"}"#,
        )
        .unwrap();

        write_enhance_status(dir.path(), "whisper", "large-v3", STATUS_QUEUED, None).unwrap();
        assert_eq!(read_enhance_status(dir.path()).as_deref(), Some("queued"));

        write_enhance_status(dir.path(), "whisper", "large-v3", STATUS_FAILED, Some("boom")).unwrap();
        let root = read_root(dir.path());
        assert_eq!(root["meeting_name"], "Meeting 1");
        assert_eq!(root["status"], "completed");
        assert_eq!(root["transcription"]["enhanced"]["status"], "failed");
        assert_eq!(root["transcription"]["enhanced"]["error"], "boom");
        assert_eq!(root["transcription"]["enhanced"]["model"], "large-v3");
        assert_eq!(root["transcription"]["enhanced"]["provider"], "whisper");
        assert!(root["transcription"]["enhanced"]["at"].is_string());
    }

    #[test]
    fn missing_metadata_reads_none_and_write_creates_it() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_enhance_status(dir.path()), None);
        write_enhance_status(dir.path(), "whisper", "large-v3", STATUS_RUNNING, None).unwrap();
        assert_eq!(read_enhance_status(dir.path()).as_deref(), Some("running"));
    }

    #[test]
    fn legacy_metadata_without_enhance_reads_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("metadata.json"),
            r#"{"version":"1.0","retranscribed_at":"2026-09-30T19:31:30Z"}"#,
        )
        .unwrap();
        assert_eq!(read_enhance_status(dir.path()), None);
    }
}
