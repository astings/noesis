# Phase 2: Projects as Folders and the transcript.md Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every meeting lives under `~/Movies/meetily-recordings/<project>/<YYYY-MM-DD_HH-MM title>/`, carries its id and project in `metadata.json` from the first write, gets a `transcript.md` at stop and after enhance, can be moved or renamed from the app, and the SQLite index repairs itself from the filesystem.

**Architecture:** Two new Rust modules do the work: `audio/library.rs` (folder naming, project listing, scan, reconcile, assign) and `audio/transcript_md.rs` (the markdown contract). The recording saver gains project, meeting id and live-model fields and writes metadata v2. The meeting id is minted in Rust at record start and flows to the frontend through the stop event so the SQLite row reuses it. Two Tauri commands (`list_projects`, `assign_meeting`) and one small React component (`ProjectSelect`) cover the UI: a picker next to the record button and a move control in meeting details. Renaming a meeting renames its folder.

**Tech Stack:** Rust (Tauri 2, serde_json, chrono, regex, uuid, sqlx, tempfile for tests), TypeScript (Next.js 14, React 18, `@tauri-apps/api`, sonner, shadcn `Select` and `Input`).

Spec: `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md`, sections 4.2, 4.3, 5.1, 5.2, 5.3.

## Global Constraints

- Additive changes only; new behaviour in the two new modules; edits in existing files stay small and carry a comment naming the spec section (`// spec 4.2`, `// spec 4.3`).
- Recordings root is `get_default_recordings_folder()` (`~/Movies/meetily-recordings` on macOS). The `save_folder` preference stays ignored, as today.
- Folder layout: `<root>/<project>/<stamp>[ <slug>]/` where `stamp` is local time `YYYY-MM-DD_HH-MM` and `slug` is present only when the title is not an auto-generated one. Default project is `_inbox`. Legacy flat folders at the root are never moved; the scan ignores them (no `meeting_id` in their metadata).
- A project is a first-level directory under the root that does not start with a dot and does not contain `metadata.json`. `_inbox` is always listed, first.
- `metadata.json` v2 keeps every v1 key (`version`, `meeting_id`, `meeting_name`, `created_at`, `completed_at`, `duration_seconds`, `devices`, `audio_file`, `transcript_file`, `sample_rate`, `status`, `retranscribed_at`) and adds `project`, `title`, `transcript_md`, `transcription.live {provider, model}`. `version` becomes `"2.0"`. Phase 1's `transcription.enhanced` object must survive every write.
- `transcript.md` format (spec 5.3): YAML frontmatter with keys `title`, `project`, `meeting_id`, `started_at` (ISO 8601 with local offset), `duration_seconds` (integer, omitted when unknown), `quality` (`live` or `enhanced`), `model`; then one line per segment `[mm:ss] text`. No speaker labels in this phase. Nothing paraphrased or dropped.
- The filesystem is the source of truth. The reconcile scan updates `meetings.folder_path` and `meetings.title` from `metadata.json`; metadata wins over the database.
- No em-dashes in any text, code comment or UI string.
- Rust tests: `cd frontend/src-tauri && cargo test --lib <module>`. Full suite baseline on `main`: 252 passed, 0 failed, 3 ignored; upstream warnings exist, add none. Type check: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json` (prints nothing on success). Do not run any `pnpm` command (system pnpm is the wrong major for this repo). No ESLint config exists.
- Branch `phase-2/projects`, one commit per task, no push.

## File Structure

| File | Responsibility |
| --- | --- |
| `frontend/src-tauri/src/audio/library.rs` (create) | Folder naming (`slug`, `is_auto_title`, `meeting_folder_name`), `project_of`, `list_projects`, `create_meeting_folder`, `scan`, `plan_reconcile`, `reconcile`, `assign`, commands `list_projects_command` and `assign_meeting` |
| `frontend/src-tauri/src/audio/transcript_md.rs` (create) | `render_transcript_md`, `write_transcript_md`, `format_mmss`, `local_iso` |
| `frontend/src-tauri/src/audio/mod.rs` (modify) | Declare both modules |
| `frontend/src-tauri/src/audio/recording_saver.rs` (modify) | Metadata v2 fields, `set_project`, `set_meeting_id`, `set_live_transcription`, project folder, transcript.md at stop |
| `frontend/src-tauri/src/audio/recording_manager.rs` (modify) | Pass-through setters and `get_meeting_id` |
| `frontend/src-tauri/src/audio/recording_commands.rs` (modify) | Mint meeting id, read live model, accept `project`, emit `meeting_id` on start and stop |
| `frontend/src-tauri/src/lib.rs` (modify) | `project` argument on the start command, register two commands, startup reconcile |
| `frontend/src-tauri/src/api/api.rs` (modify) | `meeting_id` on `api_save_transcript`, reconcile at the top of `api_get_meetings`, `api_save_meeting_title` through `library::assign` |
| `frontend/src-tauri/src/database/repositories/transcript.rs` (modify) | `save_transcript` reuses a given id |
| `frontend/src-tauri/src/audio/retranscription.rs` (modify) | `running_meeting_id()`, re-resolve folder after the queue slot, write transcript.md (enhanced), `transcript_md` key in metadata |
| `frontend/src/lib/projects.ts` (create) | `getLastProject`, `setLastProject`, `projectOfFolder` |
| `frontend/src/components/ProjectSelect.tsx` (create) | Dropdown over `list_projects` with an inline "new project" input |
| `frontend/src/app/page.tsx` (modify) | Picker next to the record controls |
| `frontend/src/services/recordingService.ts`, `frontend/src/hooks/useRecordingStart.ts` (modify) | Pass `project` to the start command |
| `frontend/src/services/storageService.ts`, `frontend/src/hooks/useRecordingStop.ts` (modify) | Pass the Rust-minted `meetingId` to the save |
| `frontend/src/components/MeetingDetails/TranscriptButtonGroup.tsx` (modify) | Move-to-project control |

---

### Task 1: `library.rs` pure helpers

**Files:**
- Create: `frontend/src-tauri/src/audio/library.rs`
- Modify: `frontend/src-tauri/src/audio/mod.rs` (after `pub mod enhance_status;`)

**Interfaces:**
- Produces (all `pub`, used by Tasks 3, 5, 6 and 7):
  - `pub const INBOX: &str = "_inbox";`
  - `pub fn slug(title: &str) -> String`
  - `pub fn is_auto_title(title: &str) -> bool`
  - `pub fn local_stamp(now: chrono::DateTime<chrono::Local>) -> String`
  - `pub fn stamp_of_folder(folder_name: &str) -> Option<&str>`
  - `pub fn meeting_folder_name(stamp: &str, title: Option<&str>) -> String`
  - `pub fn project_of(root: &Path, folder: &Path) -> String`
  - `pub fn list_projects(root: &Path) -> Vec<String>`
  - `pub fn create_meeting_folder(root: &Path, project: &str, title: Option<&str>, create_checkpoints: bool) -> anyhow::Result<PathBuf>`
  - `pub struct LibraryEntry { pub meeting_id: String, pub project: String, pub title: Option<String>, pub folder_path: String }`
  - `pub fn scan(root: &Path) -> Vec<LibraryEntry>`
  - `pub struct ReconcileUpdate { pub meeting_id: String, pub folder_path: String, pub title: Option<String> }`
  - `pub fn plan_reconcile(db_rows: &[(String, Option<String>, String)], scanned: &[LibraryEntry]) -> Vec<ReconcileUpdate>` where a row is `(id, folder_path, title)`.

- [ ] **Step 1: Write the failing tests**

Create `frontend/src-tauri/src/audio/library.rs` with the header and the tests only:

```rust
//! spec 4.2: the meeting library on disk. Folder naming, project listing, a
//! scan of `<root>/<project>/<meeting>/metadata.json`, and the reconcile that
//! repairs the SQLite index from it. The filesystem is the source of truth.

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

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
        let a = create_meeting_folder(root.path(), "dex", Some("Western Digital onboarding"), true).unwrap();
        assert_eq!(a.parent().unwrap().file_name().unwrap(), "dex");
        assert!(a.file_name().unwrap().to_string_lossy().ends_with(" western-digital-onboarding"));
        assert!(a.join(".checkpoints").is_dir());
        let b = create_meeting_folder(root.path(), "dex", Some("Western Digital onboarding"), false).unwrap();
        assert_ne!(a, b, "second folder in the same minute gets a suffix");
        assert!(b.to_string_lossy().ends_with("-2"));
        assert!(!b.join(".checkpoints").exists());
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
        let mut entries = scan(root.path());
        entries.sort_by(|x, y| x.meeting_id.cmp(&y.meeting_id));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].meeting_id, "meeting-a");
        assert_eq!(entries[0].project, "dex");
        assert_eq!(entries[0].title.as_deref(), Some("WD"));
        assert_eq!(entries[1].project, "_inbox");
        assert_eq!(entries[1].title.as_deref(), Some("Meeting 27_10_26_13_00_00"), "falls back to meeting_name");
    }

    #[test]
    fn plan_reconcile_updates_only_rows_that_differ() {
        let scanned = vec![
            LibraryEntry { meeting_id: "a".into(), project: "dex".into(), title: Some("WD".into()), folder_path: "/r/dex/x".into() },
            LibraryEntry { meeting_id: "b".into(), project: "_inbox".into(), title: None, folder_path: "/r/_inbox/y".into() },
            LibraryEntry { meeting_id: "c".into(), project: "_inbox".into(), title: Some("C".into()), folder_path: "/r/_inbox/c".into() },
        ];
        let db = vec![
            ("a".to_string(), Some("/r/_inbox/x".to_string()), "Old".to_string()),
            ("b".to_string(), Some("/r/_inbox/y".to_string()), "Meeting b".to_string()),
            ("legacy".to_string(), Some("/r/Meeting old".to_string()), "Legacy".to_string()),
        ];
        let updates = plan_reconcile(&db, &scanned);
        assert_eq!(updates.len(), 1, "b matches, c has no row, legacy is not scanned");
        assert_eq!(updates[0].meeting_id, "a");
        assert_eq!(updates[0].folder_path, "/r/dex/x");
        assert_eq!(updates[0].title.as_deref(), Some("WD"));
    }
}
```

Add to `frontend/src-tauri/src/audio/mod.rs` after `pub mod enhance_status;`:

```rust
// spec 4.2: meeting library on disk (folders, projects, scan, reconcile)
pub mod library;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib library`
Expected: compile errors, `cannot find function slug`, `INBOX`, and so on.

- [ ] **Step 3: Implement the helpers**

Insert above the `#[cfg(test)]` block:

