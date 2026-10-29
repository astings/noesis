//! spec 8: per-meeting hook status. `running` and `failed` live in memory for this
//! session; `done` is the `hook.done` marker in the meeting folder.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

pub const RUNNING: &str = "running";
pub const DONE: &str = "done";
pub const FAILED: &str = "failed";
pub const CANCELLED: &str = "cancelled";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookStatusEntry {
    pub meeting_id: String,
    pub status: String,
}

static MEMORY: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();

fn memory() -> std::sync::MutexGuard<'static, HashMap<String, &'static str>> {
    MEMORY.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// Record a transition. `done` clears memory: the marker on disk says it from now on.
pub fn set(meeting_id: &str, status: &'static str) {
    if status == DONE {
        memory().remove(meeting_id);
    } else {
        memory().insert(meeting_id.to_string(), status);
    }
}

/// Atomically reserve the RUNNING slot; false when a run already holds it.
pub fn try_start(meeting_id: &str) -> bool {
    let mut m = memory();
    if m.get(meeting_id) == Some(&RUNNING) {
        return false;
    }
    m.insert(meeting_id.to_string(), RUNNING);
    true
}

pub fn memory_status(meeting_id: &str) -> Option<&'static str> {
    memory().get(meeting_id).copied()
}

pub fn status_for(meeting_id: &str, folder: &Path) -> Option<&'static str> {
    memory_status(meeting_id).or_else(|| folder.join("hook.done").exists().then_some(DONE))
}

/// Hook status of every meeting that has one, mirroring `list_enhance_statuses`.
#[tauri::command]
pub async fn list_hook_statuses(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<HookStatusEntry>, String> {
    let pool = state.db_manager.pool();
    let meetings = crate::database::repositories::meeting::MeetingsRepository::get_meetings(pool)
        .await
        .map_err(|e| format!("Failed to list meetings: {}", e))?;
    Ok(meetings
        .into_iter()
        .filter_map(|m| {
            let status = match m.folder_path {
                Some(folder) => status_for(&m.id, Path::new(&folder)),
                None => memory_status(&m.id),
            }?;
            Some(HookStatusEntry { meeting_id: m.id, status: status.to_string() })
        })
        .collect())
}

/// The "Run hook" action: same dispatch as after enhance, ignoring `hook.done`.
#[tauri::command]
pub async fn run_meeting_hook<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, crate::state::AppState>,
    meeting_id: String,
) -> Result<String, String> {
    let row: Option<(Option<String>,)> = sqlx::query_as("SELECT folder_path FROM meetings WHERE id = ?")
        .bind(&meeting_id)
        .fetch_optional(state.db_manager.pool())
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    let Some((folder_path,)) = row else { return Err(format!("No meeting found with id {}", meeting_id)) };
    if super::library::is_live_folder(folder_path.as_deref()).await {
        return Err("This meeting is being recorded right now.".to_string());
    }
    let Some(folder) = folder_path.map(std::path::PathBuf::from).filter(|p| p.is_dir()) else {
        return Err("Meeting folder not found on disk.".to_string());
    };
    let prefs = super::recording_preferences::load_recording_preferences(&app)
        .await
        .map_err(|e| format!("Could not load preferences: {}", e))?;
    let ctx = super::post_enhance_hook::read_context(&folder, &meeting_id);
    match super::post_enhance_hook::dispatch(&app, &prefs, &ctx, true)? {
        super::post_enhance_hook::HookKind::Wiki => Ok("Wiki automation started".to_string()),
        super::post_enhance_hook::HookKind::Command => Ok("Hook started".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_prefers_memory_then_the_marker() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(status_for("m-st", dir.path()), None);
        std::fs::write(dir.path().join("hook.done"), "x").unwrap();
        assert_eq!(status_for("m-st", dir.path()), Some("done"));
        set("m-st", RUNNING);
        assert_eq!(status_for("m-st", dir.path()), Some("running"));
        set("m-st", FAILED);
        assert_eq!(status_for("m-st", dir.path()), Some("failed"));
        set("m-st", DONE);
        assert_eq!(memory_status("m-st"), None);
        assert_eq!(status_for("m-st", dir.path()), Some("done"));
    }
}
