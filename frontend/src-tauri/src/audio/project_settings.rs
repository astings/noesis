//! Per-project settings in `<root>/<project>/project.json` (spec 2026-10-30 section 3).
//! Every key is optional; a missing key means the global setting. Unknown keys survive a
//! write. An unreadable or invalid file counts as empty and never blocks a recording.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Runtime};

use super::recording_preferences::RecordingPreferences;

pub const FILE_NAME: &str = "project.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    /// `auto`, `auto-translate` or a language code; drives the live transcript and the enhance run
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enhance_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enhance_model: Option<String>,
}

/// What one enhance run uses once every level has been applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnhanceSettings {
    pub language: Option<String>,
    pub provider: String,
    pub model: String,
}

fn file_path(root: &Path, project: &str) -> PathBuf {
    root.join(project).join(FILE_NAME)
}

/// The file as a JSON object; `{}` when missing, unreadable or not an object (a warning is logged).
fn read_raw(root: &Path, project: &str) -> serde_json::Map<String, serde_json::Value> {
    let p = file_path(root, project);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Default::default(),
        Err(e) => {
            log::warn!("project settings: cannot read {}: {}", p.display(), e);
            return Default::default();
        }
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(m)) => m,
        Ok(_) => {
            log::warn!("project settings: {} is not a JSON object; ignored", p.display());
            Default::default()
        }
        Err(e) => {
            log::warn!("project settings: {} is not valid JSON ({}); ignored", p.display(), e);
            Default::default()
        }
    }
}

pub fn read(root: &Path, project: &str) -> ProjectSettings {
    serde_json::from_value(serde_json::Value::Object(read_raw(root, project))).unwrap_or_default()
}

/// Write the three keys and keep every other key. Creates the project folder when missing.
pub fn write(root: &Path, project: &str, settings: &ProjectSettings) -> Result<()> {
    if !super::library::is_valid_project_name(project) {
        return Err(anyhow!("Invalid project name '{}'", project));
    }
    let dir = root.join(project);
    std::fs::create_dir_all(&dir)?;
    let mut obj = read_raw(root, project);
    for (key, value) in [
        ("language", &settings.language),
        ("enhance_provider", &settings.enhance_provider),
        ("enhance_model", &settings.enhance_model),
    ] {
        match value {
            Some(v) => {
                obj.insert(key.to_string(), serde_json::Value::String(v.clone()));
            }
            None => {
                obj.remove(key);
            }
        }
    }
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(obj))?;
    let tmp = dir.join(format!(".{}.{}.tmp", FILE_NAME, uuid::Uuid::new_v4()));
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, dir.join(FILE_NAME)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e
    })?;
    Ok(())
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.is_empty())
}

/// The project's language override, if any.
pub fn resolve_language(root: &Path, project: &str) -> Option<String> {
    non_empty(read(root, project).language)
}

/// Per field: the explicit argument, then project.json, then the preferences. Provider and
/// model are taken as a pair from the same level: a project that sets only one of them is
/// treated as unset for both.
pub fn resolve_enhance(
    root: &Path,
    folder: &Path,
    prefs: &RecordingPreferences,
    language: Option<String>,
    provider: Option<String>,
    model: Option<String>,
) -> EnhanceSettings {
    let project = super::library::project_of(root, folder);
    let ps = read(root, &project);
    let language = non_empty(language)
        .or_else(|| non_empty(ps.language.clone()))
        .or_else(|| non_empty(prefs.enhance_language.clone()));
    let pair = |p: Option<String>, m: Option<String>| match (non_empty(p), non_empty(m)) {
        (Some(p), Some(m)) => Some((p, m)),
        _ => None,
    };
    let (provider, model) = pair(provider, model)
        .or_else(|| pair(ps.enhance_provider.clone(), ps.enhance_model.clone()))
        .unwrap_or_else(|| (prefs.enhance_provider.clone(), prefs.enhance_model.clone()));
    EnhanceSettings { language, provider, model }
}

#[tauri::command]
pub async fn get_project_settings<R: Runtime>(app: AppHandle<R>, project: String) -> Result<ProjectSettings, String> {
    let root = super::recording_preferences::recordings_root(&app);
    Ok(read(&root, &project))
}

#[tauri::command]
pub async fn set_project_settings<R: Runtime>(app: AppHandle<R>, project: String, settings: ProjectSettings) -> Result<(), String> {
    let root = super::recording_preferences::recordings_root(&app);
    write(&root, &project, &settings).map_err(|e| e.to_string())
}

