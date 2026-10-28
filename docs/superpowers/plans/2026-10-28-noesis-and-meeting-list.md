# Noesis rename, live project change and meeting-list actions: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the five changes in `docs/superpowers/specs/2026-10-28-noesis-and-meeting-list-design.md`: sidebar button order, Trash-on-delete, sidebar context menu, drag and drop between project sections, live project change while recording, and the durable rename to Noesis with a one-time data migration.

**Architecture:** Frontend changes stay in the Sidebar and home page; Rust changes go in new small functions or modules (`migration.rs`) with marked edits in existing files. The rename is two tasks: names first, migration second; the app is never launched between them.

**Tech Stack:** Tauri 2 (Rust, sqlx SQLite), Next.js 14 static export, React 18, Tailwind 3, shadcn/ui on the unified `radix-ui` package, MCP server in TypeScript (`mcp/`, `node --test`).

## Global Constraints

- Work on branch `phase-8/noesis`. One commit per task (fix commits allowed), conventional messages (`feat:`, `fix:`, `style:`, `docs:`, `refactor:`).
- NEVER launch the app (`tauri dev`, `tauri build`, `clean_run.sh`), never run `pnpm dev`, never kill processes. NEVER read, move or write `~/Library/Application Support/com.meetily.ai`, `~/Library/Application Support/Meetily`, `~/Library/Application Support/com.noesis.app`, `~/Library/Application Support/Noesis`, `~/Movies/meetily-recordings` or `~/Movies/noesis-recordings`. Tests use temp dirs only.
- Gates for every task that touches them:
  - Frontend: `cd frontend && ./node_modules/.bin/tsc --noEmit` exits 0, then `pnpm build` succeeds. Use the local tsc binary, not `pnpm exec tsc` or a global tsc.
  - Rust: `cd frontend/src-tauri && cargo check` and `cargo test --lib` (all pass; `cargo check` alone skips test code).
  - MCP: `cd mcp && npm test`.
- Dead code is not touched: `audio_v2/`, `lib_old_complex.rs`, `audio/stt.rs`, `whisper_engine/parallel_processor.rs`, `backend/`.
- Contracts that must not change: folder layout `<root>/<project>/<YYYY-MM-DD_HH-MM title>/`, `metadata.json` v2 keys, `transcript.md` format, MCP tool names and schemas (`list_projects`, `list_meetings`, `get_transcript`, `assign_meeting`).
- UI follows the reskin tokens (zinc + indigo, `bg-background`, `text-foreground`, `text-muted-foreground`, `bg-accent`, `border-border`, `text-recording`, radius `rounded-md`), no raw Tailwind colors, works in light and dark.
- No new npm dependency. One new Rust dependency is allowed: the `trash` crate (Task 2).
- Moves of a meeting between projects always go through `library::assign` (Rust) / the `assign_meeting` command (frontend). Do not write a second move implementation.
- Project display name: `_inbox` shows as "Inbox" exactly as the sidebar does today; reuse its existing helper.

---

### Task 1: Sidebar button order

**Files:**
- Modify: `frontend/src/components/Sidebar/index.tsx` (expanded header ~452-471, bottom block ~513-534; collapsed rail `renderCollapsedIcons` ~258-337)

The collapsed rail is already in the target order: Home, Search, Projects, spacer, New recording, Settings, Expand toggle (`PanelLeftOpen`) last. The expanded sidebar must match: wordmark (routes home), Search input, Projects tree, then bottom block New recording, Settings, and the collapse toggle (`PanelLeftClose`, aria-label "Collapse sidebar") as the LAST row of the bottom block.

- [ ] **Step 1:** Remove the collapse button from the expanded header row; the header keeps only the wordmark button.
- [ ] **Step 2:** Add the collapse toggle as the last row of the expanded bottom block, styled like the Settings row (icon + label "Collapse"), same `toggleCollapse` handler.
- [ ] **Step 3:** Check the toggle lands at the same vertical spot in both modes: the rail's last item and the expanded bottom block's last row both sit at the bottom edge with the same bottom padding. Adjust padding so the icon does not jump when toggling.
- [ ] **Step 4:** Frontend gates. Commit `style: keep sidebar toggle at the bottom in both modes`.

### Task 2: Delete moves the meeting folder to the Trash

**Files:**
- Modify: `frontend/src-tauri/Cargo.toml` (add `trash = "5"`)
- Modify: `frontend/src-tauri/src/api/api.rs` (`api_delete_meeting`, ~762)
- Modify or create: a small function in `frontend/src-tauri/src/audio/library.rs`, e.g. `pub fn trash_meeting_folder(root: &Path, folder: &Path) -> Result<()>`

