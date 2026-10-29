use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

use anyhow::Result;
#[cfg(target_os = "macos")]
use log::error;

#[cfg(target_os = "macos")]
use crate::audio::capture::AudioCaptureBackend;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecordingPreferences {
    pub save_folder: PathBuf,
    pub auto_save: bool,
    pub file_format: String,
    #[serde(default)]
    pub preferred_mic_device: Option<String>,
    #[serde(default)]
    pub preferred_system_device: Option<String>,
    #[cfg(target_os = "macos")]
    #[serde(default)]
    pub system_audio_backend: Option<String>,
    // spec 4.1: run the enhance (full-file re-transcription) job automatically after a recording stops
    #[serde(default = "default_true")]
    pub auto_enhance: bool,
    /// "whisper" or "parakeet"
    #[serde(default = "default_enhance_provider")]
    pub enhance_provider: String,
    /// Model name as listed by `whisper_get_available_models` or `parakeet_get_available_models`
    #[serde(default = "default_enhance_model")]
    pub enhance_model: String,
    /// Language hint for the enhance job. None means auto-detect.
    #[serde(default)]
    pub enhance_language: Option<String>,
    // spec 4.5: Zoom call detection
    /// Stop the recording about 20 seconds after the Zoom meeting ends.
    #[serde(default = "default_true")]
    pub auto_stop_on_call_end: bool,
    /// Start a recording when a Zoom meeting begins. Off by default: recording consent rules differ by country.
    #[serde(default)]
    pub auto_start_on_call: bool,
    /// Name an auto-started recording after the calendar event in progress.
    #[serde(default)]
    pub calendar_meeting_name: bool,
    // spec 4.4: how the owner is named in transcripts ("me" segments)
    #[serde(default = "default_owner_name")]
    pub owner_name: String,
    // spec phase 9 section 1: the summary model names and tags meetings after enhance
    #[serde(default = "default_true")]
    pub auto_title_tags: bool,
    // phase 9 section 3: delete audio once the enhanced transcript is on disk. Off by default: destructive.
    #[serde(default)]
    pub delete_audio_after_enhance: bool,
    #[serde(default = "default_audio_retention_hours")]
    pub audio_retention_hours: u32,
    // whisper "auto" picks among these; empty means unrestricted auto-detect
    #[serde(default = "default_transcription_languages")]
    pub transcription_languages: Vec<String>,
    // system prompt for titles and tags; empty means the built-in one
    #[serde(default)]
    pub title_tags_prompt: String,
}

/// The languages the Models tab offers; keep in sync with `LANGUAGES` in `components/ModelsSettings.tsx`.
pub const KNOWN_LANGUAGES: [&str; 7] = ["en", "fr", "de", "es", "it", "nl", "pt"];

/// ISO 639-1 codes of the given locales (`fr-FR`, `en_US`), deduplicated, known ones only; English when nothing is known.
pub fn languages_from_locales(locales: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in locales {
        let code = l.split(['-', '_']).next().unwrap_or("").to_lowercase();
        if KNOWN_LANGUAGES.contains(&code.as_str()) && !out.contains(&code) {
            out.push(code);
        }
    }
    if out.is_empty() {
        out.push("en".to_string());
    }
    out
}

#[cfg(target_os = "macos")]
fn preferred_locales() -> Vec<String> {
    objc2_foundation::NSLocale::preferredLanguages().iter().map(|s| s.to_string()).collect()
}

#[cfg(not(target_os = "macos"))]
fn preferred_locales() -> Vec<String> {
    Vec::new()
}

fn default_transcription_languages() -> Vec<String> {
    languages_from_locales(&preferred_locales())
}

fn default_true() -> bool {
    true
}

fn default_audio_retention_hours() -> u32 {
    24
}

/// The macOS account's full name; "Me" when it is blank or on other platforms.
fn default_owner_name() -> String {
    #[cfg(target_os = "macos")]
    {
        let name = objc2_foundation::NSFullUserName().to_string();
        if !name.trim().is_empty() {
            return name.trim().to_string();
        }
    }
    "Me".to_string()
}

fn default_enhance_provider() -> String {
    "whisper".to_string()
}

fn default_enhance_model() -> String {
    "large-v3".to_string()
}

