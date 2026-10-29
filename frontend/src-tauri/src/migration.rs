//! spec 5.1: one-time move of the Meetily data dirs to their Noesis names. Never deletes.

use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Rename(Move),
    SkipBothExist(Move),
    Nothing,
}

#[derive(Debug, Default)]
pub struct MigrationOutcome {
    pub moved: Vec<Move>,
    pub skipped: Vec<Move>,
    pub failed: Vec<(Move, String)>,
    /// Pairs held back because a pair they depend on did not migrate.
    pub gated: Vec<Move>,
}

/// Pure: decides per pair from existence flags. Only "old exists, new does not" renames.
pub fn plan(pairs: &[Move], exists: impl Fn(&Path) -> bool) -> Vec<Step> {
    pairs
        .iter()
        .map(|m| match (exists(&m.from), exists(&m.to)) {
            (true, false) => Step::Rename(m.clone()),
            (true, true) => Step::SkipBothExist(m.clone()),
            _ => Step::Nothing,
        })
        .collect()
}

/// Executes Rename steps with `std::fs::rename`; never deletes, never copies; logs every step.
/// A failed rename (e.g. EXDEV across volumes) leaves the old dir untouched and the app
/// carries on with the new, empty location.
pub fn run(steps: &[Step]) -> MigrationOutcome {
    let mut out = MigrationOutcome::default();
    for step in steps {
        match step {
            Step::Rename(m) => match std::fs::rename(&m.from, &m.to) {
                Ok(()) => {
                    log::info!("migration: moved {} -> {}", m.from.display(), m.to.display());
                    out.moved.push(m.clone());
                }
                Err(e) => {
                    log::error!(
                        "migration: could not move {} -> {}: {}; old dir left untouched, starting with an empty {}",
                        m.from.display(), m.to.display(), e, m.to.display()
                    );
                    out.failed.push((m.clone(), e.to_string()));
                }
            },
            Step::SkipBothExist(m) => {
                log::warn!(
                    "migration: both {} and {} exist; leaving both, using the new one",
                    m.from.display(), m.to.display()
                );
                out.skipped.push(m.clone());
            }
            Step::Nothing => {}
        }
    }
    out
}

/// Pure: rewrites a path that sits under `old_root` (component-wise, not string prefix).
pub fn rewrite_root(path: &str, old_root: &Path, new_root: &Path) -> Option<String> {
    let rest = Path::new(path).strip_prefix(old_root).ok()?;
    let out = if rest.as_os_str().is_empty() { new_root.to_path_buf() } else { new_root.join(rest) };
    Some(out.to_string_lossy().into_owned())
}

/// Rewrites `meetings.folder_path` rows under `old_root`; returns rows changed.
pub async fn rewrite_folder_paths(pool: &sqlx::SqlitePool, old_root: &Path, new_root: &Path) -> Result<u64> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, folder_path FROM meetings WHERE folder_path IS NOT NULL")
            .fetch_all(pool)
            .await?;
    let mut tx = pool.begin().await?;
    let mut changed = 0;
    for (id, folder) in rows {
        if let Some(new_path) = rewrite_root(&folder, old_root, new_root) {
            sqlx::query("UPDATE meetings SET folder_path = ? WHERE id = ?")
                .bind(&new_path)
                .bind(&id)
                .execute(&mut *tx)
                .await?;
            changed += 1;
        }
    }
    tx.commit().await?;
    Ok(changed)
}

/// Rewrites `save_folder` if it sits under `old_root`; returns whether it changed.
pub fn rewrite_save_folder(prefs: &mut crate::audio::RecordingPreferences, old_root: &Path, new_root: &Path) -> bool {
    match rewrite_root(&prefs.save_folder.to_string_lossy(), old_root, new_root) {
        Some(p) => {
            prefs.save_folder = PathBuf::from(p);
            true
        }
        None => false,
    }
}

