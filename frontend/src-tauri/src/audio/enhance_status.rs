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
    super::metadata_file::update_metadata(folder, |obj| {
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
        Ok(())
    })?;
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

/// Error recorded on an enhance the app did not live to finish.
pub const INTERRUPTED: &str = "Interrupted: the app closed before enhance finished. Enhance again to retry.";

/// No enhance job survives a restart, so a `queued` or `running` status found at startup is
/// stale (the app quit or was rebuilt mid-job). Marks those `failed` under `<root>/<project>/<meeting>/`
/// and keeps provider and model. Audio is kept: cleanup only deletes after `done`.
pub fn mark_interrupted(root: &Path) -> usize {
    let mut repaired = 0;
    let Ok(projects) = std::fs::read_dir(root) else { return 0 };
    for project in projects.flatten().filter(|e| e.path().is_dir()) {
        let Ok(meetings) = std::fs::read_dir(project.path()) else { continue };
        for meeting in meetings.flatten().filter(|e| e.path().is_dir()) {
            let folder = meeting.path();
            let status = read_enhance_status(&folder);
            if !matches!(status.as_deref(), Some(STATUS_QUEUED) | Some(STATUS_RUNNING)) {
                continue;
            }
            let text = std::fs::read_to_string(folder.join("metadata.json")).unwrap_or_default();
            let meta: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            let enhanced = &meta["transcription"]["enhanced"];
            let provider = enhanced["provider"].as_str().unwrap_or("whisper");
            let model = enhanced["model"].as_str().unwrap_or("configured");
            match write_enhance_status(&folder, provider, model, STATUS_FAILED, Some(INTERRUPTED)) {
                Ok(()) => {
                    log::info!("enhance status: marked interrupted job failed in {}", folder.display());
                    repaired += 1;
                }
                Err(e) => log::warn!("enhance status: could not repair {}: {}", folder.display(), e),
            }
        }
    }
    repaired
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

    #[test]
    fn mark_interrupted_fails_running_and_queued_and_keeps_the_rest() {
        let root = tempfile::tempdir().unwrap();
        let make = |name: &str, status: Option<&str>| {
            let dir = root.path().join("proj").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let mut meta = serde_json::json!({ "meeting_id": name, "title": name });
            if let Some(s) = status {
                meta["transcription"] = serde_json::json!({ "enhanced": { "provider": "whisper", "model": "large-v3", "status": s, "at": "x", "error": null } });
            }
            std::fs::write(dir.join("metadata.json"), meta.to_string()).unwrap();
            dir
        };
        let running = make("a", Some(STATUS_RUNNING));
        let queued = make("b", Some(STATUS_QUEUED));
        let done = make("c", Some(STATUS_DONE));
        let legacy = make("d", None);

        assert_eq!(mark_interrupted(root.path()), 2);

        let read = |d: &std::path::Path| -> serde_json::Value { serde_json::from_str(&std::fs::read_to_string(d.join("metadata.json")).unwrap()).unwrap() };
        for d in [&running, &queued] {
            let m = read(d);
            assert_eq!(m["transcription"]["enhanced"]["status"], STATUS_FAILED);
            assert_eq!(m["transcription"]["enhanced"]["error"], INTERRUPTED);
            assert_eq!(m["transcription"]["enhanced"]["model"], "large-v3", "provider and model kept");
            assert_eq!(m["title"], m["meeting_id"], "other keys kept");
        }
        assert_eq!(read(&done)["transcription"]["enhanced"]["status"], STATUS_DONE);
        assert!(read(&legacy).get("transcription").is_none());
    }
}