Today `api_delete_meeting` drops the DB rows only; the folder stays on disk and the MCP server keeps listing it.

- [ ] **Step 1:** Write failing tests for a pure guard `fn trashable_folder(root: &Path, folder: &Path) -> bool`: true only when `folder` is strictly inside `root` (after canonicalizing both when they exist) and is not `root` itself nor a project directory directly under root (depth must be exactly 2: `<root>/<project>/<meeting>`) or a legacy flat folder at depth 1 that contains `metadata.json`. False for `..` escapes and paths outside root.
- [ ] **Step 2:** Implement the guard and `trash_meeting_folder` (calls `trash::delete(folder)` only when the guard passes; a missing folder is Ok and logs).
- [ ] **Step 3:** In `api_delete_meeting`, read the meeting's `folder_path` before deleting the rows; after the DB delete succeeds, trash the folder with `get_default_recordings_folder()` as root. A Trash failure is logged and returned as an error string AFTER the DB delete (the row is gone either way; say so in the error: "Meeting removed from the list, but its folder could not be moved to the Trash: <err>").
- [ ] **Step 4:** Rust gates. Commit `feat: move deleted meetings to the Trash`.

### Task 3: Right-click menu on sidebar meetings

**Files:**
- Create: `frontend/src/components/ui/context-menu.tsx`
- Modify: `frontend/src/components/Sidebar/index.tsx` (`renderItem` ~355-436, `handleDelete` ~139-170, rename dialog ~560)
- Possibly modify: `frontend/src/components/Sidebar/SidebarProvider.tsx` (expose a refresh if not already exposed)

- [ ] **Step 1:** Create `ui/context-menu.tsx` in shadcn style, importing `{ ContextMenu as ContextMenuPrimitive } from 'radix-ui'` (the unified package, same import style as the other ui files in this repo; check how `ui/select.tsx` or `ui/tooltip.tsx` import and mirror it). Export `ContextMenu`, `ContextMenuTrigger`, `ContextMenuContent`, `ContextMenuItem`, `ContextMenuSeparator`, `ContextMenuSub`, `ContextMenuSubTrigger`, `ContextMenuSubContent`. Styling: `bg-popover text-popover-foreground border-border rounded-md shadow-md`, items `text-sm px-2 py-1.5 rounded-sm focus:bg-accent`, destructive item variant using `text-destructive`. If the `radix-ui` export does not exist at type-check time, stop and report BLOCKED (no new dependency allowed).
- [ ] **Step 2:** Wrap each meeting row (file items only, not project headers) in `ContextMenu` / `ContextMenuTrigger asChild`. Menu: "Rename", "Move to" submenu, separator, "Delete" (destructive).
- [ ] **Step 3:** Rename opens the existing edit-title dialog for that meeting (reuse the state and `handleEditConfirm` the pencil button uses). Delete opens the existing `ConfirmationModal` flow (`handleDelete`). Update the confirmation copy to say the folder goes to the Trash.
- [ ] **Step 4:** "Move to" lists every project from `list_projects_command` (fetch when the menu opens, or reuse a list already in the provider), sorted with `_inbox` first then alphabetical, shown with the sidebar's display names. The meeting's current project is shown with a check and disabled. Selecting one calls `invoke('assign_meeting', { meetingId, project })`, then refreshes meetings, then a success toast "Moved to <name>". On error, toast the error string (the Rust side already explains "being recorded" / "enhancing").
- [ ] **Step 5:** If the meeting is the one currently open (`/meeting-details?id=...`), after a successful move the page must still show it (it is the same id; just make sure the details page re-reads its folder path if it caches it — check `TranscriptButtonGroup.tsx` which already calls `assign_meeting` and follow what it does after a move).
- [ ] **Step 6:** Frontend gates. Commit `feat: right-click menu on sidebar meetings`.

### Task 4: Drag meetings between project sections

**Files:**
- Modify: `frontend/src-tauri/tauri.conf.json` (main window: `"dragDropEnabled": false`)
- Modify: `frontend/src/components/Sidebar/index.tsx`
- Check: `grep -rn "onDragDropEvent\|tauri://drag\|drag-drop" frontend/src` — nothing should still listen for Tauri file drops; if something does, remove the listener (audio import is gone).