/// The recordings root pair, with the same per-OS base dir the app uses.
pub fn recordings_pair() -> Move {
    Move {
        from: crate::audio::recording_preferences::recordings_base_dir()
            .join(crate::app_names::LEGACY_RECORDINGS_DIR_NAME),
        to: crate::audio::get_default_recordings_folder(),
    }
}

/// Tauri's app data dir pair (`dirs::data_dir()/<identifier>`): settings stores and the SQLite index.
pub fn app_data_pair() -> Option<Move> {
    use crate::app_names::*;
    dirs::data_dir().map(|data| Move { from: data.join(LEGACY_IDENTIFIER), to: data.join(IDENTIFIER) })
}

/// The secondary "Meetily" dir under both data_dir and config_dir (templates, notifications;
/// the same dir on macOS and Windows, so deduplicated). Independent of the other pairs.
pub fn config_pairs() -> Vec<Move> {
    use crate::app_names::*;
    let mut pairs: Vec<Move> = Vec::new();
    for base in [dirs::data_dir(), dirs::config_dir()].into_iter().flatten() {
        let m = Move { from: base.join(LEGACY_CONFIG_DIR_NAME), to: base.join(APP_CONFIG_DIR_NAME) };
        if !pairs.contains(&m) {
            pairs.push(m);
        }
    }
    pairs
}

/// Every legacy dir pair.
pub fn legacy_pairs() -> Vec<Move> {
    let mut pairs: Vec<Move> = app_data_pair().into_iter().collect();
    pairs.extend(config_pairs());
    pairs.push(recordings_pair());
    pairs
}

/// Pure: the recordings root may move only if the database that indexes it moves with it,
/// i.e. the app data dir was renamed this run, or there was no old app data dir (nothing
/// to rewrite). Otherwise moving it would leave the real index pointing at the old root.
pub fn gate_recordings(app_data: &Step, app_data_moved: bool, recordings: Step) -> Step {
    match app_data {
        Step::Nothing => recordings,
        Step::Rename(_) if app_data_moved => recordings,
        _ => Step::Nothing,
    }
}

/// Plans and runs the moves, the recordings root last and gated on the app data dir.
pub fn migrate(app_data: Option<Move>, config: &[Move], recordings: Move, exists: impl Fn(&Path) -> bool) -> MigrationOutcome {
    let app_step = match &app_data {
        Some(m) => plan(std::slice::from_ref(m), &exists).remove(0),
        None => Step::Nothing,
    };
    let mut steps = vec![app_step.clone()];
    steps.extend(plan(config, &exists));
    let mut out = run(&steps);

    let app_moved = app_data.as_ref().is_some_and(|m| out.moved.contains(m));
    let rec_step = plan(std::slice::from_ref(&recordings), &exists).remove(0);
    let gated = gate_recordings(&app_step, app_moved, rec_step.clone());
    if gated != rec_step {
        if let Some(app) = &app_data {
            log::error!(
                "migration: NOT moving {} -> {} because the app data dir was not migrated ({} -> {}); resolve the app data dir first, then relaunch",
                recordings.from.display(), recordings.to.display(), app.from.display(), app.to.display()
            );
        }
        out.gated.push(recordings);
    } else {
        let rec_out = run(&[gated]);
        out.moved.extend(rec_out.moved);
        out.skipped.extend(rec_out.skipped);
        out.failed.extend(rec_out.failed);
    }
    out
}

/// Startup step 1, before anything resolves the app dirs: plan and run every rename.
pub fn migrate_dirs() -> MigrationOutcome {
    // symlink_metadata: a dangling symlink at the target still counts as existing
    migrate(app_data_pair(), &config_pairs(), recordings_pair(), |p| p.symlink_metadata().is_ok())
}

/// Written in the Noesis app data dir when the user answers "Skip"; the dialog then stays away.
pub const SKIP_MARKER: &str = "migration.skipped";