impl Default for RecordingPreferences {
    fn default() -> Self {
        Self {
            save_folder: get_default_recordings_folder(),
            auto_save: true,
            file_format: "mp4".to_string(),
            preferred_mic_device: None,
            preferred_system_device: None,
            #[cfg(target_os = "macos")]
            system_audio_backend: Some("coreaudio".to_string()),
            auto_enhance: default_true(),
            enhance_provider: default_enhance_provider(),
            enhance_model: default_enhance_model(),
            enhance_language: None,
            auto_stop_on_call_end: true,
            auto_start_on_call: false,
            calendar_meeting_name: false,
            owner_name: default_owner_name(),
            auto_title_tags: default_true(),
            delete_audio_after_enhance: false,
            audio_retention_hours: default_audio_retention_hours(),
            transcription_languages: default_transcription_languages(),
            title_tags_prompt: String::new(),
        }
    }
}

/// Get the default recordings folder based on platform
pub fn get_default_recordings_folder() -> PathBuf {
    recordings_base_dir().join(crate::app_names::RECORDINGS_DIR_NAME)
}

/// `save_folder` from a stored preferences object; None when absent or blank.
pub fn root_from_preferences_value(value: Option<&serde_json::Value>) -> Option<PathBuf> {
    value?
        .get("save_folder")?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Generic settings section 4: the recordings root the app records into, lists and reconciles.
/// The `save_folder` preference when set, else the platform default. Reads the store directly
/// so callers without async context can use it.
pub fn recordings_root<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.store("recording_preferences.json")
        .ok()
        .and_then(|store| root_from_preferences_value(store.get("preferences").as_ref()))
        .unwrap_or_else(get_default_recordings_folder)
}

/// Parent of the default recordings folder: Music on Windows, Movies on macOS,
/// Documents elsewhere (Documents is also the fallback). The migration reuses it.
pub fn recordings_base_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let preferred = dirs::audio_dir();
    #[cfg(target_os = "macos")]
    let preferred = dirs::video_dir();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let preferred: Option<PathBuf> = None;

    preferred
        .or_else(dirs::document_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Ensure the recordings directory exists
pub fn ensure_recordings_directory(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
        info!("Created recordings directory: {:?}", path);
    }
    Ok(())
}

/// Generate a unique filename for a recording
pub fn generate_recording_filename(format: &str) -> String {
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y%m%d_%H%M%S");
    format!("recording_{}.{}", timestamp, format)
}

/// Load recording preferences from store
pub async fn load_recording_preferences<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<RecordingPreferences> {
    // Try to load from Tauri store
    let store = match app.store("recording_preferences.json") {
        Ok(store) => store,
        Err(e) => {
            warn!("Failed to access store: {}, using defaults", e);
            return Ok(RecordingPreferences::default());
        }
    };

    // Try to get the preferences from store
    let prefs = if let Some(value) = store.get("preferences") {
        match serde_json::from_value::<RecordingPreferences>(value.clone()) {
            Ok(mut p) => {
                info!("Loaded recording preferences from store");
                // Update macOS backend to current value if needed
                #[cfg(target_os = "macos")]
                {
                    let backend = crate::audio::capture::get_current_backend();
                    p.system_audio_backend = Some(backend.to_string());
                }
                p
            }
            Err(e) => {
                warn!("Failed to deserialize preferences: {}, using defaults", e);
                RecordingPreferences::default()
            }
        }
    } else {
        info!("No stored preferences found, using defaults");
        RecordingPreferences::default()
    };

    crate::set_transcription_languages(prefs.transcription_languages.clone());
    info!("Loaded recording preferences: save_folder={:?}, auto_save={}, format={}, mic={:?}, system={:?}",
          prefs.save_folder, prefs.auto_save, prefs.file_format,
          prefs.preferred_mic_device, prefs.preferred_system_device);
    Ok(prefs)
}

/// Save recording preferences to store
pub async fn save_recording_preferences<R: Runtime>(
    app: &AppHandle<R>,
    preferences: &RecordingPreferences,
) -> Result<()> {
    info!("Saving recording preferences: save_folder={:?}, auto_save={}, format={}, mic={:?}, system={:?}",
          preferences.save_folder, preferences.auto_save, preferences.file_format,
          preferences.preferred_mic_device, preferences.preferred_system_device);

    // Get or create store
    let store = app
        .store("recording_preferences.json")
        .map_err(|e| anyhow::anyhow!("Failed to access store: {}", e))?;

    // Serialize preferences to JSON value
    let prefs_value = serde_json::to_value(preferences)
        .map_err(|e| anyhow::anyhow!("Failed to serialize preferences: {}", e))?;

    // Save to store
    store.set("preferences", prefs_value);

    // Persist to disk
    store
        .save()
        .map_err(|e| anyhow::anyhow!("Failed to save store to disk: {}", e))?;

    info!("Successfully persisted recording preferences to disk");
    crate::set_transcription_languages(preferences.transcription_languages.clone());

    // Save backend preference to global config
    #[cfg(target_os = "macos")]
    if let Some(backend_str) = &preferences.system_audio_backend {
        if let Some(backend) = AudioCaptureBackend::from_string(backend_str) {
            info!("Setting audio capture backend to: {:?}", backend);
            crate::audio::capture::set_current_backend(backend);
        }
    }

    // Ensure the directory exists
    ensure_recordings_directory(&preferences.save_folder)?;

    Ok(())
}

