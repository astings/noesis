//! spec 4.2: link meeting folders the reconcile scan cannot see (no `meeting_id` in metadata.json, or an id
//! SQLite does not know) to their database row, and import folders that have no row at all.
//! Runs before `library::reconcile`, which then syncs folder_path and title for the linked folders.

use crate::api::TranscriptSegment;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A meeting folder on disk, as far as adoption cares.
#[derive(Debug, Clone)]
pub struct Folder {
    pub path: String,
    pub meeting_id: Option<String>,
    /// metadata.json has a non-empty `title` (v2); v1 files only carry the original `meeting_name`
    pub has_title: bool,
    pub segment_ids: Vec<String>,
    /// a recording is running or transcripts.json was written moments ago: never import
    pub busy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Write `meeting_id` (and `title` when the file has none: old renames only reached SQLite) into metadata.json.
    Link { folder: String, meeting_id: String, title: Option<String> },
    /// Create the meeting row from metadata.json and transcripts.json; `meeting_id` is the disk id when it is free.
    Import { folder: String, meeting_id: Option<String> },
}

/// `db_rows` is `(id, folder_path, title)`; `segment_owner` maps a transcript id to its meeting id.
pub fn plan(db_rows: &[(String, Option<String>, String)], segment_owner: &HashMap<String, String>, folders: &[Folder]) -> Vec<Action> {
    let known: HashSet<&str> = db_rows.iter().map(|r| r.0.as_str()).collect();
    let on_disk: HashSet<&str> = folders.iter().map(|f| f.path.as_str()).collect();
    // a row is claimed by the folder whose metadata already names it, or by the first folder linked to it
    let mut claimed: HashSet<String> = folders.iter().filter_map(|f| f.meeting_id.clone()).filter(|id| known.contains(id.as_str())).collect();
    let mut out = Vec::new();
    for f in folders {
        if f.meeting_id.as_deref().is_some_and(|id| known.contains(id)) {
            continue; // linked already; reconcile handles it
        }
        let by_path = db_rows.iter().find(|r| r.1.as_deref() == Some(f.path.as_str()) && !claimed.contains(&r.0));
        // a folder renamed by hand: its row points at a path that is gone, and owns this folder's segments
        let by_segments = || {
            f.segment_ids.iter().filter_map(|s| segment_owner.get(s)).find_map(|id| {
                db_rows.iter().find(|r| &r.0 == id && !claimed.contains(&r.0) && !r.1.as_deref().is_some_and(|p| on_disk.contains(p)))
            })
        };
        if let Some(row) = by_path.or_else(by_segments) {
            claimed.insert(row.0.clone());
            out.push(Action::Link { folder: f.path.clone(), meeting_id: row.0.clone(), title: (!f.has_title).then(|| row.2.clone()) });
        } else if !f.segment_ids.is_empty() && !f.busy {
            let id = f.meeting_id.clone().filter(|id| !claimed.contains(id));
            if let Some(id) = &id { claimed.insert(id.clone()); }
            out.push(Action::Import { folder: f.path.clone(), meeting_id: id });
        }
    }
    out
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn segments_of(folder: &Path) -> Vec<serde_json::Value> {
    read_json(&folder.join("transcripts.json"))
        .and_then(|v| v.get("segments").and_then(|s| s.as_array()).cloned())
        .unwrap_or_default()
}

fn str_of(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty()).map(str::to_string)
}