/// What an enhance run of this meeting uses when nothing is picked explicitly.
#[tauri::command]
pub async fn resolve_enhance_settings<R: Runtime>(app: AppHandle<R>, meeting_folder_path: String) -> Result<EnhanceSettings, String> {
    let root = super::recording_preferences::recordings_root(&app);
    let prefs = super::recording_preferences::load_recording_preferences(&app).await.unwrap_or_default();
    Ok(resolve_enhance(&root, Path::new(&meeting_folder_path), &prefs, None, None, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs() -> RecordingPreferences {
        RecordingPreferences {
            enhance_language: None,
            enhance_provider: "whisper".into(),
            enhance_model: "large-v3".into(),
            ..RecordingPreferences::default()
        }
    }

    fn settings(language: Option<&str>, provider: Option<&str>, model: Option<&str>) -> ProjectSettings {
        ProjectSettings {
            language: language.map(str::to_string),
            enhance_provider: provider.map(str::to_string),
            enhance_model: model.map(str::to_string),
        }
    }

    #[test]
    fn missing_file_reads_empty_and_resolves_to_preferences() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(read(root.path(), "dex"), ProjectSettings::default());
        assert_eq!(resolve_language(root.path(), "dex"), None);
        let folder = root.path().join("dex").join("2026-10-30_10-00 standup");
        let r = resolve_enhance(root.path(), &folder, &prefs(), None, None, None);
        assert_eq!(r, EnhanceSettings { language: None, provider: "whisper".into(), model: "large-v3".into() });
    }

    #[test]
    fn write_keeps_unknown_keys_and_removes_cleared_ones() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("dex");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE_NAME), r#"{"language":"es","note":"keep me"}"#).unwrap();

        write(root.path(), "dex", &settings(None, Some("parakeet"), Some("parakeet-tdt-0.6b-v3-int8"))).unwrap();

        let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join(FILE_NAME)).unwrap()).unwrap();
        assert_eq!(raw["note"], "keep me");
        assert!(raw.get("language").is_none(), "cleared key removed");
        assert_eq!(raw["enhance_provider"], "parakeet");
        assert_eq!(read(root.path(), "dex").enhance_model.as_deref(), Some("parakeet-tdt-0.6b-v3-int8"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temp file left behind");
    }

    #[test]
    fn write_creates_the_project_folder_and_rejects_bad_names() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "new-case", &settings(Some("auto-translate"), None, None)).unwrap();
        assert!(root.path().join("new-case").is_dir());
        assert_eq!(resolve_language(root.path(), "new-case").as_deref(), Some("auto-translate"));
        assert!(write(root.path(), "../x", &ProjectSettings::default()).is_err());
        assert!(write(root.path(), ".hidden", &ProjectSettings::default()).is_err());
    }

    #[test]
    fn invalid_json_counts_as_empty() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("dex");
        std::fs::create_dir_all(&dir).unwrap();
        for bad in ["{not json", "[1,2]", r#"{"language": 7}"#] {
            std::fs::write(dir.join(FILE_NAME), bad).unwrap();
            assert_eq!(read(root.path(), "dex"), ProjectSettings::default(), "{}", bad);
        }
    }

    #[test]
    fn resolve_order_is_argument_then_project_then_preferences() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "dex", &settings(Some("auto-translate"), Some("parakeet"), Some("p-v3"))).unwrap();
        let folder = root.path().join("dex").join("2026-10-30_10-00 standup");

        let project_wins = resolve_enhance(root.path(), &folder, &prefs(), None, None, None);
        assert_eq!(project_wins, EnhanceSettings { language: Some("auto-translate".into()), provider: "parakeet".into(), model: "p-v3".into() });

        let arg_wins = resolve_enhance(root.path(), &folder, &prefs(), Some("es".into()), Some("whisper".into()), Some("medium".into()));
        assert_eq!(arg_wins, EnhanceSettings { language: Some("es".into()), provider: "whisper".into(), model: "medium".into() });

        // an explicit provider without a model is not a pair: the project pair applies
        let half = resolve_enhance(root.path(), &folder, &prefs(), None, Some("whisper".into()), None);
        assert_eq!((half.provider.as_str(), half.model.as_str()), ("parakeet", "p-v3"));

        // a meeting in another project sees only the preferences
        let other = root.path().join("other").join("2026-10-30_11-00 x");
        let mut p = prefs();
        p.enhance_language = Some("fr".into());
        assert_eq!(
            resolve_enhance(root.path(), &other, &p, None, None, None),
            EnhanceSettings { language: Some("fr".into()), provider: "whisper".into(), model: "large-v3".into() }
        );
    }

    #[test]
    fn a_project_with_model_but_no_provider_falls_through() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "dex", &settings(None, None, Some("medium"))).unwrap();
        let folder = root.path().join("dex").join("2026-10-30_10-00 standup");
        let r = resolve_enhance(root.path(), &folder, &prefs(), None, None, None);
        assert_eq!((r.provider.as_str(), r.model.as_str()), ("whisper", "large-v3"));
    }
}