- [ ] **Step 1:** Set `dragDropEnabled: false` on the main window in tauri.conf.json so the webview gets HTML5 drag events.
- [ ] **Step 2:** Meeting rows: `draggable`, `onDragStart` sets `e.dataTransfer.setData('application/x-noesis-meeting', meetingId)` and `effectAllowed = 'move'`; keep track of the dragged meeting's current project in component state for the highlight logic.
- [ ] **Step 3:** Project section headers AND the expanded area of a section (its meeting list) are drop targets: `onDragOver` calls `preventDefault()` only when the dragged data type is present and the target project differs from the source; show a highlight (`bg-accent` + `ring-1 ring-primary/40`) while hovered; `onDragLeave`/`onDrop` clear it. Collapsed sections still accept drops on the header.
- [ ] **Step 4:** `onDrop` calls the same move function Task 3 uses (extract it once into a `moveMeeting(meetingId, project)` in the Sidebar or provider; do not duplicate). Same toasts and refresh.
- [ ] **Step 5:** Dragging must not break click-to-open on rows nor the existing hover buttons; rows being renamed (inline input, if any) are not draggable.
- [ ] **Step 6:** Frontend gates. Commit `feat: drag meetings between project sections`.

### Task 5: Change the project while recording

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_manager.rs` (holds the saver; `set_project` ~521)
- Modify: `frontend/src-tauri/src/audio/recording_saver.rs` (`project` ~143)
- Modify: `frontend/src-tauri/src/audio/recording_commands.rs` (new command; stop path; `get_meeting_folder_path` ~1293)
- Modify: `frontend/src-tauri/src/lib.rs` (register the command)
- Modify: `frontend/src/app/page.tsx` (`ProjectSelect` ~191-214, currently `disabled` while recording), `frontend/src/lib/projects.ts` if a helper fits
- Reuse: `library::assign` (`frontend/src-tauri/src/audio/library.rs` ~287), project-name validation used by `set_project`

Behavior (spec section 2): the folder is not moved during the recording. A pending project is stored; after stop, once the meeting row exists in SQLite and before auto-enhance, post-enhance hook or wiki automation can start, the meeting is moved with `library::assign(pool, root, meeting_id, Some(pending), None)`.

- [ ] **Step 1:** Read the stop flow end to end and write down in the report, with file:line, (a) where the meetings row is inserted, (b) where `get_meeting_folder_path()` stops returning the live folder (assign refuses while they are equal), (c) where auto-enhance is triggered. The move must sit after (a) and (b) and before (c). If (c) can happen before (a), report DONE_WITH_CONCERNS with the sequence instead of guessing.
- [ ] **Step 2:** Write failing unit tests for a pure helper `fn pending_move(started_project: &str, pending: Option<&str>) -> Option<String>`: None when pending is None or equals started (after the same normalization `set_project` applies), Some(normalized) otherwise; invalid names normalize to `_inbox` exactly as `set_project` does.
- [ ] **Step 3:** Add `pending_project: Option<String>` to the recording state that lives for one recording (manager or saver, whichever owns `project`); reset it when a recording starts. New command `#[tauri::command] async fn set_recording_project(project: String) -> Result<String, String>` returns the normalized name; error "No recording in progress." when not recording.
- [ ] **Step 4:** At the point found in Step 1, if `pending_move` returns Some, call `library::assign`. On success log and emit the existing meetings-changed event the sidebar listens to (find what `assign_meeting` or recording stop emits; reuse it). On failure, log a warning and emit a toast-able event or return the message through the existing stop result; the meeting stays in its original project. The new folder path must be what auto-enhance and hooks receive.
- [ ] **Step 5:** Frontend: `ProjectSelect` stays enabled while recording. While recording, `onChange` calls `invoke('set_recording_project', { project })` and shows the returned project; it also calls `setLastProject` like the not-recording path does today. "New project..." works while recording too (it already creates the name client-side; the folder appears at move time).
- [ ] **Step 6:** Rust gates and frontend gates. Commit `feat: change a meeting's project while it is recording`.

### Task 6: Rename to Noesis (names, no data migration)