/// Tauri commands for recording preferences
#[tauri::command]
pub async fn get_recording_preferences<R: Runtime>(
    app: AppHandle<R>,
) -> Result<RecordingPreferences, String> {
    load_recording_preferences(&app)
        .await
        .map_err(|e| format!("Failed to load recording preferences: {}", e))
}

#[tauri::command]
pub async fn set_recording_preferences<R: Runtime>(
    app: AppHandle<R>,
    preferences: RecordingPreferences,
) -> Result<(), String> {
    save_recording_preferences(&app, &preferences)
        .await
        .map_err(|e| format!("Failed to save recording preferences: {}", e))
}

#[tauri::command]
pub async fn get_default_recordings_folder_path() -> Result<String, String> {
    let path = get_default_recordings_folder();
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_recordings_folder<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let preferences = load_recording_preferences(&app)
        .await
        .map_err(|e| format!("Failed to load preferences: {}", e))?;

    // Ensure directory exists before trying to open it
    ensure_recordings_directory(&preferences.save_folder)
        .map_err(|e| format!("Failed to create directory: {}", e))?;

    let folder_path = preferences.save_folder.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    info!("Opened recordings folder: {}", folder_path);
    Ok(())
}

#[tauri::command]
pub async fn select_recording_folder<R: Runtime>(
    _app: AppHandle<R>,
) -> Result<Option<String>, String> {
    // Use Tauri's dialog to select folder
    // For now, return None - this would need to be implemented with tauri-plugin-dialog
    // when it's available in the Cargo.toml
    warn!("Folder selection not yet implemented - using dialog plugin");
    Ok(None)
}

// Backend selection commands

/// Get available audio capture backends for the current platform
#[tauri::command]
pub async fn get_available_audio_backends() -> Result<Vec<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let backends = crate::audio::capture::get_available_backends();
        Ok(backends.iter().map(|b| b.to_string()).collect())
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Only ScreenCaptureKit available on non-macOS
        Ok(vec!["screencapturekit".to_string()])
    }
}

/// Get current audio capture backend
#[tauri::command]
pub async fn get_current_audio_backend() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let backend = crate::audio::capture::get_current_backend();
        Ok(backend.to_string())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok("screencapturekit".to_string())
    }
}

/// Set audio capture backend
#[tauri::command]
pub async fn set_audio_backend(backend: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use crate::audio::capture::AudioCaptureBackend;
        use crate::audio::permissions::{
            check_screen_recording_permission, request_screen_recording_permission,
        };

        let backend_enum = AudioCaptureBackend::from_string(&backend)
            .ok_or_else(|| format!("Invalid backend: {}", backend))?;

        // If switching to Core Audio, log information about Audio Capture permission
        if backend_enum == AudioCaptureBackend::CoreAudio {
            info!("🔐 Core Audio backend requires Audio Capture permission (macOS 14.4+)");
            info!("📍 Permission dialog will appear automatically when recording starts");

            // Check if permission is already granted (this is informational only)
            if !check_screen_recording_permission() {
                warn!("⚠️  Audio Capture permission may not be granted");

                // Attempt to open System Settings (opens System Settings)
                if let Err(e) = request_screen_recording_permission() {
                    error!("Failed to open System Settings: {}", e);
                }

                return Err(
                    "Core Audio requires Audio Capture permission. \
                    The permission dialog will appear when you start recording. \
                    If already denied, enable it in System Settings → Privacy & Security → Audio Capture, \
                    then restart the app.".to_string()
                );
            }

            info!(
                "✅ Core Audio backend selected - permission check will occur at recording start"
            );
        }

        info!("Setting audio backend to: {:?}", backend_enum);
        crate::audio::capture::set_current_backend(backend_enum);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        if backend != "screencapturekit" {
            return Err(format!(
                "Backend {} not available on this platform",
                backend
            ));
        }
        Ok(())
    }
}