```rust
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

/// Create `<root>/<project>/<stamp>[ <slug>]/` (and `.checkpoints/` when asked),
/// adding `-2`, `-3`, ... when a folder with that name already exists.
pub fn create_meeting_folder(
    root: &Path,
    project: &str,
    title: Option<&str>,
    create_checkpoints: bool,
) -> Result<PathBuf> {
    let base = meeting_folder_name(&local_stamp(chrono::Local::now()), title);
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
        .or_else(|| meta.get("meeting_name").and_then(|v| v.as_str()))
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend/src-tauri && cargo test --lib library`
Expected: `test result: ok. 8 passed`. If `slug("x" * 100).len()` is 59 because of the trailing trim, adjust the loop so it stops at 60 collected characters before trimming only dashes; the assertion stays `60`.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/audio/library.rs frontend/src-tauri/src/audio/mod.rs
git commit -m "feat(library): folder naming, project listing, scan and reconcile planning"
```

---

### Task 2: `transcript_md.rs`

**Files:**
- Create: `frontend/src-tauri/src/audio/transcript_md.rs`
- Modify: `frontend/src-tauri/src/audio/mod.rs` (after `pub mod library;`)

**Interfaces:**
- Produces:
  - `pub struct TranscriptMdMeta<'a> { pub title: &'a str, pub project: &'a str, pub meeting_id: &'a str, pub started_at: &'a str, pub duration_seconds: Option<f64>, pub quality: &'a str, pub model: &'a str }`
  - `pub fn format_mmss(seconds: f64) -> String`
  - `pub fn local_iso(rfc3339: &str) -> String` (converts a UTC RFC 3339 string to local time with offset; returns the input unchanged if it does not parse)
  - `pub fn render_transcript_md(meta: &TranscriptMdMeta, segments: &[(f64, String)]) -> String`
  - `pub fn write_transcript_md(folder: &Path, meta: &TranscriptMdMeta, segments: &[(f64, String)]) -> anyhow::Result<()>` (atomic write to `transcript.md`)
  - Tasks 3 and 6 call `write_transcript_md`.

- [ ] **Step 1: Write the failing tests**

Create the file with header and tests:

```rust
//! spec 4.3 / 5.3: the transcript.md contract. One function renders it, one
//! writes it atomically. Consumers (the wikis, the MCP server) read this file
//! and ignore frontmatter keys they do not know.

use anyhow::Result;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mmss_pads_minutes_and_seconds() {
        assert_eq!(format_mmss(0.0), "00:00");
        assert_eq!(format_mmss(65.4), "01:05");
        assert_eq!(format_mmss(3599.9), "59:59");
        assert_eq!(format_mmss(3600.0), "60:00");
    }

    #[test]
    fn renders_frontmatter_and_one_line_per_segment() {
        let meta = TranscriptMdMeta {
            title: "Western \"Digital\" onboarding",
            project: "dex",
            meeting_id: "meeting-a",
            started_at: "2026-10-26T14:30:05-04:00",
            duration_seconds: Some(2555.2),
            quality: "enhanced",
            model: "whisper large-v3",
        };
        let segments = vec![(0.0, "Thank you. Hello.".to_string()), (4.2, "Yes, it's been\na long time.".to_string())];
        let md = render_transcript_md(&meta, &segments);
        let expected = "---\n\
title: \"Western \\\"Digital\\\" onboarding\"\n\
project: dex\n\
meeting_id: meeting-a\n\
started_at: 2026-10-26T14:30:05-04:00\n\
duration_seconds: 2555\n\
quality: enhanced\n\
model: whisper large-v3\n\
---\n\
[00:00] Thank you. Hello.\n\
[00:04] Yes, it's been a long time.\n";
        assert_eq!(md, expected);
    }

    #[test]
    fn omits_duration_when_unknown_and_handles_no_segments() {
        let meta = TranscriptMdMeta {
            title: "t", project: "_inbox", meeting_id: "m", started_at: "2026-10-27T12:03:09+00:00",
            duration_seconds: None, quality: "live", model: "parakeet parakeet-tdt-0.6b-v3-int8",
        };
        let md = render_transcript_md(&meta, &[]);
        assert!(!md.contains("duration_seconds"));
        assert!(md.ends_with("---\n"));
    }

    #[test]
    fn write_is_atomic_and_readable() {
        let dir = tempfile::tempdir().unwrap();
        let meta = TranscriptMdMeta {
            title: "t", project: "p", meeting_id: "m", started_at: "2026-10-27T12:03:09+00:00",
            duration_seconds: Some(1.0), quality: "live", model: "x",
        };
        write_transcript_md(dir.path(), &meta, &[(0.0, "hi".into())]).unwrap();
        let text = std::fs::read_to_string(dir.path().join("transcript.md")).unwrap();
        assert!(text.contains("[00:00] hi"));
        assert!(!dir.path().join(".transcript.md.tmp").exists());
    }

    #[test]
    fn local_iso_converts_utc_and_passes_through_garbage() {
        let out = local_iso("2026-10-27T16:03:09.210153+00:00");
        assert!(out.starts_with("2026-10-27T"), "{}", out);
        assert!(!out.ends_with("Z") || out.ends_with("+00:00"), "has an offset: {}", out);
        assert_eq!(local_iso("not a date"), "not a date");
    }
}
```

Add to `audio/mod.rs` after `pub mod library;`:

```rust
// spec 4.3: transcript.md contract
pub mod transcript_md;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib transcript_md`
Expected: compile errors, missing `format_mmss`, `TranscriptMdMeta`.

- [ ] **Step 3: Implement**

Insert above the tests:

```rust
pub struct TranscriptMdMeta<'a> {
    pub title: &'a str,
    pub project: &'a str,
    pub meeting_id: &'a str,
    /// ISO 8601 with offset, already in the zone you want shown
    pub started_at: &'a str,
    pub duration_seconds: Option<f64>,
    /// "live" or "enhanced"
    pub quality: &'a str,
    /// e.g. "whisper large-v3"
    pub model: &'a str,
}

/// `[mm:ss]` style time; minutes are not wrapped at 60 so a 2 hour call reads 120:00.
pub fn format_mmss(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// A UTC RFC 3339 string rendered in local time with its offset. Unparseable input is returned as is.
pub fn local_iso(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|d| d.with_timezone(&chrono::Local).to_rfc3339_opts(chrono::SecondsFormat::Secs, false))
        .unwrap_or_else(|_| rfc3339.to_string())
}

fn yaml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn render_transcript_md(meta: &TranscriptMdMeta, segments: &[(f64, String)]) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("title: {}\n", yaml_string(meta.title)));
    out.push_str(&format!("project: {}\n", meta.project));
    out.push_str(&format!("meeting_id: {}\n", meta.meeting_id));
    out.push_str(&format!("started_at: {}\n", meta.started_at));
    if let Some(d) = meta.duration_seconds {
        out.push_str(&format!("duration_seconds: {}\n", d.round() as i64));
    }
    out.push_str(&format!("quality: {}\n", meta.quality));
    out.push_str(&format!("model: {}\n", meta.model));
    out.push_str("---\n");
    for (start, text) in segments {
        let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if one_line.is_empty() {
            continue;
        }
        out.push_str(&format!("[{}] {}\n", format_mmss(*start), one_line));
    }
    out
}

/// Write `transcript.md` through a temp file and rename.
pub fn write_transcript_md(folder: &Path, meta: &TranscriptMdMeta, segments: &[(f64, String)]) -> Result<()> {
    let tmp = folder.join(".transcript.md.tmp");
    std::fs::write(&tmp, render_transcript_md(meta, segments))?;
    std::fs::rename(&tmp, folder.join("transcript.md"))?;
    Ok(())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend/src-tauri && cargo test --lib transcript_md`