**Files (load-bearing, all must change):**
- `frontend/src-tauri/tauri.conf.json`: `productName` `"Noesis"`, `identifier` `"com.noesis.app"`, window `title` `"Noesis"`.
- `frontend/src-tauri/Cargo.toml`: package `name = "noesis"` (lib stays `app_lib`); let `cargo check` update `Cargo.lock`.
- `frontend/package.json`: `"name": "noesis"`.
- `frontend/src-tauri/src/console_utils/console_utils.rs` (~52, 90, 132): `log stream --process noesis`.
- `frontend/src-tauri/src/tray.rs:26`: tooltip `"Noesis"`.
- `frontend/src-tauri/src/notifications/types.rs` (~122-192) and `notifications/commands.rs` (~335, 385): `"Noesis"`, `"Noesis Error"`.
- `frontend/src-tauri/src/audio/recording_commands.rs:111` and `frontend/src/hooks/useRecordingStart.ts:18`: "Restart Noesis".
- Secondary config dir name `Meetily`/`meetily` -> `Noesis` in: `summary/templates/loader.rs` (~22-27), `notifications/settings.rs` (~118), model fallbacks `whisper_engine/whisper_engine.rs` (~154), `parakeet_engine/parakeet_engine.rs` (~294), `summary/summary_engine/model_manager.rs` (~148). Put the string in ONE place: `pub const APP_CONFIG_DIR_NAME: &str = "Noesis";` in a new `frontend/src-tauri/src/app_names.rs` (registered in lib.rs) and use it everywhere.
- Recordings root `meetily-recordings` -> `noesis-recordings` in `audio/recording_preferences.rs` (~115-150): `pub const RECORDINGS_DIR_NAME: &str = "noesis-recordings";` in `app_names.rs`. Also export `LEGACY_RECORDINGS_DIR_NAME = "meetily-recordings"`, `LEGACY_IDENTIFIER = "com.meetily.ai"`, `LEGACY_CONFIG_DIR_NAME = "Meetily"` there for Task 7.
- Env vars in `audio/post_enhance_hook.rs` (~81-84) and `audio/wiki_automation.rs` (~118-121): set `NOESIS_FOLDER`, `NOESIS_PROJECT`, `NOESIS_TITLE`, `NOESIS_MEETING_ID` AND keep setting the four `MEETILY_*` with the same values. Update the test at `post_enhance_hook.rs` ~123 to assert both.
- `audio/wiki_automation.rs` (~31-32, 198-199): allowed tool `mcp__noesis`, disallowed `mcp__noesis__assign_meeting`; prompt text naming the app says Noesis.
- `frontend/src/components/RecordingSettings.tsx` (~344, 349): hint text lists `NOESIS_*` (mention `MEETILY_*` still work), placeholder path uses `noesis-recordings`.
- `scripts/post-enhance.sh`: use `NOESIS_*` with fallback `${NOESIS_FOLDER:-$MEETILY_FOLDER}` etc., `mcp__noesis` tool names.
- `scripts/inject_transcript.py` (~34-42): `Application Support/com.noesis.app` (check what it actually points at and keep the same semantic).
- MCP: `mcp/package.json` name `noesis-mcp` and description; `mcp/src/index.ts:9` server name `'noesis'`, description strings say Noesis; `mcp/src/paths.ts` root lookup order: `NOESIS_RECORDINGS`, then `MEETILY_RECORDINGS`, then `~/Movies/noesis-recordings` if it exists, else `~/Movies/meetily-recordings` if it exists, else `~/Movies/noesis-recordings`. Add tests in `mcp/test/paths.test.ts` for each branch (inject env and home dir; follow how the existing test does it). Update `mcp/test/library.test.ts` tmpdir prefix. Run `npm run build` in `mcp/` if a build script exists so `dist/` matches.
- User-visible frontend strings: `src/app/metadata.ts` and `src/app/metadata.tsx` titles, Sidebar wordmark (~463), `PermissionWarning.tsx` (~101, 127), `TranscriptView.tsx` (~373), onboarding `DownloadProgressStep.tsx` (~341), `PermissionsStep.tsx` (~117), `SetupOverviewStep.tsx` (~29), `WelcomeStep.tsx` (~23).
- Cosmetic Rust strings worth changing: `build.rs:31`, `audio/capture/core_audio.rs:139` tap name, `audio/decoder.rs:294` temp prefix, whisper User-Agent (~1144, 1657).

**Do NOT change:** `build/onnxruntime.rs` zip name, `parakeet_engine.rs:127` download URL, `.github/workflows/*`, `OnboardingContext.tsx:114` Homebrew path, localStorage keys `meetily.*`, IndexedDB `MeetilyRecoveryDB`, `MEETILY_LLAMA_HELPER` (summary sidecar, dormant), anything in dead code, docs (Task 8).

- [ ] **Step 1:** `grep -rIn -i meetily` over `frontend/src`, `frontend/src-tauri/src`, `frontend/src-tauri/*.json`, `frontend/src-tauri/Cargo.toml`, `frontend/package.json`, `mcp/src`, `mcp/test`, `mcp/package.json`, `scripts/` and classify every hit as change / keep (with the reason from the list above). Put the table in the report.
- [ ] **Step 2:** Write the failing tests first (MCP paths order, env vars in post_enhance_hook test, `get_default_recordings_folder` ends with `noesis-recordings` in recording_preferences tests).
- [ ] **Step 3:** Apply the changes.
- [ ] **Step 4:** All gates (Rust, frontend, MCP). Re-run the grep; every remaining hit must be in the keep list. Commit `feat: rename the app to Noesis`.