/// Pure: a dialog is due when at least one legacy dir would be renamed.
pub fn needs_prompt(steps: &[Step]) -> bool {
    steps.iter().any(|s| matches!(s, Step::Rename(_)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Move,
    Skip,
}

/// Blocking native dialog. Runs before the Tauri window exists, so it uses rfd directly.
#[cfg(target_os = "macos")]
fn ask() -> Answer {
    use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
    let result = MessageDialog::new()
        .set_level(MessageLevel::Info)
        .set_title("Meetily data found")
        .set_description("Meetily data was found on this Mac. Move it to Noesis now? Nothing is deleted. If you skip, Noesis starts fresh and leaves the Meetily data where it is; you can move it by hand later.")
        .set_buttons(MessageButtons::OkCancelCustom("Move".into(), "Skip".into()))
        .show();
    match result {
        MessageDialogResult::Custom(s) if s == "Move" => Answer::Move,
        _ => Answer::Skip,
    }
}

#[cfg(not(target_os = "macos"))]
fn ask() -> Answer {
    Answer::Move
}

/// Startup step 1 with consent. Marker present: do nothing. No rename due: behave as `migrate_dirs`.
/// Otherwise ask: "Move" migrates; "Skip" (or closing the dialog) writes the marker and leaves the
/// data in place for good. A postponed move is not offered because this launch creates the Noesis
/// dirs, after which the rename can never run again.
pub fn migrate_dirs_after_asking() -> MigrationOutcome {
    let exists = |p: &Path| p.symlink_metadata().is_ok();
    let marker = app_data_pair().map(|m| m.to.join(SKIP_MARKER));
    if marker.as_ref().is_some_and(|m| m.exists()) {
        log::info!("migration: skipped for good ({} exists)", SKIP_MARKER);
        return MigrationOutcome::default();
    }
    if !needs_prompt(&plan(&legacy_pairs(), exists)) {
        return migrate_dirs();
    }
    match ask() {
        Answer::Move => migrate_dirs(),
        Answer::Skip => {
            if let Some(m) = marker {
                if let Some(dir) = m.parent() {
                    if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&m, "")) {
                        log::warn!("migration: could not write {}: {}", m.display(), e);
                    }
                }
            }
            log::info!("migration: skipped by the user; data left in place");
            MigrationOutcome::default()
        }
    }
}

/// Rewrite paths when the root moved this run, or an earlier run moved it but died before
/// the rewrite (old gone, new present). Both rewrites are idempotent.
fn needs_rewrite(moved_this_run: bool, old_exists: bool, new_exists: bool) -> bool {
    moved_this_run || (!old_exists && new_exists)
}