Expected: `test result: ok. 5 passed`. If the second test fails on `duration_seconds: 2555` because `2555.2.round()` is 2555, that is the intended value; check the exact expected string for a stray space instead.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/audio/transcript_md.rs frontend/src-tauri/src/audio/mod.rs
git commit -m "feat(transcript-md): render and write the transcript.md contract"
```

---

### Task 3: Record into project folders with metadata v2, a Rust-minted meeting id, and transcript.md at stop

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_saver.rs` (struct fields, setters, `initialize_meeting_folder`, `stop_and_save`)
- Modify: `frontend/src-tauri/src/audio/recording_manager.rs` (pass-through setters, `get_meeting_id`)
- Modify: `frontend/src-tauri/src/audio/recording_commands.rs` (both start functions, `recording-started` and `recording-stopped` payloads)
- Modify: `frontend/src-tauri/src/lib.rs` (`start_recording_with_devices_and_meeting` gains `project`; `start_recording` passes `None`)

**Interfaces:**
- Consumes: `library::{INBOX, create_meeting_folder}`, `transcript_md::{TranscriptMdMeta, write_transcript_md, local_iso}` from Tasks 1 and 2.
- Produces:
  - `RecordingSaver::set_project(&mut self, project: Option<String>)`, `set_meeting_id(&mut self, id: Option<String>)`, `set_live_transcription(&mut self, provider_model: Option<(String, String)>)`, `get_meeting_id(&self) -> Option<String>`; the same four on `RecordingManager`.
  - `recording_commands::start_recording_with_meeting_name(app, meeting_name, project: Option<String>)` and `start_recording_with_devices_and_meeting(app, mic, sys, meeting_name, project: Option<String>)`.
  - Tauri command `start_recording_with_devices_and_meeting(mic_device_name, system_device_name, meeting_name, project: Option<String>)`.
  - Event `recording-started` gains `meeting_id`; event `recording-stopped` gains `meeting_id` (Task 4 reads it).
  - `metadata.json` written as v2 (spec 5.2 keys listed in Global Constraints).

- [ ] **Step 1: Saver fields and setters**

In `recording_saver.rs`, extend `MeetingMetadata` (keep every existing field):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingMetadata {
    pub version: String,
    pub meeting_id: Option<String>,
    pub meeting_name: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<f64>,
    pub devices: DeviceInfo,
    pub audio_file: String,
    pub transcript_file: String,
    pub sample_rate: u32,
    pub status: String,  // "recording", "completed", "error"
    // spec 5.2: v2 keys. All optional so v1 files still deserialize.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub transcript_md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcription: Option<serde_json::Value>,
}
```

Add three fields to `RecordingSaver` and initialize them to `None` in `new()`:

```rust
    // spec 4.2
    project: Option<String>,
    meeting_id: Option<String>,
    live_transcription: Option<(String, String)>,
```

Add setters next to `set_meeting_name`:

```rust
    /// spec 4.2: project folder the meeting is recorded into (None means `_inbox`)
    pub fn set_project(&mut self, project: Option<String>) {
        self.project = project.filter(|p| !p.trim().is_empty());
    }

    /// spec 4.2: the meeting id is minted at record start so metadata.json carries it from the first write
    pub fn set_meeting_id(&mut self, id: Option<String>) {
        self.meeting_id = id;
    }

    pub fn get_meeting_id(&self) -> Option<String> {
        self.meeting_id.clone()
    }

    /// spec 5.2: provider and model of the live engine, recorded under transcription.live
    pub fn set_live_transcription(&mut self, provider_model: Option<(String, String)>) {
        self.live_transcription = provider_model;
    }
```

- [ ] **Step 2: Project folder and v2 metadata at initialization**

In `initialize_meeting_folder`, replace

```rust
        let base_folder = super::recording_preferences::get_default_recordings_folder();

        // Create meeting folder structure (with or without .checkpoints/ subdirectory)
        let meeting_folder = create_meeting_folder(&base_folder, meeting_name, create_checkpoints)?;
```

with

```rust
        let base_folder = super::recording_preferences::get_default_recordings_folder();

        // spec 4.2: <root>/<project>/<stamp slug>/
        let project = self.project.clone().unwrap_or_else(|| super::library::INBOX.to_string());
        let meeting_folder = super::library::create_meeting_folder(
            &base_folder,
            &project,
            Some(meeting_name),
            create_checkpoints,
        )?;
