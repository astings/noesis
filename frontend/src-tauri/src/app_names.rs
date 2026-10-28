//! Every place the app's name leaks into a path or identifier lives here, so the rename
//! (and the one-time data migration from the legacy names) has a single source.

/// Secondary config/data dir under `dirs::data_dir()` (models, templates, notification settings).
pub const APP_CONFIG_DIR_NAME: &str = "Noesis";
/// Default recordings root under the platform's video/documents dir.
pub const RECORDINGS_DIR_NAME: &str = "noesis-recordings";
/// Tauri bundle identifier (the app data dir is named after it).
pub const IDENTIFIER: &str = "com.noesis.app";

pub const LEGACY_RECORDINGS_DIR_NAME: &str = "meetily-recordings";
pub const LEGACY_IDENTIFIER: &str = "com.meetily.ai";
pub const LEGACY_CONFIG_DIR_NAME: &str = "Meetily";