/// ponytail: "written moments ago" is a 2 minute mtime window on transcripts.json, covering the gap between
/// a recording stopping and its row being saved; a per-folder lock with the recorder would be exact.
fn recently_written(folder: &Path) -> bool {
    std::fs::metadata(folder.join("transcripts.json"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < std::time::Duration::from_secs(120))
}

fn read_folder(path: &Path, recording: bool) -> Option<Folder> {
    let meta = read_json(&path.join("metadata.json"))?;
    Some(Folder {
        path: path.to_string_lossy().to_string(),
        meeting_id: str_of(&meta, "meeting_id"),
        has_title: str_of(&meta, "title").is_some(),
        segment_ids: segments_of(path).iter().filter_map(|s| str_of(s, "id")).collect(),
        busy: recording || recently_written(path),
    })
}

async fn import(pool: &sqlx::SqlitePool, folder: &Path, meeting_id: Option<String>) -> Result<String, String> {
    let meta = read_json(&folder.join("metadata.json")).unwrap_or_default();
    let title = str_of(&meta, "title")
        .or_else(|| str_of(&meta, "meeting_name"))
        .unwrap_or_else(|| folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    let segments: Vec<TranscriptSegment> = segments_of(folder)
        .iter()
        .map(|s| TranscriptSegment {
            id: str_of(s, "id").unwrap_or_default(),
            text: str_of(s, "text").unwrap_or_default(),
            timestamp: str_of(s, "timestamp").or_else(|| str_of(s, "display_time")).unwrap_or_default(),
            audio_start_time: s.get("audio_start_time").and_then(|x| x.as_f64()),
            audio_end_time: s.get("audio_end_time").and_then(|x| x.as_f64()),
            duration: s.get("duration").and_then(|x| x.as_f64()),
            speaker: str_of(s, "speaker"),
        })
        .collect();
    let folder_str = folder.to_string_lossy().to_string();
    let id = crate::database::repositories::transcript::TranscriptsRepository::save_transcript(pool, &title, &segments, Some(folder_str), meeting_id)
        .await
        .map_err(|e| e.to_string())?;
    // keep the recording date, not the import date, so the sidebar sorts it where it belongs
    if let Some(created) = str_of(&meta, "created_at").and_then(|c| chrono::DateTime::parse_from_rfc3339(&c).ok()) {
        sqlx::query("UPDATE meetings SET created_at = ? WHERE id = ?")
            .bind(created.with_timezone(&chrono::Utc))
            .bind(&id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(id)
}

fn write_link(folder: &Path, meeting_id: &str, title: Option<&str>) -> Result<(), String> {
    super::metadata_file::update_metadata(folder, |obj| {
        obj.insert("meeting_id".into(), serde_json::json!(meeting_id));
        if let Some(t) = title {
            obj.insert("title".into(), serde_json::json!(t));
        }
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Link and import what `plan` finds. Returns the meeting ids touched, for the search index.
pub async fn adopt(pool: &sqlx::SqlitePool, root: &Path) -> Result<Vec<String>, String> {
    let recording = crate::audio::recording_commands::is_recording().await;
    let folders: Vec<Folder> = super::library::meeting_folders(root).iter().filter_map(|p| read_folder(p, recording)).collect();
    let db_rows: Vec<(String, Option<String>, String)> =
        sqlx::query_as("SELECT id, folder_path, title FROM meetings").fetch_all(pool).await.map_err(|e| e.to_string())?;
    let owners: Vec<(String, String)> =
        sqlx::query_as("SELECT id, meeting_id FROM transcripts").fetch_all(pool).await.map_err(|e| e.to_string())?;
    let segment_owner: HashMap<String, String> = owners.into_iter().collect();

    let mut touched = Vec::new();
    for action in plan(&db_rows, &segment_owner, &folders) {
        match action {
            Action::Link { folder, meeting_id, title } => match write_link(Path::new(&folder), &meeting_id, title.as_deref()) {
                Ok(()) => {
                    log::info!("Linked meeting folder {} to {}", folder, meeting_id);
                    touched.push(meeting_id);
                }
                Err(e) => log::warn!("Could not link meeting folder {}: {}", folder, e),
            },
            Action::Import { folder, meeting_id } => match import(pool, Path::new(&folder), meeting_id).await {
                Ok(id) => {
                    if let Err(e) = write_link(Path::new(&folder), &id, None) {
                        log::warn!("Imported {} as {} but could not write its meeting_id: {}", folder, id, e);
                    }
                    log::info!("Imported meeting folder {} as {}", folder, id);
                    touched.push(id);
                }
                Err(e) => log::warn!("Could not import meeting folder {}: {}", folder, e),
            },
        }
    }
    Ok(touched)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(path: &str, id: Option<&str>, has_title: bool, segs: &[&str]) -> Folder {
        Folder { path: path.into(), meeting_id: id.map(Into::into), has_title, segment_ids: segs.iter().map(|s| s.to_string()).collect(), busy: false }
    }
    fn row(id: &str, path: Option<&str>, title: &str) -> (String, Option<String>, String) {
        (id.into(), path.map(Into::into), title.into())
    }

    #[test]
    fn links_by_path_by_segments_and_imports_the_rest() {
        let db = vec![
            row("m-path", Some("/r/Meeting old"), "Renamed in Meetily"),
            row("m-moved", Some("/r/Meeting gone"), "DTE"),
            row("m-v2", Some("/r/flowers/x"), "V2 title"),
            row("m-ok", Some("/r/dex/ok"), "Fine"),
        ];
        let owners: HashMap<String, String> = [("s-moved".to_string(), "m-moved".to_string()), ("s-ok".into(), "m-ok".into())].into();
        let folders = vec![
            folder("/r/Meeting old", None, false, &["a"]),
            folder("/r/dte-energy by hand", None, false, &["s-moved"]),
            folder("/r/flowers/x", Some("m-stale-disk-id"), true, &["b"]),
            folder("/r/dex/ok", Some("m-ok"), true, &["s-ok"]),
            folder("/r/orphan", None, false, &["c"]),
            folder("/r/_inbox/orphan-v2", Some("m-free"), true, &["d"]),
            folder("/r/empty", None, false, &[]),
        ];
        let mut busy = folder("/r/live", Some("m-live"), true, &["e"]);
        busy.busy = true;
        let mut all = folders.clone();
        all.push(busy);
        assert_eq!(plan(&db, &owners, &all), vec![
            Action::Link { folder: "/r/Meeting old".into(), meeting_id: "m-path".into(), title: Some("Renamed in Meetily".into()) },
            Action::Link { folder: "/r/dte-energy by hand".into(), meeting_id: "m-moved".into(), title: Some("DTE".into()) },
            Action::Link { folder: "/r/flowers/x".into(), meeting_id: "m-v2".into(), title: None },
            Action::Import { folder: "/r/orphan".into(), meeting_id: None },
            Action::Import { folder: "/r/_inbox/orphan-v2".into(), meeting_id: Some("m-free".into()) },
        ]);
    }

    #[test]
    fn a_row_whose_folder_still_exists_is_not_stolen_by_a_copy() {
        let db = vec![row("m", Some("/r/a"), "A")];
        let owners: HashMap<String, String> = [("s".to_string(), "m".to_string())].into();
        // /r/a is linked already; /r/copy is an old-format duplicate with the same segments: imported, not linked
        let folders = vec![folder("/r/a", Some("m"), true, &["s"]), folder("/r/copy", None, false, &["s"])];
        assert_eq!(plan(&db, &owners, &folders), vec![Action::Import { folder: "/r/copy".into(), meeting_id: None }]);
    }
}