```

and replace the `MeetingMetadata { ... }` literal with:

```rust
        let metadata = MeetingMetadata {
            version: "2.0".to_string(),
            meeting_id: self.meeting_id.clone(),
            meeting_name: Some(meeting_name.to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            duration_seconds: None,
            devices: DeviceInfo {
                microphone: None,
                system_audio: None,
            },
            audio_file: if create_checkpoints { "audio.mp4".to_string() } else { "".to_string() },
            transcript_file: "transcripts.json".to_string(),
            sample_rate: 48000,
            status: "recording".to_string(),
            project: Some(project.clone()),
            title: Some(meeting_name.to_string()),
            transcript_md: Some("transcript.md".to_string()),
            transcription: self.live_transcription.as_ref().map(|(provider, model)| {
                serde_json::json!({ "live": { "provider": provider, "model": model } })
            }),
        };
```

Remove the now unused `use super::audio_processing::create_meeting_folder;` import at the top of the file (otherwise `cargo check` warns). Leave `audio_processing::create_meeting_folder` itself in place.

- [ ] **Step 3: transcript.md at stop**

In `stop_and_save`, inside the block `if let (Some(folder), Some(mut metadata)) = (&self.meeting_folder, self.metadata.clone())`, right after the line `info!("✅ Metadata updated with duration: {:?}s", metadata.duration_seconds);`, add:

```rust
            // spec 4.3: transcript.md, quality live. Enhance rewrites it later.
            let segments_for_md: Vec<(f64, String)> = self
                .transcript_segments
                .lock()
                .map(|s| s.iter().map(|t| (t.audio_start_time, t.text.clone())).collect())
                .unwrap_or_default();
            let live_model = metadata
                .transcription
                .as_ref()
                .and_then(|t| t.get("live"))
                .map(|l| {
                    format!(
                        "{} {}",
                        l.get("provider").and_then(|v| v.as_str()).unwrap_or(""),
                        l.get("model").and_then(|v| v.as_str()).unwrap_or("")
                    )
                    .trim()
                    .to_string()
                })
                .unwrap_or_else(|| "live".to_string());
            let started_at = super::transcript_md::local_iso(&metadata.created_at);
            let md_meta = super::transcript_md::TranscriptMdMeta {
                title: metadata.title.as_deref().or(metadata.meeting_name.as_deref()).unwrap_or(""),
                project: metadata.project.as_deref().unwrap_or(super::library::INBOX),
                meeting_id: metadata.meeting_id.as_deref().unwrap_or(""),
                started_at: &started_at,
                duration_seconds: metadata.duration_seconds,
                quality: "live",
                model: &live_model,
            };
            if let Err(e) = super::transcript_md::write_transcript_md(folder, &md_meta, &segments_for_md) {
                warn!("Failed to write transcript.md: {}", e);
            }
```

This sits before the `recording-saved` emit and before `segments.clear()`, so the segments are still available.

- [ ] **Step 4: Manager pass-throughs**

In `recording_manager.rs`, next to `set_meeting_name`:

```rust
    // spec 4.2
    pub fn set_project(&mut self, project: Option<String>) {
        self.recording_saver.set_project(project);
    }

    pub fn set_meeting_id(&mut self, id: Option<String>) {
        self.recording_saver.set_meeting_id(id);
    }

    pub fn get_meeting_id(&self) -> Option<String> {
        self.recording_saver.get_meeting_id()
    }

    pub fn set_live_transcription(&mut self, provider_model: Option<(String, String)>) {
        self.recording_saver.set_live_transcription(provider_model);
    }
```

- [ ] **Step 5: Mint the id and accept `project` in both start functions**

In `recording_commands.rs`, change the two signatures:

```rust
pub async fn start_recording_with_meeting_name<R: Runtime>(
    app: AppHandle<R>,
    meeting_name: Option<String>,
    project: Option<String>,
) -> Result<(), String> {
```

```rust
pub async fn start_recording_with_devices_and_meeting<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
    project: Option<String>,
) -> Result<(), String> {
```

`start_recording(app)` at the top of the file calls `start_recording_with_meeting_name(app, None)`; make it `start_recording_with_meeting_name(app, None, None).await`. Grep the crate for every other caller of these two functions (`lib.rs` has at least two, `tray.rs` may have one) and pass `None` for `project` everywhere except the Tauri command in Step 6.

In both functions, directly after `manager.set_meeting_name(Some(effective_meeting_name));`, add:

```rust
    // spec 4.2: mint the id here so metadata.json carries it from the first write;
    // the frontend reuses it when it saves the SQLite row.
    let meeting_id = format!("meeting-{}", uuid::Uuid::new_v4());
    manager.set_meeting_id(Some(meeting_id.clone()));
    manager.set_project(project.clone());
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        match crate::database::repositories::setting::SettingsRepository::get_transcript_config(state.db_manager.pool()).await {
            Ok(Some(cfg)) => manager.set_live_transcription(Some((cfg.provider.clone(), cfg.model.clone()))),
            Ok(None) => manager.set_live_transcription(Some(("parakeet".to_string(), crate::config::DEFAULT_PARAKEET_MODEL.to_string()))),
            Err(e) => warn!("Could not read transcript config for metadata: {}", e),
        }
    }
```

`SettingsRepository` is the struct in `database/repositories/setting.rs` that owns `get_transcript_config` (the one `api_get_transcript_config` in `api/api.rs` calls; use that exact name) and `TranscriptSetting` has `provider` and `model` string fields. `try_state` needs `use tauri::Manager;` at the top of the file if it is not already imported.

In both `recording-started` emits add `"meeting_id": meeting_id,` to the JSON object.

- [ ] **Step 6: Carry the id through stop**

In `stop_recording`, where `let meeting_folder = manager.get_meeting_folder(); let meeting_name = manager.get_meeting_name();` are extracted, also extract `let meeting_id = manager.get_meeting_id();` and return it from both arms of that `if let` (change the tuple to three elements, `(None, None, None)` in the else arm). Then in the `recording-stopped` emit add `"meeting_id": meeting_id,` next to `folder_path` and `meeting_name`.

- [ ] **Step 7: The Tauri command**

In `lib.rs`, change the command:

```rust
#[tauri::command]
async fn start_recording_with_devices_and_meeting<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
    project: Option<String>, // spec 4.2
) -> Result<(), String> {
```

and forward `project.clone()` to both `audio::recording_commands::start_recording_with_meeting_name(app.clone(), meeting_name, project.clone())` and `audio::recording_commands::start_recording_with_devices_and_meeting(app.clone(), mic_device_name, system_device_name, meeting_name, project)`. The `start_recording` command at `lib.rs:141` keeps its signature and passes `None` for project wherever it calls into `recording_commands`.

- [ ] **Step 8: Build and test**

Run: `cd frontend/src-tauri && cargo check 2>&1 | grep -E "^(error|warning: unused)" ; cargo test --lib library && cargo test --lib transcript_md`
Expected: no errors, no new unused warnings, both module suites green. Then start the app (`cd frontend && pnpm run tauri dev` is forbidden in this environment; the owner runs it) is not required for this task; the acceptance task covers it.

- [ ] **Step 9: Commit**

```bash
git add frontend/src-tauri/src/audio/recording_saver.rs frontend/src-tauri/src/audio/recording_manager.rs frontend/src-tauri/src/audio/recording_commands.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(library): record into project folders with metadata v2, minted meeting id and transcript.md"
```

---

### Task 4: The SQLite row reuses the Rust-minted id

**Files:**
- Modify: `frontend/src-tauri/src/database/repositories/transcript.rs:13-20`
- Modify: `frontend/src-tauri/src/api/api.rs` (`api_save_transcript`)
- Modify: `frontend/src/services/storageService.ts` (`saveMeeting`)
- Modify: `frontend/src/hooks/useRecordingStop.ts` (the `recording-stopped` listener and the save call)

**Interfaces:**
- Consumes: `recording-stopped` payload field `meeting_id` (Task 3).
- Produces: `TranscriptsRepository::save_transcript(pool, meeting_title, transcripts, folder_path, meeting_id: Option<String>) -> Result<String, SqlxError>`; Tauri command `api_save_transcript(meeting_title, transcripts, folder_path, meeting_id: Option<String>, auth_token)`; `storageService.saveMeeting(title, transcripts, folderPath, meetingId: string | null)`.

- [ ] **Step 1: Repository**

```rust
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
        meeting_id: Option<String>, // spec 4.2: reuse the id minted at record start
    ) -> Result<String, SqlxError> {
        let meeting_id = meeting_id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("meeting-{}", Uuid::new_v4()));
```

Grep for other callers of `save_transcript` (the audio import path `audio/import.rs` may call it) and pass `None`.

- [ ] **Step 2: Command**

In `api_save_transcript`, add the parameter `meeting_id: Option<String>,` after `folder_path: Option<String>,` and pass it as the fifth argument to `TranscriptsRepository::save_transcript(pool, &meeting_title, &transcripts_to_save, folder_path, meeting_id)`. Include it in the `log_info!` line.

- [ ] **Step 3: Frontend service**

```ts
  async saveMeeting(
    meetingTitle: string,
    transcripts: Transcript[],
    folderPath: string | null,
    meetingId: string | null = null, // spec 4.2
  ): Promise<SaveMeetingResponse> {
    return invoke<SaveMeetingResponse>('api_save_transcript', {
      meetingTitle,
      transcripts,
      folderPath,
      meetingId,
    });
  }
```

- [ ] **Step 4: Stop hook**

In `useRecordingStop.ts`, the `recording-stopped` listener payload type gains `meeting_id?: string;` and, next to the two existing `sessionStorage.setItem` calls, add:

```ts
            if (event.payload.meeting_id) {
              sessionStorage.setItem('last_recording_meeting_id', event.payload.meeting_id); // spec 4.2
            }
```

In `handleRecordingStop`, next to `const folderPath = sessionStorage.getItem('last_recording_folder_path');` add `const mintedMeetingId = sessionStorage.getItem('last_recording_meeting_id');` and pass it as the fourth argument of `storageService.saveMeeting(...)`. Where the hook removes `last_recording_folder_path` from sessionStorage, also remove `last_recording_meeting_id`.

- [ ] **Step 5: Verify**

Run: `cd frontend/src-tauri && cargo check 2>&1 | grep -E "^error" ; cd ../ && node_modules/.bin/tsc --noEmit -p tsconfig.json`
Expected: no errors, tsc silent.

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/src/database/repositories/transcript.rs frontend/src-tauri/src/api/api.rs frontend/src/services/storageService.ts frontend/src/hooks/useRecordingStop.ts
git commit -m "feat(library): SQLite row reuses the meeting id minted at record start"
```

---

### Task 5: Reconcile the index from the filesystem

**Files:**
- Modify: `frontend/src-tauri/src/audio/library.rs` (add `reconcile`)
- Modify: `frontend/src-tauri/src/api/api.rs` (`api_get_meetings`)
- Modify: `frontend/src-tauri/src/lib.rs` (setup, after database init)

**Interfaces:**
- Consumes: `plan_reconcile`, `scan` (Task 1); `MeetingModel { id, title, folder_path }`.
- Produces: `pub async fn reconcile(pool: &sqlx::SqlitePool, root: &Path) -> anyhow::Result<usize>` (number of rows updated).

- [ ] **Step 1: Implement `reconcile`**

Add to `library.rs` below `plan_reconcile`:

```rust
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
```

- [ ] **Step 2: Run it on sidebar refresh and at startup**

In `api/api.rs`, at the top of `api_get_meetings` after `let pool = state.db_manager.pool();`:

```rust
    // spec 4.2: the filesystem is the source of truth; repair the index before listing
    if let Err(e) = crate::audio::library::reconcile(pool, &crate::audio::get_default_recordings_folder()).await {
        log_warn!("Library reconcile failed: {}", e);
    }
```

(`get_default_recordings_folder` is re-exported from `audio/mod.rs`.)

In `lib.rs` setup, right after the `initialize_database_on_startup` block (`.expect("Failed to initialize database");`), add:

