//! spec 4.2: the meeting library on disk. Folder naming, project listing, a
//! scan of `<root>/<project>/<meeting>/metadata.json`, and the reconcile that
//! repairs the SQLite index from it. The filesystem is the source of truth.

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const INBOX: &str = "_inbox";

static AUTO_TITLE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^Meeting (\d{2}_\d{2}_\d{2}_\d{2}_\d{2}_\d{2}|\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2})$").unwrap()
});
static STAMP_PREFIX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}").unwrap());

/// Lowercase ASCII slug, non-alphanumerics collapsed to one dash, at most 60 chars.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
        if out.len() >= 60 {
            break;
        }
    }
    out.trim_matches('-').to_string()
}

/// True for the titles the app generates itself ("Meeting 27_10_26_12_03_07" or
/// "Meeting 2026-10-27_12-03-07"). Those carry no information worth a slug.
pub fn is_auto_title(title: &str) -> bool {
    AUTO_TITLE.is_match(title.trim())
}

/// Folder stamp in local time, minute precision.
pub fn local_stamp(now: chrono::DateTime<chrono::Local>) -> String {
    now.format("%Y-%m-%d_%H-%M").to_string()
}

/// The stamp prefix of a v2 folder name, if it has one.
pub fn stamp_of_folder(folder_name: &str) -> Option<&str> {
    STAMP_PREFIX.find(folder_name).map(|m| m.as_str())
}

/// `<stamp>` or `<stamp> <slug>` (spec 5.1).
pub fn meeting_folder_name(stamp: &str, title: Option<&str>) -> String {
    match title.filter(|t| !is_auto_title(t)).map(slug).filter(|s| !s.is_empty()) {
        Some(s) => format!("{} {}", stamp, s),
        None => stamp.to_string(),
    }
}

/// The project a meeting folder belongs to: its parent directory when the
/// folder sits two levels under the root, `_inbox` otherwise (legacy flat
/// folders, or anything outside the root).
pub fn project_of(root: &Path, folder: &Path) -> String {
    match folder.strip_prefix(root) {
        Ok(rel) => {
            let parts: Vec<_> = rel.components().collect();
            if parts.len() >= 2 {
                parts[0].as_os_str().to_string_lossy().to_string()
            } else {
                INBOX.to_string()
            }
        }
        Err(_) => INBOX.to_string(),
    }
}

/// Project folders under the root, `_inbox` first, then sorted. A directory is a
/// project when it is not hidden and holds no metadata.json (legacy meetings do).
pub fn list_projects(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .filter(|e| !e.path().join("metadata.json").exists())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n != INBOX)
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.insert(0, INBOX.to_string());
    names
}

/// A project is one normal path component: not empty, no `/` or `\`, not `.` or `..`, no leading dot.
pub fn is_valid_project_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && n != "."
        && n != ".."
        && !n.starts_with('.')
        && !n.contains('/')
        && !n.contains('\\')
        && !n.contains('\0')
}

/// Project name as `RecordingSaver::set_project` applies it: invalid names become `_inbox`.
pub fn normalize_project(name: &str) -> String {
    if is_valid_project_name(name) { name.to_string() } else { INBOX.to_string() }
}

/// Live project change: the project to move the meeting to after stop, or None when
/// nothing is pending or the pending project is the one the recording started in.
pub fn pending_move(started_project: &str, pending: Option<&str>) -> Option<String> {
    let target = normalize_project(pending?);
    (target != normalize_project(started_project)).then_some(target)
}

/// Create `<root>/<project>/<stamp>[ <slug>]/` (and `.checkpoints/` when asked),
/// adding `-2`, `-3`, ... when a folder with that name already exists.
pub fn create_meeting_folder(
    root: &Path,
    project: &str,
    title: Option<&str>,
    create_checkpoints: bool,
) -> Result<PathBuf> {
    create_meeting_folder_with_stamp(root, project, &local_stamp(chrono::Local::now()), title, create_checkpoints)
}