### Task 7: One-time migration of Meetily data

**Files:**
- Create: `frontend/src-tauri/src/migration.rs`
- Modify: `frontend/src-tauri/src/lib.rs` (call at the very start of `.setup()`, before anything resolves `app_data_dir()`, the store plugin files, `set_models_directory`, or `initialize_database_on_startup` at ~586; if the store plugin at ~471 is initialized before `.setup()`, check whether it touches the disk at plugin init or only on first `store()` call and report which, with evidence)
- Uses: constants from `app_names.rs` (Task 6)

Design (spec 5.1):

```rust
pub struct Move { pub from: PathBuf, pub to: PathBuf }
pub enum Step { Rename(Move), SkipBothExist(Move), Nothing }
/// Pure: decides per pair from existence flags.
pub fn plan(pairs: &[Move], exists: impl Fn(&Path) -> bool) -> Vec<Step>;
/// Executes Rename steps with std::fs::rename; never deletes; logs every step.
pub fn run(steps: &[Step]) -> MigrationOutcome; // which pairs moved, which failed (with error)
/// Pure: rewrites a folder path if it starts with old_root (component-wise, not string prefix).
pub fn rewrite_root(path: &str, old_root: &Path, new_root: &Path) -> Option<String>;
/// Rewrites meetings.folder_path rows under old_root; returns rows changed.
pub async fn rewrite_folder_paths(pool: &SqlitePool, old_root: &Path, new_root: &Path) -> Result<u64>;
```

Pairs on macOS (use the same dir functions the app already uses: `dirs::data_dir()` for Application Support, `dirs::config_dir()`, `dirs::video_dir()`; mirror the per-OS choice in `recording_preferences.rs` for the recordings root):
1. `<data_dir>/com.meetily.ai` -> `<data_dir>/com.noesis.app` (this is what Tauri's `app_data_dir()` resolves to; verify against the Tauri 2 path resolver and state it in the report).
2. `<config_dir>/Meetily` -> `<config_dir>/Noesis` (on macOS config_dir == data_dir, case-insensitive: `meetily` is the same entry).
3. `<videos>/meetily-recordings` -> `<videos>/noesis-recordings`.

- [ ] **Step 1:** Tests first, all on `tempfile` dirs (add `tempfile` as a dev-dependency only if not already present): plan for each of the four existence combinations; run renames and leaves contents intact; run with both existing touches nothing; `rewrite_root` for exact root, nested path, sibling with shared string prefix (`/r/meetily-recordings-old/x` must NOT match `/r/meetily-recordings`), unrelated path; `rewrite_folder_paths` on an in-memory SQLite (follow `database/repositories/summary.rs` `test_pool` pattern, create just the `meetings` table columns needed).
- [ ] **Step 2:** Implement.
- [ ] **Step 3:** Wire: run `plan` + `run` at the start of `.setup()`. Keep the outcome; after the database is initialized (after ~586), if pair 3 was renamed, call `rewrite_folder_paths(old_root, new_root)` and rewrite `save_folder` in `recording_preferences.json` if it starts with the old root (load and save through the existing preferences functions). Log a single summary line: `migration: moved <n> dirs, skipped <m> (both exist), rewrote <k> meeting paths`.
- [ ] **Step 4:** Rust gates. Commit `feat: migrate Meetily data to Noesis on first launch`.

### Task 8: Docs

**Files:**
- Modify: `CLAUDE.md` (fork section: name Noesis, recordings root `~/Movies/noesis-recordings`, env vars `NOESIS_*` with `MEETILY_*` compatibility, bundle id; keep upstream sections, just update names where they describe this fork's behavior)
- Modify: `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md` (sections 4.6, 4.7, 4.8, 5 and decision 8.1: new names; add one line at the top "Renamed to Noesis on 2026-10-28, see 2026-10-28-noesis-and-meeting-list-design.md")
- Modify: `mcp/README.md` (registration `claude mcp add --scope user noesis -- node <repo>/mcp/dist/index.js`, how to remove the old `meetily` entry: `claude mcp remove --scope user meetily`, env var order)

- [ ] **Step 1:** Edit the three files. Do not rewrite README.md / CONTRIBUTING.md / upstream docs.
- [ ] **Step 2:** Commit `docs: Noesis rename`.