```rust
            // spec 4.2: repair the meeting index from disk once at startup
            if let Some(state) = _app.handle().try_state::<state::AppState>() {
                let pool = state.db_manager.pool();
                match tauri::async_runtime::block_on(audio::library::reconcile(pool, &audio::get_default_recordings_folder())) {
                    Ok(n) => log::info!("Library reconcile at startup: {} rows updated", n),
                    Err(e) => log::warn!("Library reconcile at startup failed: {}", e),
                }
            }
```

Use the module path that `lib.rs` already uses for `AppState` (grep `AppState` in `lib.rs` for the right `use`).

- [ ] **Step 3: Verify**

Run: `cd frontend/src-tauri && cargo check 2>&1 | grep -E "^error" ; cargo test --lib library`
Expected: no errors; 8 tests still pass.

- [ ] **Step 4: Commit**

```bash
git add frontend/src-tauri/src/audio/library.rs frontend/src-tauri/src/api/api.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(library): reconcile the SQLite index from metadata.json at startup and on refresh"
```

---

### Task 6: `assign_meeting`, `list_projects`, rename renames the folder, enhance writes transcript.md

**Files:**
- Modify: `frontend/src-tauri/src/audio/library.rs` (add `assign`, two commands)
- Modify: `frontend/src-tauri/src/audio/retranscription.rs` (`running_meeting_id()`, folder re-resolve, transcript.md, `transcript_md` key)
- Modify: `frontend/src-tauri/src/api/api.rs` (`api_save_meeting_title`)
- Modify: `frontend/src-tauri/src/lib.rs` (register two commands)

**Interfaces:**
- Consumes: `recording_commands::is_recording().await -> bool`, `recording_commands::get_meeting_folder_path().await -> Result<Option<String>, String>` (both exist), `enhance_status::read_enhance_status`.
- Produces:
  - `pub fn retranscription::running_meeting_id() -> Option<String>`
  - `pub async fn library::assign(pool, root: &Path, meeting_id: &str, project: Option<&str>, title: Option<&str>) -> Result<PathBuf, String>`
  - Tauri commands `list_projects() -> Vec<String>` and `assign_meeting(meeting_id: String, project: Option<String>, title: Option<String>) -> Result<String, String>` (returns the new folder path). Tasks 7 and 8 call them.

- [ ] **Step 1: Expose the running meeting id**

In `retranscription.rs`, below `is_retranscription_in_progress`:

```rust
/// spec 4.2: the meeting whose enhance job holds the in-progress flag, if any
pub fn running_meeting_id() -> Option<String> {
    RETRANSCRIPTION_RUNNING_MEETING.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
```

- [ ] **Step 2: Re-resolve the folder after waiting in the queue**

In `start_retranscription`, right after `let _queue_slot = ENHANCE_QUEUE.lock().await;`, add:

```rust
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
```

and delete the earlier `let folder = PathBuf::from(&meeting_folder_path);` so `folder` is defined once, after this block. The status writes that happen before the lock (`STATUS_QUEUED`) must use the original path: keep a `let queued_folder = PathBuf::from(&meeting_folder_path);` before the queue wait for that first `set_enhance_status` call. `meeting_folder_path` must be `mut` or shadowed; shadowing as shown is fine.

- [ ] **Step 3: transcript.md after enhance**

In `run_retranscription`, after `write_transcripts_json(&folder_path, &segments)` and before `write_retranscription_metadata(...)`, add:

```rust
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
        let md_meta = super::transcript_md::TranscriptMdMeta {
            title,
            project,
            meeting_id: &meeting_id,
            started_at: &started_at,
            duration_seconds: Some(duration_seconds),
            quality: "enhanced",
            model: &model_label,
        };
        let md_segments: Vec<(f64, String)> = segments
            .iter()
            .map(|s| (s.audio_start_time.unwrap_or(0.0), s.text.clone()))
            .collect();
        if let Err(e) = super::transcript_md::write_transcript_md(&folder_path, &md_meta, &md_segments) {
            warn!("Failed to write transcript.md after enhance: {}", e);
        }
    }
```

`model` is `Option<String>` in scope (the function parameter); if it was moved earlier, clone it at the top of the function. In `write_retranscription_metadata`, add `obj.insert("transcript_md".to_string(), serde_json::json!("transcript.md"));` next to the `transcript_file` insert.

- [ ] **Step 4: `assign` and the two commands**

Add to `library.rs`:

```rust
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
    let row: Option<(Option<String>, String)> = sqlx::query_as("SELECT folder_path, title FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
    let Some((folder_path, db_title)) = row else { return Err(format!("No meeting found with id {}", meeting_id)) };

    if let Some(p) = project {
        if p.trim().is_empty() || p.starts_with('.') || p.contains('/') || p.contains('\\') {
            return Err(format!("Invalid project name: {}", p));
        }
    }
    if crate::audio::retranscription::running_meeting_id().as_deref() == Some(meeting_id) {
        return Err("This meeting is being enhanced right now. Try again when it finishes.".to_string());
    }
    if crate::audio::recording_commands::is_recording().await {
        if let Ok(Some(active)) = crate::audio::recording_commands::get_meeting_folder_path().await {
            if folder_path.as_deref() == Some(active.as_str()) {
                return Err("This meeting is being recorded right now.".to_string());
            }
        }
    }

    let new_title = title.map(str::to_string).unwrap_or(db_title);
    let now = chrono::Utc::now();

    let Some(old_path) = folder_path.map(PathBuf::from).filter(|p| p.exists()) else {
        sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
            .bind(&new_title).bind(now).bind(meeting_id)
            .execute(pool).await.map_err(|e| format!("Database error: {}", e))?;
        return Err("Folder not found on disk; only the title was updated".to_string());
    };

    let meta_path = old_path.join("metadata.json");
    let mut meta: serde_json::Value = std::fs::read_to_string(&meta_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let current_project = project_of(root, &old_path);
    let new_project = project.map(str::to_string).unwrap_or(current_project);
    let old_name = old_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
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

    if let Some(obj) = meta.as_object_mut() {
        obj.insert("meeting_id".into(), serde_json::json!(meeting_id));
        obj.insert("project".into(), serde_json::json!(new_project));
        obj.insert("title".into(), serde_json::json!(new_title));
        obj.entry("version").or_insert(serde_json::json!("2.0"));
        let tmp = new_path.join(".metadata.json.tmp");
        if let Err(e) = std::fs::write(&tmp, serde_json::to_string_pretty(&meta).unwrap_or_default())
            .and_then(|_| std::fs::rename(&tmp, new_path.join("metadata.json")))
        {
            log::warn!("Could not update metadata.json after move: {}", e);
        }
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
```

Register in `lib.rs` after `audio::enhance_status::list_enhance_statuses,`:

```rust
            audio::library::list_projects_command,
            audio::library::assign_meeting,
```

The frontend invokes `list_projects_command` under that exact name.

- [ ] **Step 5: Rename through `assign`**

In `api/api.rs`, replace the body of `api_save_meeting_title` after the `log_info!` with:

```rust
    // spec 4.2: a title change renames the folder too (slug), via the library
    let pool = state.db_manager.pool();
    let root = crate::audio::get_default_recordings_folder();
    match crate::audio::library::assign(pool, &root, &meeting_id, None, Some(&title)).await {
        Ok(_) => {
            log_info!("Successfully saved meeting title");
            Ok(serde_json::json!({"message": "Meeting title saved successfully"}))
        }
        Err(e) if e.starts_with("Folder not found on disk") => {
            log_warn!("{}", e);
            Ok(serde_json::json!({"message": "Meeting title saved successfully"}))
        }
        Err(e) => {
            log_error!("Failed to update meeting {}", e);
            Err(format!("Failed to update meeting: {}", e))
        }
    }
```

- [ ] **Step 6: Verify**

Run: `cd frontend/src-tauri && cargo check 2>&1 | grep -E "^(error|warning: unused)" ; cargo test --lib library && cargo test --lib retranscription`
Expected: no errors, no new warnings, both suites green (library 8, retranscription 17).

- [ ] **Step 7: Commit**

```bash
git add frontend/src-tauri/src/audio/library.rs frontend/src-tauri/src/audio/retranscription.rs frontend/src-tauri/src/api/api.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(library): assign_meeting and list_projects commands, rename moves the folder, enhance writes transcript.md"
```

---

### Task 7: Project picker at record start

**Files:**
- Create: `frontend/src/lib/projects.ts`
- Create: `frontend/src/components/ProjectSelect.tsx`
- Modify: `frontend/src/services/recordingService.ts` (`startRecordingWithDevices`)
- Modify: `frontend/src/hooks/useRecordingStart.ts` (both `startRecordingWithDevices` calls)
- Modify: `frontend/src/app/page.tsx` (inside the pill next to `<RecordingControls`)