/// Same as `create_meeting_folder` with an explicit stamp (tests, and callers that already know the time).
pub fn create_meeting_folder_with_stamp(
    root: &Path,
    project: &str,
    stamp: &str,
    title: Option<&str>,
    create_checkpoints: bool,
) -> Result<PathBuf> {
    if !is_valid_project_name(project) {
        anyhow::bail!("Invalid project name: {:?}", project);
    }
    let base = meeting_folder_name(stamp, title);
    let project_dir = root.join(project);
    std::fs::create_dir_all(&project_dir)?;
    let mut candidate = project_dir.join(&base);
    let mut n = 2;
    while candidate.exists() {
        candidate = project_dir.join(format!("{}-{}", base, n));
        n += 1;
    }
    std::fs::create_dir_all(&candidate)?;
    if create_checkpoints {
        std::fs::create_dir_all(candidate.join(".checkpoints"))?;
    }
    log::info!("Created meeting folder {}", candidate.display());
    Ok(candidate)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LibraryEntry {
    pub meeting_id: String,
    pub project: String,
    pub title: Option<String>,
    pub folder_path: String,
}

fn read_entry(root: &Path, folder: &Path) -> Option<LibraryEntry> {
    let text = std::fs::read_to_string(folder.join("metadata.json")).ok()?;
    let meta: serde_json::Value = serde_json::from_str(&text).ok()?;
    let meeting_id = meta.get("meeting_id")?.as_str()?.to_string();
    let title = meta
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|t| !t.trim().is_empty())
        .or_else(|| meta.get("meeting_name").and_then(|v| v.as_str()))
        .filter(|t| !t.trim().is_empty())
        .map(str::to_string);
    Some(LibraryEntry {
        meeting_id,
        project: project_of(root, folder),
        title,
        folder_path: folder.to_string_lossy().to_string(),
    })
}