/// Startup step 2, after the database opens: if the recordings root moved, rewrite the
/// absolute paths that point into it, then log the one-line summary.
pub async fn finish<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: Option<&sqlx::SqlitePool>,
    outcome: &MigrationOutcome,
) {
    let rec = recordings_pair();
    let mut rewrote = 0;
    if needs_rewrite(outcome.moved.contains(&rec), rec.from.exists(), rec.to.exists()) {
        match pool {
            Some(pool) => match rewrite_folder_paths(pool, &rec.from, &rec.to).await {
                Ok(n) => rewrote = n,
                Err(e) => log::error!(
                    "migration: FAILED to rewrite meetings.folder_path from {} to {}: {}; the startup reconcile repairs rows found on disk",
                    rec.from.display(), rec.to.display(), e
                ),
            },
            None => log::error!("migration: no database pool, meetings.folder_path not rewritten"),
        }
        match crate::audio::recording_preferences::load_recording_preferences(app).await {
            Ok(mut prefs) => {
                if rewrite_save_folder(&mut prefs, &rec.from, &rec.to) {
                    match crate::audio::recording_preferences::save_recording_preferences(app, &prefs).await {
                        Ok(()) => log::info!("migration: save_folder now {}", prefs.save_folder.display()),
                        Err(e) => log::error!("migration: could not save rewritten save_folder: {}", e),
                    }
                }
            }
            Err(e) => log::error!("migration: could not load recording preferences: {}", e),
        }
    }
    log::info!(
        "migration: moved {} dirs, skipped {} (both exist), rewrote {} meeting paths",
        outcome.moved.len(), outcome.skipped.len(), rewrote
    );
    if !outcome.gated.is_empty() {
        log::error!("migration: {} dirs held back until the app data dir is resolved: {:?}", outcome.gated.len(), outcome.gated);
    }
    if !outcome.failed.is_empty() {
        log::error!("migration: {} dirs failed to move: {:?}", outcome.failed.len(), outcome.failed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    #[test]
    fn a_prompt_is_due_only_when_something_would_be_renamed() {
        let m = Move { from: "/old".into(), to: "/new".into() };
        assert!(needs_prompt(&[Step::Nothing, Step::Rename(m.clone())]));
        assert!(!needs_prompt(&[Step::Nothing, Step::SkipBothExist(m)]));
        assert!(!needs_prompt(&[]));
    }

    fn mv(from: &str, to: &str) -> Move {
        Move { from: PathBuf::from(from), to: PathBuf::from(to) }
    }

    #[test]
    fn needs_rewrite_decision() {
        assert!(needs_rewrite(true, false, true));
        assert!(needs_rewrite(false, false, true));
        assert!(!needs_rewrite(false, true, true));
        assert!(!needs_rewrite(false, true, false));
    }

    #[test]
    fn identifier_matches_tauri_conf() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"].as_str(), Some(crate::app_names::IDENTIFIER));
    }

    #[test]
    fn plan_covers_the_four_existence_combinations() {
        let pairs = vec![mv("/o1", "/n1"), mv("/o2", "/n2"), mv("/o3", "/n3"), mv("/o4", "/n4")];
        // o1 only; o2 + n2; n3 only; neither for 4
        let present = ["/o1", "/o2", "/n2", "/n3"];
        let steps = plan(&pairs, |p| present.iter().any(|x| Path::new(x) == p));
        assert_eq!(
            steps,
            vec![
                Step::Rename(mv("/o1", "/n1")),
                Step::SkipBothExist(mv("/o2", "/n2")),
                Step::Nothing,
                Step::Nothing,
            ]
        );
    }

    #[test]
    fn run_renames_and_keeps_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("meetily-recordings");
        std::fs::create_dir_all(old.join("dex/2026-10-01_10-00 Standup")).unwrap();
        std::fs::write(old.join("dex/2026-10-01_10-00 Standup/transcript.md"), "hello").unwrap();
        let new = tmp.path().join("noesis-recordings");
        let m = Move { from: old.clone(), to: new.clone() };

        let out = run(&plan(&[m.clone()], |p| p.exists()));
        assert_eq!(out.moved, vec![m.clone()]);
        assert!(out.failed.is_empty() && out.skipped.is_empty());
        assert!(!old.exists());
        assert_eq!(
            std::fs::read_to_string(new.join("dex/2026-10-01_10-00 Standup/transcript.md")).unwrap(),
            "hello"
        );

        // second launch: no-op
        let again = run(&plan(&[m], |p| p.exists()));
        assert!(again.moved.is_empty() && again.skipped.is_empty() && again.failed.is_empty());
        assert!(new.join("dex/2026-10-01_10-00 Standup/transcript.md").exists());
    }

    #[test]
    fn run_with_both_existing_touches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("com.meetily.ai");
        let new = tmp.path().join("com.noesis.app");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("meeting_minutes.sqlite"), "old").unwrap();
        std::fs::write(new.join("meeting_minutes.sqlite"), "new").unwrap();
        let m = Move { from: old.clone(), to: new.clone() };

        let out = run(&plan(&[m.clone()], |p| p.exists()));
        assert_eq!(out.skipped, vec![m]);
        assert!(out.moved.is_empty() && out.failed.is_empty());
        assert_eq!(std::fs::read_to_string(old.join("meeting_minutes.sqlite")).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(new.join("meeting_minutes.sqlite")).unwrap(), "new");
    }

    #[test]
    fn run_reports_a_failed_rename_and_continues() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("a");
        std::fs::create_dir_all(&old).unwrap();
        // parent of the target does not exist, so rename fails
        let bad = Move { from: old.clone(), to: tmp.path().join("missing/parent/a") };
        let old2 = tmp.path().join("b");
        std::fs::create_dir_all(&old2).unwrap();
        let good = Move { from: old2, to: tmp.path().join("c") };

        let out = run(&[Step::Rename(bad.clone()), Step::Rename(good.clone())]);
        assert_eq!(out.failed.len(), 1);
        assert_eq!(out.failed[0].0, bad);
        assert_eq!(out.moved, vec![good]);
        assert!(old.exists(), "failed source left untouched");
    }

    #[test]
    fn rewrite_root_is_component_wise() {
        let old = Path::new("/r/meetily-recordings");
        let new = Path::new("/r/noesis-recordings");
        assert_eq!(rewrite_root("/r/meetily-recordings", old, new).as_deref(), Some("/r/noesis-recordings"));
        assert_eq!(rewrite_root("/r/meetily-recordings/", old, new).as_deref(), Some("/r/noesis-recordings"));
        assert_eq!(
            rewrite_root("/r/meetily-recordings/dex/2026-10-01_10-00 Standup", old, new).as_deref(),
            Some("/r/noesis-recordings/dex/2026-10-01_10-00 Standup")
        );
        assert_eq!(rewrite_root("/r/meetily-recordings-old/x", old, new), None);
        assert_eq!(rewrite_root("/elsewhere/x", old, new), None);
        assert_eq!(rewrite_root("relative/meetily-recordings/x", old, new), None);
    }

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE meetings (id TEXT PRIMARY KEY, folder_path TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        for (id, fp) in [
            ("a", Some("/r/meetily-recordings/dex/x")),
            ("b", Some("/r/meetily-recordings-old/y")),
            ("c", None),
            ("d", Some("/custom/z")),
            ("e", Some("/r/meetily-recordings")),
        ] {
            sqlx::query("INSERT INTO meetings (id, folder_path) VALUES (?, ?)")
                .bind(id)
                .bind(fp)
                .execute(&pool)
                .await
                .unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn rewrite_folder_paths_changes_only_rows_under_old_root() {
        let pool = test_pool().await;
        let old = Path::new("/r/meetily-recordings");
        let new = Path::new("/r/noesis-recordings");
        assert_eq!(rewrite_folder_paths(&pool, old, new).await.unwrap(), 2);
        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT id, folder_path FROM meetings ORDER BY id").fetch_all(&pool).await.unwrap();
        assert_eq!(
            rows,
            vec![
                ("a".into(), Some("/r/noesis-recordings/dex/x".into())),
                ("b".into(), Some("/r/meetily-recordings-old/y".into())),
                ("c".into(), None),
                ("d".into(), Some("/custom/z".into())),
                ("e".into(), Some("/r/noesis-recordings".into())),
            ]
        );
        // idempotent
        assert_eq!(rewrite_folder_paths(&pool, old, new).await.unwrap(), 0);
    }

    #[test]
    fn legacy_pairs_are_distinct_and_named() {
        let pairs = legacy_pairs();
        for (i, a) in pairs.iter().enumerate() {
            assert!(pairs[i + 1..].iter().all(|b| b.from != a.from), "duplicate pair {:?}", a);
        }
        assert!(pairs.iter().any(|m| m.from.ends_with("com.meetily.ai") && m.to.ends_with("com.noesis.app")));
        assert!(pairs.iter().any(|m| m.from.ends_with("Meetily") && m.to.ends_with("Noesis")));
        let rec = recordings_pair();
        assert!(rec.from.ends_with("meetily-recordings"));
        assert_eq!(rec.to, crate::audio::get_default_recordings_folder());
        assert!(pairs.contains(&rec));
    }

    #[test]
    fn save_folder_is_rewritten_only_under_old_root() {
        let old = Path::new("/r/meetily-recordings");
        let new = Path::new("/r/noesis-recordings");
        let mut p = crate::audio::RecordingPreferences::default();
        p.save_folder = PathBuf::from("/r/meetily-recordings");
        assert!(rewrite_save_folder(&mut p, old, new));
        assert_eq!(p.save_folder, PathBuf::from("/r/noesis-recordings"));
        assert!(!rewrite_save_folder(&mut p, old, new), "second run is a no-op");
        p.save_folder = PathBuf::from("/r/meetily-recordings-old");
        assert!(!rewrite_save_folder(&mut p, old, new));
        assert_eq!(p.save_folder, PathBuf::from("/r/meetily-recordings-old"));
    }

    fn rec_rename() -> Step {
        Step::Rename(mv("/m/meetily-recordings", "/m/noesis-recordings"))
    }

    #[test]
    fn recordings_gated_on_app_data_step() {
        let app = mv("/d/com.meetily.ai", "/d/com.noesis.app");
        // skipped, both exist
        assert_eq!(gate_recordings(&Step::SkipBothExist(app.clone()), false, rec_rename()), Step::Nothing);
        // rename failed
        assert_eq!(gate_recordings(&Step::Rename(app.clone()), false, rec_rename()), Step::Nothing);
        // old app data missing
        assert_eq!(gate_recordings(&Step::Nothing, false, rec_rename()), rec_rename());
        // renamed this run
        assert_eq!(gate_recordings(&Step::Rename(app), true, rec_rename()), rec_rename());
    }

    fn setup_dirs(app_old: bool, app_new: bool) -> (tempfile::TempDir, Move, Move) {
        let tmp = tempfile::tempdir().unwrap();
        let app = Move { from: tmp.path().join("com.meetily.ai"), to: tmp.path().join("com.noesis.app") };
        let rec = Move { from: tmp.path().join("meetily-recordings"), to: tmp.path().join("noesis-recordings") };
        if app_old { std::fs::create_dir_all(&app.from).unwrap(); }
        if app_new { std::fs::create_dir_all(&app.to).unwrap(); }
        std::fs::create_dir_all(rec.from.join("dex")).unwrap();
        (tmp, app, rec)
    }

    #[test]
    fn migrate_holds_recordings_back_when_app_data_both_exist() {
        let (_tmp, app, rec) = setup_dirs(true, true);
        let out = migrate(Some(app.clone()), &[], rec.clone(), |p| p.exists());
        assert_eq!(out.skipped, vec![app]);
        assert_eq!(out.gated, vec![rec.clone()]);
        assert!(out.moved.is_empty());
        assert!(rec.from.join("dex").exists() && !rec.to.exists());
    }

    #[test]
    fn migrate_holds_recordings_back_when_app_data_rename_fails() {
        let (tmp, _, rec) = setup_dirs(false, false);
        let old = tmp.path().join("com.meetily.ai");
        std::fs::create_dir_all(&old).unwrap();
        let app = Move { from: old, to: tmp.path().join("missing/com.noesis.app") };
        let out = migrate(Some(app), &[], rec.clone(), |p| p.exists());
        assert_eq!(out.failed.len(), 1);
        assert_eq!(out.gated, vec![rec.clone()]);
        assert!(rec.from.exists() && !rec.to.exists());
    }

    #[test]
    fn migrate_moves_recordings_when_app_data_moved_or_absent() {
        let (_tmp, app, rec) = setup_dirs(true, false);
        let out = migrate(Some(app.clone()), &[], rec.clone(), |p| p.exists());
        assert_eq!(out.moved, vec![app.clone(), rec.clone()]);
        assert!(rec.to.join("dex").exists());
        // second launch: no-op
        let again = migrate(Some(app), &[], rec, |p| p.exists());
        assert!(again.moved.is_empty() && again.gated.is_empty() && again.skipped.is_empty());

        let (_tmp2, app2, rec2) = setup_dirs(false, true);
        let out2 = migrate(Some(app2), &[], rec2.clone(), |p| p.exists());
        assert_eq!(out2.moved, vec![rec2.clone()]);
        assert!(rec2.to.join("dex").exists());
    }
}