**Interfaces:**
- Consumes: Tauri commands `list_projects_command` and (Task 3) `start_recording_with_devices_and_meeting` with `project`.
- Produces: `getLastProject(): string`, `setLastProject(p: string): void`, `projectOfFolder(root: string, folderPath: string): string`; component `ProjectSelect({ value, onChange, disabled?, compact? })`.

- [ ] **Step 1: `projects.ts`**

```ts
// spec 4.2: project helpers shared by the record button and meeting details.
export const INBOX = '_inbox';
const LAST_PROJECT_KEY = 'meetily.lastProject';

/** The project chosen last time. Per-device convenience, so localStorage is fine. */
export function getLastProject(): string {
  try {
    return localStorage.getItem(LAST_PROJECT_KEY) || INBOX;
  } catch {
    return INBOX;
  }
}

export function setLastProject(project: string): void {
  try {
    localStorage.setItem(LAST_PROJECT_KEY, project);
  } catch {
    // storage unavailable: the picker still works for this session
  }
}

/** Parent directory name when the folder sits two levels under the root, `_inbox` otherwise. */
export function projectOfFolder(root: string, folderPath: string): string {
  const normalizedRoot = root.replace(/\/+$/, '');
  if (!folderPath.startsWith(normalizedRoot + '/')) return INBOX;
  const parts = folderPath.slice(normalizedRoot.length + 1).split('/').filter(Boolean);
  return parts.length >= 2 ? parts[0] : INBOX;
}

/** Lowercase, dashes, no leading dot: the same shape the Rust side accepts. */
export function normalizeProjectName(input: string): string {
  return input.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40);
}
```

- [ ] **Step 2: `ProjectSelect.tsx`**

```tsx
'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { INBOX, normalizeProjectName } from '@/lib/projects';

const NEW_PROJECT = '__new__';

interface ProjectSelectProps {
  value: string;
  onChange: (project: string) => void;
  disabled?: boolean;
  compact?: boolean;
}

/** spec 4.2: pick a project folder. Projects are folders under the recordings root; "New project" creates one by name. */
export function ProjectSelect({ value, onChange, disabled, compact }: ProjectSelectProps) {
  const [projects, setProjects] = useState<string[]>([INBOX]);
  const [creating, setCreating] = useState(false);
  const [draft, setDraft] = useState('');

  useEffect(() => {
    invoke<string[]>('list_projects_command')
      .then((list) => setProjects(list.length ? list : [INBOX]))
      .catch((error) => console.warn('[ProjectSelect] list_projects_command failed:', error));
  }, []);

  const options = projects.includes(value) ? projects : [...projects, value];

  const confirmNew = () => {
    const name = normalizeProjectName(draft);
    if (!name) return;
    setProjects((prev) => (prev.includes(name) ? prev : [...prev, name].sort((a, b) => (a === INBOX ? -1 : b === INBOX ? 1 : a.localeCompare(b)))));
    setCreating(false);
    setDraft('');
    onChange(name);
  };

  if (creating) {
    return (
      <div className="flex items-center gap-1">
        <Input
          autoFocus
          value={draft}
          placeholder="project name"
          className={compact ? 'h-8 w-36 text-xs' : 'h-9 w-44 text-sm'}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') confirmNew();
            if (e.key === 'Escape') { setCreating(false); setDraft(''); }
          }}
        />
        <Button size="sm" variant="outline" onClick={confirmNew} disabled={!normalizeProjectName(draft)}>Add</Button>
      </div>
    );
  }

  return (
    <Select
      value={value}
      disabled={disabled}
      onValueChange={(v) => {
        if (v === NEW_PROJECT) { setCreating(true); return; }
        onChange(v);
      }}
    >
      <SelectTrigger className={compact ? 'h-8 w-36 text-xs' : 'h-9 w-44 text-sm'} title="Project folder for this meeting">
        <SelectValue placeholder="Project" />
      </SelectTrigger>
      <SelectContent>
        {options.map((p) => (
          <SelectItem key={p} value={p}>{p === INBOX ? 'Inbox' : p}</SelectItem>
        ))}
        <SelectItem value={NEW_PROJECT}>New project...</SelectItem>
      </SelectContent>
    </Select>
  );
}
```

- [ ] **Step 3: Pass `project` to the start command**

`recordingService.ts`:

```ts
  async startRecordingWithDevices(
    micDeviceName: string | null,
    systemDeviceName: string | null,
    meetingName: string,
    project: string | null = null, // spec 4.2
  ): Promise<void> {
    return withStartTimeout(invoke('start_recording_with_devices_and_meeting', {
      micDeviceName,
      systemDeviceName,
      meetingName,
      project,
    }));
  }
```

`useRecordingStart.ts`: import `{ getLastProject } from '@/lib/projects'` and pass `getLastProject()` as the fourth argument in both `recordingService.startRecordingWithDevices(...)` calls (the manual start around line 158 and the auto-start around line 251).

- [ ] **Step 4: The picker next to the record controls**

In `page.tsx`, import `{ ProjectSelect } from '@/components/ProjectSelect'` and `{ getLastProject, setLastProject } from '@/lib/projects'`. Add state near the other `useState` calls:

```ts
  const [project, setProject] = useState<string>(getLastProject()); // spec 4.2
```

Inside the pill `<div className="bg-white rounded-full shadow-lg flex items-center">`, before `<RecordingControls`, add:

```tsx
                    {!recordingState.isRecording && (
                      <div className="pl-3">
                        <ProjectSelect
                          value={project}
                          onChange={(p) => { setProject(p); setLastProject(p); }}
                          disabled={isRecordingDisabled}
                          compact
                        />
                      </div>
                    )}
```

`getLastProject()` touches `localStorage`; the home page is a client component (`useState` is already used there). If the initial render runs during static export without `window`, wrap the initializer: `useState<string>(() => (typeof window === 'undefined' ? INBOX : getLastProject()))` and import `INBOX`.

- [ ] **Step 5: Type-check**

Run: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json`
Expected: no output.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/lib/projects.ts frontend/src/components/ProjectSelect.tsx frontend/src/services/recordingService.ts frontend/src/hooks/useRecordingStart.ts frontend/src/app/page.tsx
git commit -m "feat(library): project picker next to the record button"
```

---

### Task 8: Move to project from meeting details

**Files:**
- Modify: `frontend/src/components/MeetingDetails/TranscriptButtonGroup.tsx`

**Interfaces:**
- Consumes: `ProjectSelect`, `projectOfFolder` (Task 7), Tauri commands `get_default_recordings_folder_path` (exists) and `assign_meeting` (Task 6), `useSidebar().refetchMeetings`.

- [ ] **Step 1: Add the control**

Imports to add:

```ts
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { ProjectSelect } from '@/components/ProjectSelect';
import { INBOX, projectOfFolder } from '@/lib/projects';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
```

(merge `useState` with the existing `import { useState, useCallback } from 'react';`).

Inside the component, after `const [showRetranscribeDialog, setShowRetranscribeDialog] = useState(false);`:

```ts
  // spec 4.2: move the meeting to another project folder
  const { refetchMeetings } = useSidebar();
  const [root, setRoot] = useState<string>('');
  const [moving, setMoving] = useState(false);
  useEffect(() => {
    invoke<string>('get_default_recordings_folder_path').then(setRoot).catch(() => setRoot(''));
  }, []);
  const currentProject = meetingFolderPath && root ? projectOfFolder(root, meetingFolderPath) : INBOX;

  const handleMove = async (project: string) => {
    if (!meetingId || project === currentProject) return;
    setMoving(true);
    try {
      await invoke<string>('assign_meeting', { meetingId, project });
      toast.success(`Moved to ${project === INBOX ? 'Inbox' : project}`);
      await refetchMeetings();
      await onRefetchTranscripts?.();
    } catch (error) {
      toast.error('Could not move the meeting', { description: error instanceof Error ? error.message : String(error) });
    } finally {
      setMoving(false);
    }
  };
```

In the JSX, right after the closing `</ButtonGroup>` and before the `RetranscribeDialog` block, add:

```tsx
      {meetingId && meetingFolderPath && (
        <ProjectSelect value={currentProject} onChange={handleMove} disabled={moving} compact />
      )}
```

The outer `div` is `flex items-center justify-center w-full gap-2`, so the select sits beside the buttons.

- [ ] **Step 2: Type-check**

Run: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json`
Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/MeetingDetails/TranscriptButtonGroup.tsx
git commit -m "feat(library): move a meeting to another project from meeting details"
```

---

### Task 9: Manual acceptance on real recordings

**Files:** none.