/// Get backend information (name and description)
#[derive(Serialize)]
pub struct BackendInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[tauri::command]
pub async fn get_audio_backend_info() -> Result<Vec<BackendInfo>, String> {
    #[cfg(target_os = "macos")]
    {
        use crate::audio::capture::AudioCaptureBackend;

        let backends = vec![
            BackendInfo {
                id: AudioCaptureBackend::ScreenCaptureKit.to_string(),
                name: AudioCaptureBackend::ScreenCaptureKit.name().to_string(),
                description: AudioCaptureBackend::ScreenCaptureKit
                    .description()
                    .to_string(),
            },
            BackendInfo {
                id: AudioCaptureBackend::CoreAudio.to_string(),
                name: AudioCaptureBackend::CoreAudio.name().to_string(),
                description: AudioCaptureBackend::CoreAudio.description().to_string(),
            },
        ];
        Ok(backends)
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(vec![BackendInfo {
            id: "screencapturekit".to_string(),
            name: "ScreenCaptureKit".to_string(),
            description: "Default system audio capture".to_string(),
        }])
    }
}

/// Native folder picker for the recordings folder.
#[tauri::command]
pub async fn select_directory<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
        .await
        .map_err(|e| e.to_string())?;
    Ok(picked.map(|p| p.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_come_from_the_locales_the_app_knows() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(languages_from_locales(&l(&["fr-FR", "en-US", "zz", "FR_CA"])), vec!["fr", "en"]);
        assert_eq!(languages_from_locales(&l(&["ja-JP"])), vec!["en"], "nothing known falls back to English");
        assert_eq!(languages_from_locales(&[]), vec!["en"]);
        assert!(!RecordingPreferences::default().transcription_languages.is_empty());
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert_eq!(p.title_tags_prompt, "");
    }

    #[test]
    fn root_from_preferences_falls_back_when_missing_or_empty() {
        let v = serde_json::json!({"save_folder": "/Volumes/Work/recordings", "auto_save": true});
        assert_eq!(root_from_preferences_value(Some(&v)), Some(PathBuf::from("/Volumes/Work/recordings")));
        assert_eq!(root_from_preferences_value(Some(&serde_json::json!({"save_folder": "  "}))), None);
        assert_eq!(root_from_preferences_value(Some(&serde_json::json!({"auto_save": true}))), None);
        assert_eq!(root_from_preferences_value(None), None);
    }

    #[test]
    fn default_recordings_folder_is_noesis() {
        assert!(get_default_recordings_folder().ends_with(crate::app_names::RECORDINGS_DIR_NAME));
        assert!(get_default_recordings_folder().ends_with("noesis-recordings"));
    }

    #[test]
    fn old_preferences_json_gets_enhance_defaults() {
        // A store written before this phase has none of the enhance fields.
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert!(p.auto_enhance);
        assert_eq!(p.enhance_provider, "whisper");
        assert_eq!(p.enhance_model, "large-v3");
        assert_eq!(p.enhance_language, None);
    }

    #[test]
    fn default_preferences_enable_enhance_with_large_v3() {
        let p = RecordingPreferences::default();
        assert!(p.auto_enhance);
        assert_eq!(p.enhance_model, "large-v3");
    }

    #[test]
    fn call_detection_defaults_stop_on_and_start_off() {
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert!(p.auto_stop_on_call_end);
        assert!(!p.auto_start_on_call, "a stranger opts in to auto-start");
        let d = RecordingPreferences::default();
        assert!(d.auto_stop_on_call_end);
        assert!(!d.auto_start_on_call);
        assert!(!p.calendar_meeting_name && !d.calendar_meeting_name);
        let stored = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4","auto_start_on_call":true}"#;
        assert!(serde_json::from_str::<RecordingPreferences>(stored).unwrap().auto_start_on_call, "a stored value is kept");
    }

    #[test]
    fn auto_title_tags_defaults_on() {
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert!(p.auto_title_tags);
        assert!(RecordingPreferences::default().auto_title_tags);
    }

    #[test]
    fn audio_deletion_defaults_off_and_owner_name_is_never_blank() {
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert!(!p.delete_audio_after_enhance);
        assert!(!RecordingPreferences::default().delete_audio_after_enhance);
        assert_eq!(p.audio_retention_hours, 24);
        assert!(!RecordingPreferences::default().owner_name.trim().is_empty());
        assert!(!p.owner_name.trim().is_empty());
    }
}