/// Every v2 meeting under `<root>/<project>/<meeting>/`. Legacy flat folders
/// have `meeting_id: null` and are skipped; the MCP server reads them directly.
pub fn scan(root: &Path) -> Vec<LibraryEntry> {
    let mut out = Vec::new();
    let Ok(projects) = std::fs::read_dir(root) else { return out };
    for project in projects.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
        if project.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if project.path().join("metadata.json").exists() {
            continue; // legacy meeting at the root
        }
        let Ok(meetings) = std::fs::read_dir(project.path()) else { continue };
        for meeting in meetings.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
            if let Some(entry) = read_entry(root, &meeting.path()) {
                out.push(entry);
            }
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReconcileUpdate {
    pub meeting_id: String,
    pub folder_path: String,
    pub title: Option<String>,
}

/// Rows that need an update: `db_rows` is `(id, folder_path, title)` from SQLite.
/// Metadata wins over the database (spec 4.2).
pub fn plan_reconcile(
    db_rows: &[(String, Option<String>, String)],
    scanned: &[LibraryEntry],
) -> Vec<ReconcileUpdate> {
    let mut updates = Vec::new();
    for entry in scanned {
        let Some((_, folder, title)) = db_rows.iter().find(|(id, _, _)| *id == entry.meeting_id) else { continue };
        let folder_differs = folder.as_deref() != Some(entry.folder_path.as_str());
        let title_differs = matches!(&entry.title, Some(t) if t != title);
        if folder_differs || title_differs {
            updates.push(ReconcileUpdate {
                meeting_id: entry.meeting_id.clone(),
                folder_path: entry.folder_path.clone(),
                title: if title_differs { entry.title.clone() } else { None },
            });
        }
    }
    updates
}

/// Update folder_path and title in SQLite from what is on disk. Returns the number of rows changed.
/// ponytail: reads every metadata.json on each call; fine for a few hundred meetings, cache the scan if it grows.
pub async fn reconcile(pool: &sqlx::SqlitePool, root: &Path) -> Result<usize> {
    let rows: Vec<(String, Option<String>, String)> =
        sqlx::query_as("SELECT id, folder_path, title FROM meetings")
            .fetch_all(pool)
            .await?;
    let updates = plan_reconcile(&rows, &scan(root));
    let now = chrono::Utc::now();
    for u in &updates {
        sqlx::query("UPDATE meetings SET folder_path = ?, title = COALESCE(?, title), updated_at = ? WHERE id = ?")
            .bind(&u.folder_path)
            .bind(&u.title)
            .bind(now)
            .bind(&u.meeting_id)
            .execute(pool)
            .await?;
        log::info!("Reconciled meeting {} to {}", u.meeting_id, u.folder_path);
    }
    Ok(updates.len())
}

/// Write the title to metadata.json (merged into the existing JSON) and to SQLite, without touching the folder.
async fn update_title_in_place(
    pool: &sqlx::SqlitePool,
    folder: &Path,
    meeting_id: &str,
    title: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> std::result::Result<(), String> {
    // A missing metadata.json stays missing; an unreadable one is left alone and the title goes to SQLite only.
    if folder.join("metadata.json").exists() {
        if let Err(e) = super::metadata_file::update_metadata(folder, |obj| {
            obj.insert("meeting_id".into(), serde_json::json!(meeting_id));
            obj.insert("title".into(), serde_json::json!(title));
            Ok(())
        }) {
            log::warn!("Could not update metadata.json title in place at {}: {}", folder.display(), e);
        }
    }
    sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
        .bind(title)
        .bind(now)
        .bind(meeting_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    Ok(())
}

/// True when `folder` is the folder of the recording in progress.
pub async fn is_live_folder(folder: Option<&str>) -> bool {
    if !crate::audio::recording_commands::is_recording().await {
        return false;
    }
    matches!(crate::audio::recording_commands::get_meeting_folder_path().await, Ok(Some(active)) if folder == Some(active.as_str()))
}

/// Move and/or rename a meeting folder, update metadata.json and the SQLite row.
/// Rules (spec 4.2): the folder keeps its stamp; the title decides the slug; the project
/// decides the parent; a legacy folder gets its meeting_id written; the database is
/// updated even when the folder is gone (title only).
pub async fn assign(
    pool: &sqlx::SqlitePool,
    root: &Path,
    meeting_id: &str,
    project: Option<&str>,
    title: Option<&str>,
) -> std::result::Result<PathBuf, String> {
    let result = assign_inner(pool, root, meeting_id, project, title).await;
    // title and tags live in the search index; refresh even after a title-only update
    if let Err(e) = crate::audio::search_index::reindex_meeting(pool, meeting_id).await {
        log::warn!("search index: reindex after assign failed for {}: {}", meeting_id, e);
    }
    result
}

async fn assign_inner(
    pool: &sqlx::SqlitePool,
    root: &Path,
    meeting_id: &str,
    project: Option<&str>,
    title: Option<&str>,
) -> std::result::Result<PathBuf, String> {
    let row: Option<(Option<String>, String)> = sqlx::query_as("SELECT folder_path, title FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    let Some((folder_path, db_title)) = row else { return Err(format!("No meeting found with id {}", meeting_id)) };

    if let Some(p) = project {
        if !is_valid_project_name(p) {
            return Err(format!("Invalid project name: {}", p));
        }
    }
    // spec 4.2: while the enhance job holds this folder, update the title in place and skip the rename.
    // The folder slug catches up when the job finishes (see start_retranscription_command).
    let enhance_running = crate::audio::retranscription::running_meeting_id().as_deref() == Some(meeting_id);
    if enhance_running && project.is_some() {
        return Err("This meeting is being enhanced right now. Try moving it when it finishes.".to_string());
    }
    if project.is_some() && crate::audio::hook_status::memory_status(meeting_id) == Some(crate::audio::hook_status::RUNNING) {
        return Err("The hook is running for this meeting. Try moving it when it finishes.".to_string());
    }
    if is_live_folder(folder_path.as_deref()).await {
        return Err("This meeting is being recorded right now.".to_string());
    }

    let new_title = title.map(str::to_string).unwrap_or(db_title);
    let now = chrono::Utc::now();

    let Some(old_path) = folder_path.map(PathBuf::from).filter(|p| p.exists()) else {
        sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
            .bind(&new_title).bind(now).bind(meeting_id)
            .execute(pool).await.map_err(|e| format!("Database error: {}", e))?;
        return Err("Folder not found on disk; only the title was updated".to_string());
    };

    if enhance_running {
        update_title_in_place(pool, &old_path, meeting_id, &new_title, now).await?;
        return Ok(old_path);
    }

    let meta_path = old_path.join("metadata.json");
    let meta: serde_json::Value = std::fs::read_to_string(&meta_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let current_project = project_of(root, &old_path);
    let new_project = project.map(str::to_string).unwrap_or(current_project);
    let old_name = old_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    // spec 4.2 / 5.1: legacy flat folders are never moved by a title edit. An explicit project is a deliberate migration.
    if stamp_of_folder(&old_name).is_none() && project.is_none() {
        update_title_in_place(pool, &old_path, meeting_id, &new_title, now).await?;
        return Ok(old_path);
    }
    let stamp = match stamp_of_folder(&old_name) {
        Some(s) => s.to_string(),
        None => meta
            .get("created_at")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| local_stamp(d.with_timezone(&chrono::Local)))
            .unwrap_or_else(|| local_stamp(chrono::Local::now())),
    };
    let base = meeting_folder_name(&stamp, Some(&new_title));
    let project_dir = root.join(&new_project);
    std::fs::create_dir_all(&project_dir).map_err(|e| format!("Cannot create project folder: {}", e))?;
    let mut new_path = project_dir.join(&base);
    let mut n = 2;
    while new_path.exists() && new_path != old_path {
        new_path = project_dir.join(format!("{}-{}", base, n));
        n += 1;
    }
    if new_path != old_path {
        std::fs::rename(&old_path, &new_path).map_err(|e| format!("Cannot move folder: {}", e))?;
    }

    // Re-read after the move so a write that landed in between is kept.
    if let Err(e) = super::metadata_file::update_metadata(&new_path, |obj| {
        obj.insert("meeting_id".into(), serde_json::json!(meeting_id));
        obj.insert("project".into(), serde_json::json!(new_project));
        obj.insert("title".into(), serde_json::json!(new_title));
        obj.entry("version").or_insert(serde_json::json!("2.0"));
        Ok(())
    }) {
        log::warn!("Could not update metadata.json after move: {}", e);
    }

    sqlx::query("UPDATE meetings SET folder_path = ?, title = ?, updated_at = ? WHERE id = ?")
        .bind(new_path.to_string_lossy().to_string())
        .bind(&new_title)
        .bind(now)
        .bind(meeting_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    Ok(new_path)
}

#[tauri::command]
pub async fn list_projects_command() -> Result<Vec<String>, String> {
    Ok(list_projects(&super::recording_preferences::get_default_recordings_folder()))
}

#[tauri::command]
pub async fn assign_meeting(
    state: tauri::State<'_, crate::state::AppState>,
    meeting_id: String,
    project: Option<String>,
    title: Option<String>,
) -> Result<String, String> {
    let root = super::recording_preferences::get_default_recordings_folder();
    assign(state.db_manager.pool(), &root, &meeting_id, project.as_deref(), title.as_deref())
        .await
        .map(|p| p.to_string_lossy().to_string())
}

/// Guard for deletion: true only for `<root>/<project>/<meeting>` (depth 2) or a legacy flat
/// meeting folder directly under root (depth 1) that holds `metadata.json`.
pub fn trashable_folder(root: &Path, folder: &Path) -> bool {
    if folder.components().any(|c| c == std::path::Component::ParentDir) {
        return false;
    }
    let (Ok(root), Ok(folder)) = (root.canonicalize(), folder.canonicalize()) else {
        return false;
    };
    match folder.strip_prefix(&root).map(|r| r.components().count()) {
        Ok(2) => true,
        Ok(1) => folder.join("metadata.json").is_file(),
        _ => false,
    }
}

/// Move a meeting folder to the OS Trash if the guard passes. A missing folder is Ok.
pub fn trash_meeting_folder(root: &Path, folder: &Path) -> Result<()> {
    if !folder.exists() {
        log::info!("trash_meeting_folder: {} already gone", folder.display());
        return Ok(());
    }
    if !trashable_folder(root, folder) {
        anyhow::bail!("refusing to trash {}: not a meeting folder under the recordings root", folder.display());
    }
    // NsFileManager, not the default Finder/osascript method: that one needs an Apple Events
    // entitlement the hardened-runtime app does not have.
    #[allow(unused_mut)]
    let mut ctx = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        ctx.set_delete_method(DeleteMethod::NsFileManager);
    }
    ctx.delete(folder).map_err(|e| anyhow::anyhow!("{}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_move_none_when_nothing_pending() {
        assert_eq!(pending_move("acme", None), None);
    }

    #[test]
    fn pending_move_none_when_back_to_started_project() {
        assert_eq!(pending_move("acme", Some("acme")), None);
        assert_eq!(pending_move(INBOX, Some("../bad")), None);
        assert_eq!(pending_move("", Some(INBOX)), None);
    }

    #[test]
    fn pending_move_some_normalized_otherwise() {
        assert_eq!(pending_move("acme", Some("globex")), Some("globex".to_string()));
        assert_eq!(pending_move("acme", Some("../bad")), Some(INBOX.to_string()));
        assert_eq!(pending_move("acme", Some("")), Some(INBOX.to_string()));
        assert_eq!(pending_move(INBOX, Some("acme")), Some("acme".to_string()));
    }

    fn write_meta(dir: &Path, json: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("metadata.json"), json).unwrap();
    }

    #[test]
    fn slug_lowercases_and_collapses() {
        assert_eq!(slug("Western Digital: onboarding (P3)"), "western-digital-onboarding-p3");
        assert_eq!(slug("  --Hello--  "), "hello");
        assert_eq!(slug(""), "");
        assert_eq!(slug(&"x".repeat(100)).len(), 60);
    }

    #[test]
    fn auto_titles_are_recognized() {
        assert!(is_auto_title("Meeting 27_10_26_12_03_07"));
        assert!(is_auto_title("Meeting 2026-10-27_12-03-07"));
        assert!(!is_auto_title("Western Digital onboarding"));
        assert!(!is_auto_title("Meeting with Ben"));
    }

    #[test]
    fn folder_name_has_stamp_and_optional_slug() {
        assert_eq!(meeting_folder_name("2026-10-27_12-03", None), "2026-10-27_12-03");
        assert_eq!(meeting_folder_name("2026-10-27_12-03", Some("Meeting 27_10_26_12_03_07")), "2026-10-27_12-03");
        assert_eq!(
            meeting_folder_name("2026-10-27_12-03", Some("Western Digital onboarding")),
            "2026-10-27_12-03 western-digital-onboarding"
        );
        assert_eq!(stamp_of_folder("2026-10-27_12-03 western-digital-onboarding"), Some("2026-10-27_12-03"));
        assert_eq!(stamp_of_folder("Meeting 27_10_26_12_03_07_2026-10-27_16-03"), None);
    }

    #[test]
    fn legacy_folder_names_have_no_stamp_and_v2_names_do() {
        assert!(stamp_of_folder("Meeting 27_10_26_12_03_07_2026-10-27_16-03").is_none());
        assert!(stamp_of_folder("dte-energy-bad-debt 2026-09-30-1502").is_none());
        assert_eq!(stamp_of_folder("2026-10-27_12-03"), Some("2026-10-27_12-03"));
    }

    #[test]
    fn project_of_reads_the_parent_under_root() {
        let root = Path::new("/r");
        assert_eq!(project_of(root, Path::new("/r/dex/2026-10-27_12-03")), "dex");
        assert_eq!(project_of(root, Path::new("/r/Meeting 27_10_26_12_03_07_2026-10-27_16-03")), INBOX);
        assert_eq!(project_of(root, Path::new("/elsewhere/x/y")), INBOX);
    }

    #[test]
    fn list_projects_skips_dotdirs_and_legacy_meetings_and_puts_inbox_first() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("recruiting")).unwrap();
        std::fs::create_dir_all(root.path().join("dex")).unwrap();
        std::fs::create_dir_all(root.path().join(".hidden")).unwrap();
        write_meta(&root.path().join("Meeting 27_10_26_12_03_07_2026-10-27_16-03"), r#"{"version":"1.0"}"#);
        assert_eq!(list_projects(root.path()), vec!["_inbox", "dex", "recruiting"]);
    }

    #[test]
    fn create_meeting_folder_builds_the_layout_and_avoids_collisions() {
        let root = tempfile::tempdir().unwrap();
        let a = create_meeting_folder_with_stamp(root.path(), "dex", "2026-10-27_12-03", Some("Western Digital onboarding"), true).unwrap();
        assert_eq!(a.parent().unwrap().file_name().unwrap(), "dex");
        assert!(a.file_name().unwrap().to_string_lossy().ends_with(" western-digital-onboarding"));
        assert!(a.join(".checkpoints").is_dir());
        let b = create_meeting_folder_with_stamp(root.path(), "dex", "2026-10-27_12-03", Some("Western Digital onboarding"), false).unwrap();
        assert_ne!(a, b, "second folder in the same minute gets a suffix");
        assert!(b.to_string_lossy().ends_with("-2"));
        assert!(b.file_name().unwrap().to_string_lossy().ends_with("western-digital-onboarding-2"));
        assert!(!b.join(".checkpoints").exists());
        let c = create_meeting_folder(root.path(), "dex", Some("Western Digital onboarding"), false).unwrap();
        assert_eq!(c.parent().unwrap().file_name().unwrap(), "dex");
        assert!(c.is_dir());
    }

    #[test]
    fn scan_lists_v2_meetings_and_skips_legacy_folders() {
        let root = tempfile::tempdir().unwrap();
        write_meta(
            &root.path().join("dex").join("2026-10-27_12-03 wd"),
            r#"{"version":"2.0","meeting_id":"meeting-a","project":"dex","title":"WD"}"#,
        );
        write_meta(
            &root.path().join("_inbox").join("2026-10-27_13-00"),
            r#"{"version":"2.0","meeting_id":"meeting-b","meeting_name":"Meeting 27_10_26_13_00_00"}"#,
        );
        write_meta(
            &root.path().join("Meeting 27_10_26_12_03_07_2026-10-27_16-03"),
            r#"{"version":"1.0","meeting_id":null}"#,
        );
        write_meta(
            &root.path().join("dex").join("2026-10-27_14-00"),
            r#"{"version":"2.0","meeting_id":"meeting-c","title":"","meeting_name":"Fallback"}"#,
        );
        let mut entries = scan(root.path());
        entries.sort_by(|x, y| x.meeting_id.cmp(&y.meeting_id));
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].meeting_id, "meeting-a");
        assert_eq!(entries[0].project, "dex");
        assert_eq!(entries[0].title.as_deref(), Some("WD"));
        assert_eq!(entries[1].meeting_id, "meeting-b");
        assert_eq!(entries[1].project, "_inbox");
        assert_eq!(entries[1].title.as_deref(), Some("Meeting 27_10_26_13_00_00"), "falls back to meeting_name");
        assert_eq!(entries[2].title.as_deref(), Some("Fallback"), "empty title is ignored, meeting_name used");
    }

    #[test]
    fn plan_reconcile_updates_only_rows_that_differ() {
        let scanned = vec![
            LibraryEntry { meeting_id: "a".into(), project: "dex".into(), title: Some("WD".into()), folder_path: "/r/dex/x".into() },
            LibraryEntry { meeting_id: "b".into(), project: "_inbox".into(), title: None, folder_path: "/r/_inbox/y".into() },
            LibraryEntry { meeting_id: "c".into(), project: "_inbox".into(), title: Some("C".into()), folder_path: "/r/_inbox/c".into() },
            LibraryEntry { meeting_id: "d".into(), project: "dex".into(), title: None, folder_path: "/r/dex/d".into() },
        ];
        let db = vec![
            ("a".to_string(), Some("/r/_inbox/x".to_string()), "Old".to_string()),
            ("b".to_string(), Some("/r/_inbox/y".to_string()), "Meeting b".to_string()),
            ("d".to_string(), Some("/r/_inbox/d".to_string()), "Keep me".to_string()),
            ("legacy".to_string(), Some("/r/Meeting old".to_string()), "Legacy".to_string()),
        ];
        let updates = plan_reconcile(&db, &scanned);
        assert_eq!(updates.len(), 2, "b matches, c has no row, legacy is not scanned");
        let a = updates.iter().find(|u| u.meeting_id == "a").expect("a is updated");
        assert_eq!(a.folder_path, "/r/dex/x");
        assert_eq!(a.title.as_deref(), Some("WD"));
        let d = updates.iter().find(|u| u.meeting_id == "d").expect("d is updated");
        assert_eq!(d.folder_path, "/r/dex/d");
        assert_eq!(d.title, None, "a missing scanned title must not clear the DB title");
    }

    #[test]
    fn project_names_are_single_normal_components() {
        for ok in ["dex", "_inbox", "flowers-2026", "Recruiting"] {
            assert!(is_valid_project_name(ok), "{}", ok);
        }
        for bad in ["", "  ", ".", "..", "../x", "/tmp/x", "a/b", "a\\b", ".hidden"] {
            assert!(!is_valid_project_name(bad), "{:?}", bad);
        }
    }

    #[test]
    fn create_meeting_folder_rejects_bad_project_names() {
        let root = tempfile::tempdir().unwrap();
        assert!(create_meeting_folder_with_stamp(root.path(), "../escape", "2026-10-27_12-03", None, false).is_err());
        assert!(create_meeting_folder_with_stamp(root.path(), "/tmp/x", "2026-10-27_12-03", None, false).is_err());
        assert!(!root.path().parent().unwrap().join("escape").exists());
    }

    #[test]
    fn trashable_folder_guard() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        let meeting = root.join("proj/2026-10-28_10-00 x");
        std::fs::create_dir_all(&meeting).unwrap();
        let flat = root.join("2026-10-27_09-00 legacy");
        write_meta(&flat, "{}");
        let flat_no_meta = root.join("random");
        std::fs::create_dir_all(&flat_no_meta).unwrap();
        let outside = tempfile::tempdir().unwrap();

        assert!(trashable_folder(root, &meeting));
        assert!(trashable_folder(root, &flat));
        assert!(!trashable_folder(root, root));
        assert!(!trashable_folder(root, &root.join("proj")));
        assert!(!trashable_folder(root, &flat_no_meta));
        assert!(!trashable_folder(root, &meeting.join("sub")));
        assert!(!trashable_folder(root, &root.join("proj/../proj")));
        assert!(!trashable_folder(root, &meeting.join("../../..")));
        assert!(!trashable_folder(root, outside.path()));
    }

    #[test]
    fn trash_missing_folder_is_ok() {
        let t = tempfile::tempdir().unwrap();
        assert!(trash_meeting_folder(t.path(), &t.path().join("proj/nope")).is_ok());
    }
}