- [ ] **Step 1: Start the app** with `cd frontend && pnpm run tauri dev` (owner's terminal, which already holds the microphone permission). On launch, the log shows `Library reconcile at startup: 0 rows updated`.

- [ ] **Step 2: Record into a project.** Pick "New project...", type `dex`, record 30 seconds, stop. Expected: a folder `~/Movies/meetily-recordings/dex/<YYYY-MM-DD_HH-MM>/` with `audio.mp4`, `transcripts.json`, `metadata.json` (version 2.0, `meeting_id` set, `project: "dex"`, `title`, `transcription.live`) and `transcript.md` with `quality: live`. The sidebar lists the meeting; the enhance job runs and `transcript.md` flips to `quality: enhanced` with `model: whisper large-v3`; `metadata.json` gains `transcript_md` and `transcription.enhanced.status: done`.

- [ ] **Step 3: Rename.** Rename the meeting from the sidebar pencil to "Western Digital onboarding". Expected: the folder becomes `dex/<stamp> western-digital-onboarding`, `metadata.json` `title` updated, "Open Recording Folder" still works.

- [ ] **Step 4: Move.** In meeting details, change the project select to Inbox. Expected: folder now under `_inbox/`, toast "Moved to Inbox", the picker shows Inbox after refresh.

- [ ] **Step 5: Summary title survives refresh.** On a meeting with a transcript, generate a summary (the summary panel). The sidebar title changes to the summary heading. Click another meeting and back. Expected: the title stays, and the folder slug matches it after any running enhance finishes.

- [ ] **Step 6: Rename during enhance.** Record 30 seconds, stop, and while the status dot is blue, rename the meeting from the sidebar. Expected: the rename succeeds, the title stays after refresh, and once the dot disappears the folder name carries the new slug.

- [ ] **Step 7: External move.** Quit nothing; in Finder, drag that folder back into `dex/`. Click another meeting then back (sidebar refresh). Expected: "Open Recording Folder" opens the new location (reconcile repaired `folder_path`).

- [ ] **Step 8: Legacy untouched.** The 2026-10-27 flat folders at the root are still there, still open from their meetings, and show no chip.

- [ ] **Step 9: Inbox default.** Select Inbox, record 20 seconds. Expected: folder under `_inbox/` named by stamp only (auto title gives no slug).

- [ ] **Step 10: Finish** with `superpowers:finishing-a-development-branch` (squash merge to `main`, as Phase 1).


---

### Task 10: Collapsible project sections in the sidebar

Added after the owner's acceptance run: the meeting list should group meetings under one collapsible section per project.

**Files:**
- Modify: `frontend/src-tauri/src/api/api.rs` (`pub struct Meeting` near line 30, and the single construction site in `api_get_meetings`)
- Modify: `frontend/src/components/Sidebar/SidebarProvider.tsx` (`CurrentMeeting`, `fetchMeetings`, `baseItems`)
- Modify: `frontend/src/components/Sidebar/index.tsx` (`isExpanded`, `isMeetingItem`, `toggleFolder`, the search filter memo)

**Interfaces:**
- Consumes: `library::project_of(root, folder) -> String` (Task 1), `INBOX` from `@/lib/projects` (Task 7).
- Produces: `Meeting.project: String` on the `api_get_meetings` payload; `CurrentMeeting.project?: string`; sidebar folder ids `project:<name>`.

- [ ] **Step 1: Project on the meetings payload**

In `api/api.rs`, extend the struct:

```rust
pub struct Meeting {
    pub id: String,
    pub title: String,
    /// spec 4.2: project folder derived from folder_path, `_inbox` when unknown or legacy
    pub project: String,
}
```

In `api_get_meetings`, compute it while mapping (the root is `crate::audio::get_default_recordings_folder()`, computed once before the map):

```rust
            let root = crate::audio::get_default_recordings_folder();
            let result: Vec<Meeting> = meeting_models
                .into_iter()
                .map(|m| Meeting {
                    id: m.id,
                    title: m.title,
                    project: m
                        .folder_path
                        .as_deref()
                        .map(|f| crate::audio::library::project_of(&root, std::path::Path::new(f)))
                        .unwrap_or_else(|| crate::audio::library::INBOX.to_string()),
                })
                .collect();
```

Grep the crate for other `Meeting {` constructions and add `project` there if any exist (at the time of writing there is only this one).

- [ ] **Step 2: Group meetings by project in the provider**

In `SidebarProvider.tsx`, import `{ INBOX } from '@/lib/projects'`. Extend the type:

```ts
export interface CurrentMeeting {
  id: string;
  title: string;
  enhanceStatus?: string;
  // spec 4.2: project folder name, `_inbox` for legacy meetings
  project?: string;
}
```

In `fetchMeetings`, the invoke type becomes `Array<{ id: string, title: string, project?: string }>` and the mapping adds `project: meeting.project || INBOX`.

Replace the `baseItems` definition with:

```ts
  // spec 4.2: one collapsible section per project, Inbox first
  const PROJECT_FOLDER_PREFIX = 'project:';
  const meetingsByProject = new Map<string, CurrentMeeting[]>();
  for (const meeting of meetings) {
    const project = meeting.project || INBOX;
    const bucket = meetingsByProject.get(project);
    if (bucket) bucket.push(meeting); else meetingsByProject.set(project, [meeting]);
  }
  const projectNames = Array.from(meetingsByProject.keys()).sort((a, b) =>
    a === INBOX ? -1 : b === INBOX ? 1 : a.localeCompare(b),
  );
  const baseItems: SidebarItem[] = [
    {
      id: 'meetings',
      title: 'Meeting Notes',
      type: 'folder' as const,
      children: projectNames.map((project) => ({
        id: PROJECT_FOLDER_PREFIX + project,
        title: project === INBOX ? 'Inbox' : project,
        type: 'folder' as const,
        children: (meetingsByProject.get(project) ?? []).map((meeting) => ({
          id: meeting.id,
          title: meeting.title,
          type: 'file' as const,
          enhanceStatus: meeting.enhanceStatus,
        })),
      })),
    },
  ];
```

Export the prefix for the renderer: `export const PROJECT_FOLDER_PREFIX = 'project:';` at module level (and use that constant in the block above instead of a local).

- [ ] **Step 3: Project folders expand by default and remember collapses**

In `index.tsx`, import `PROJECT_FOLDER_PREFIX` from `@/components/Sidebar/SidebarProvider`. Add state next to `expandedFolders`:

```ts
  // spec 4.2: project sections are open unless the user collapsed them; the choice is kept per device
  const COLLAPSED_PROJECTS_KEY = 'meetily.collapsedProjects';
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(() => {
    try {
      return new Set<string>(JSON.parse(localStorage.getItem(COLLAPSED_PROJECTS_KEY) || '[]'));
    } catch {
      return new Set<string>();
    }
  });
```

Change `toggleFolder` to route project ids to that set:

```ts
  const toggleFolder = (folderId: string) => {
    if (folderId.startsWith(PROJECT_FOLDER_PREFIX)) {
      const next = new Set(collapsedProjects);
      if (next.has(folderId)) next.delete(folderId); else next.add(folderId);
      setCollapsedProjects(next);
      try { localStorage.setItem(COLLAPSED_PROJECTS_KEY, JSON.stringify(Array.from(next))); } catch { /* per-device convenience only */ }
      return;
    }
    // Normal toggle behavior for all other folders
    const newExpanded = new Set(expandedFolders);
    if (newExpanded.has(folderId)) {
      newExpanded.delete(folderId);
    } else {
      newExpanded.add(folderId);
    }
    setExpandedFolders(newExpanded);
  };
```

In `renderItem`, change the two checks:

```ts
    const isExpanded = item.id.startsWith(PROJECT_FOLDER_PREFIX)
      ? !collapsedProjects.has(item.id)
      : expandedFolders.has(item.id);
    const isActive = item.type === 'file' && currentMeeting?.id === item.id;
    const isMeetingItem = item.type === 'file' && item.id.includes('-') && !item.id.startsWith('intro-call');
```

(`isMeetingItem` gains the `type === 'file'` guard so a project named `flowers-2026` is not mistaken for a meeting.)

- [ ] **Step 4: Search filters through the nested tree**

Replace the body of the `filteredSidebarItems` memo so it recurses instead of assuming one level of children. Keep the memo's dependency array as it is.

```ts
  // spec 4.2: filter meetings inside project sections; drop empty sections, keep the top folder
  const filterTree = (items: SidebarItem[], keep: (item: SidebarItem) => boolean): SidebarItem[] => {
    const out: SidebarItem[] = [];
    for (const item of items) {
      if (item.type === 'folder') {
        const children = filterTree(item.children ?? [], keep);
        if (children.length > 0 || item.id === 'meetings') out.push({ ...item, children });
      } else if (keep(item)) {
        out.push(item);
      }
    }
    return out;
  };

  const filteredSidebarItems = useMemo(() => {
    if (!searchQuery.trim()) return sidebarItems;
    const q = searchQuery.toLowerCase();
    if (searchResults.length > 0) {
      const matchedMeetingIds = new Set(searchResults.map(result => result.id));
      return filterTree(sidebarItems, item => matchedMeetingIds.has(item.id) || item.title.toLowerCase().includes(q));
    }
    return filterTree(sidebarItems, item => item.title.toLowerCase().includes(q));
  }, [sidebarItems, searchQuery, searchResults, expandedFolders]);
```

If the existing memo's dependency array differs, keep the existing one. Define `filterTree` above the memo (inside the component or at module level; module level is fine since it is pure).

- [ ] **Step 5: Type-check**

Run: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json` and `cd frontend/src-tauri && cargo check`
Expected: tsc silent; cargo check with no new warnings.

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/src/api/api.rs frontend/src/components/Sidebar/SidebarProvider.tsx frontend/src/components/Sidebar/index.tsx
git commit -m "feat(library): collapsible project sections in the sidebar"
```

Acceptance (owner): the sidebar shows Inbox first, then each project, each collapsible; a collapsed section stays collapsed after switching pages; search still finds meetings inside sections and hides empty sections; the pencil and trash actions still appear only on meetings.

---

## Self-review against the spec

- 4.2 layout and rules: Task 1 (naming, projects, scan), Task 3 (recording into `<root>/<project>/`), Task 6 (assign and rename), Task 5 (reconcile on startup and refresh), legacy folders untouched (scan skips them, nothing moves them).
- 4.2 meeting id at start: Task 3 mints it and emits it; Task 4 reuses it in SQLite.
- 4.2 UI: Task 7 picker (remembers the last choice), Task 8 move, rename through Task 6.
- 4.3 transcript.md: Task 2 renders; Task 3 writes at stop (live); Task 6 writes after enhance (enhanced).
- 5.2 metadata v2 keys: Task 3 (`project`, `title`, `transcript_md`, `transcription.live`, version 2.0); Task 6 adds `transcript_md` on the enhance rewrite; Phase 1's `transcription.enhanced` is preserved because every writer merges into the existing JSON or writes before enhance starts.
- 5.3 format: Task 2 test pins the exact bytes.
- Names: `INBOX`, `slug`, `is_auto_title`, `local_stamp`, `stamp_of_folder`, `meeting_folder_name`, `project_of`, `list_projects`, `create_meeting_folder`, `LibraryEntry`, `scan`, `ReconcileUpdate`, `plan_reconcile`, `reconcile`, `assign`, `list_projects_command`, `assign_meeting`, `running_meeting_id`, `TranscriptMdMeta`, `format_mmss`, `local_iso`, `render_transcript_md`, `write_transcript_md`, `getLastProject`, `setLastProject`, `projectOfFolder`, `normalizeProjectName`, `ProjectSelect`. Each defined once, spelled the same everywhere.
- Known limits accepted for this phase: `auto_save` off writes no transcript.md (the early return in `stop_and_save` predates this work); a queued enhance job for a meeting that is moved re-resolves its folder from the index after the wait (Task 6 Step 2), but a job already running blocks moves (assign refuses).

## Outcome (2026-10-27)

Implemented on branch `phase-2/projects` (20 commits incl. docs). Task reviews passed, two of them after one fix round each (clock-dependent test; project-name validation). The whole-branch review found one Critical integration bug (the summary auto-rename bypassed the library and was reverted by reconcile) and two Important behaviours (titles refused during an enhance job; a title edit relocated legacy folders), all fixed and re-reviewed. Owner acceptance passed on real recordings; Task 10 (collapsible project sections) was added at the owner's request and confirmed in the UI.

Owner note: new recordings live one level deeper than before; wiki tooling that globs `Meeting */metadata.json` at the root needs `*/*/metadata.json` too until Phase 3 lands. `transcripts.json` is still written.

### Follow-ups deferred from review (none block merge)

- : a stray metadata.json inside a project dir hides the project from list_projects and scan.
- : exists()+create_dir_all race on folder creation; create_dir with AlreadyExists retry would be atomic.
- : create_meeting_folder does not sanitize `project`; Task 6's assign validates it, the record-start path must too (check in Task 3 review).
- : duplicate meeting_id across two folders (Finder copy) makes reconcile updates fight; first-wins or a warn would help.
- : scan skips unreadable dirs and malformed JSON silently.
- : plan_reconcile doc should state that title None means "leave unchanged" (reconcile in Task 5 uses COALESCE, which honours it).
- : slug has no tests for non-ASCII titles or a dash at the 60-char boundary.
- : title escaping covers only backslash and quote; control chars in a title could break the frontmatter.
- : project, meeting_id and model are unquoted YAML scalars (fine for slugs/ids/known models).
- : fixed temp filename .transcript.md.tmp; concurrent writers in one folder would collide (plan-mandated, single writer assumed).
- : temp file leaks on write/rename failure.
- : negative/NaN duration not clamped.
- : local_iso test asserts the local date; fails at UTC+8 or later (owner is in US Eastern).
- : with auto_save off, stop_and_save returns early (pre-existing) so no transcript.md and no completed status; consumers must not assume transcript.md exists.
- : model label can be "" when transcription.live has empty provider and model; filter before falling back to "live".
- : the 12-line id/project/live-config block is duplicated across the two start functions (mirrors existing structure).
- : fallback provider "parakeet" when no config row exists; matches engine.rs default (verified earlier: get_or_init_transcription_engine falls back to Parakeet).
- : validator trims before checking but the untrimmed name is joined; " dex" creates a folder with a leading space.
- : a failed save leaves last_recording_meeting_id in sessionStorage (consistent with folder_path handling; unreachable while Task 3 always emits the id).
- : id stored untrimmed (harmless for uuids).
- : scan does blocking fs reads inside an async fn on every sidebar refresh (spawn_blocking is the upgrade).
- : reconcile updates are not transactional; first failure aborts the rest (next refresh retries).
- : assign overwrites an existing but unparseable metadata.json with a 3-key stub; skipping with a warning would keep it for recovery.
- : enhance re-resolves the folder before taking the in-progress guard; an assign between the read and the guard runs the job on a moved path (small window).
- : assign uses the untrimmed project name after a trimmed validation (" dex" creates a folder with a leading space).
- : a failed metadata write after a title change gets reverted by the next reconcile (metadata wins); rare.
- : rename refused while the meeting is being enhanced, title not saved either; toast shows the reason (plan-mandated).
- : assign is ~90 lines; the target-path computation could be a pure, testable function.
- : picker shows page state while the start call reads localStorage; with blocked storage they can diverge.
- : possible hydration mismatch on the prerendered picker (reads storage in the initializer).
- : inline new-project input lacks aria-label and blur handling; disabled ignored while creating.
- : a stored project missing on disk still appears as an option (folder is created at record start, so harmless).
- : refetches run inside the same try as the move; a refetch error would show "Could not move" after a successful move.
- : while the root is unknown, currentProject shows Inbox for a meeting elsewhere.
- : transcript panel briefly shows loading after a move (same as after Enhance).
- : layout of the select beside the container-query button group at narrow widths not exercised.
- : useTranscriptRecovery saves a recovered meeting without the minted id (self-heals on first assign).
- : reconcile and a concurrent assign can interleave; stale folder_path until next refresh.
- : tray start always records into _inbox (last project lives in localStorage).
- : yaml_string does not escape newlines (titles are single-line today).
- : assign on a folder outside the root treats it as _inbox and renames into the root.
- : audio import still creates flat legacy folders; route through library::create_meeting_folder to unify.
- : id/project/live-config block duplicated across the two start functions.
- : a double stop shows a "save failed" toast though the first save succeeded.
- in-place title write can be lost if it lands between the enhance job's metadata read and write; post-job assign repairs from the DB title unless a reconcile runs first (ms window). Ruling: real, rare, self-correcting on the next title write; revisit with a shared metadata lock if seen.
- a summary rename that completes while the meeting is still recording is refused by assign and only logged (old code always wrote the title). Ruling: unlikely (summaries run after stop); acceptable.
- transcript.md frontmatter keeps the title it had when written; a rename after enhance leaves it stale. Ruling: real; the MCP (Phase 3) should take the title from metadata.json, and assign could rewrite the frontmatter line later.
- : search results inside a collapsed project section stay hidden; treat sections as expanded while a search is active.
- : setCurrentMeeting drops project and enhanceStatus (pre-existing shape).
- : collapse state for a project that no longer exists stays in localStorage.
- : filterTree is redefined per render (pure; module level would be cleaner).
