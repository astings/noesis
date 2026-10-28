# Noesis rename, live project change and meeting-list actions: design

Status: approved 2026-10-28. Builds on branch `reskin/lean-ui` (lean UI reskin). Branch `phase-8/noesis`.

## 1. Goal

Five changes requested by the owner after the reskin:

1. Change a meeting's project while it is still recording (recordings start automatically and sometimes land in the wrong project).
2. The sidebar keeps the same button order collapsed and expanded; the collapse/expand toggle sits at the bottom in both.
3. Rename the app to Noesis, durably: bundle identifier, data directories, recordings root, env vars and MCP server, with a one-time migration of existing models, settings and meetings.
4. Drag a meeting onto another project section in the sidebar to move it.
5. Right-click a meeting in the sidebar: Delete, Rename, Move to section (submenu listing every project).

## 2. Live project change

Today the project is picked on the home page (`ProjectSelect`, disabled while recording), passed to `start_recording`, and the meeting folder `<root>/<project>/<stamp title>/` is created at start. `library::assign` refuses to move a meeting that is recording.

Design: the picker stays enabled while recording. Changing it calls a new Tauri command `set_recording_project(project: String)`, which validates the name like `set_project` does and stores it as the pending project of the active recording. The folder is not moved mid-recording (the saver and checkpoints hold its path). When the recording stops and the meeting row is saved, and before auto-enhance or any hook starts, the app moves the meeting to the pending project through the existing `library::assign` path, so `metadata.json`, the folder and SQLite stay consistent. Picking the original project again clears the pending change. The picker shows the pending project. If the move fails, the meeting stays where it was and the user gets a toast; nothing is lost.

## 3. Sidebar button order

Both modes read top to bottom: Home (wordmark when expanded), Search, Projects, spacer, New recording, Settings, collapse/expand toggle. The expanded header loses its collapse button; the toggle becomes the last row of the bottom block.

## 4. Meeting-list actions

- Right-click on a meeting row opens a context menu (Radix ContextMenu from the installed `radix-ui` package, new `ui/context-menu.tsx` in shadcn style): Rename, Move to section (submenu: every project from `list_projects_command`, current one marked and disabled), separator, Delete.
- Rename reuses the existing title dialog and `api_save_meeting_title`. Delete reuses the existing confirmation and must also move the meeting folder to the macOS Trash (today `api_delete_meeting` only drops DB rows, so the folder stays visible to the MCP server). Trash, not permanent deletion, so a mistake is recoverable.
- Drag and drop uses native HTML5 drag events, no library: meeting rows are draggable, project section headers are drop targets with a highlight, dropping calls `assign_meeting(meeting_id, project)`. The Tauri window sets `dragDropEnabled: false` so the webview receives HTML5 drag events (file drops are no longer used since audio import was removed).
- Move by menu or drag shows the existing error toasts when `assign` refuses (meeting recording or enhancing) and refreshes the list on success.

## 5. Rename to Noesis

| Item | Before | After |
|---|---|---|
| Bundle identifier | `com.meetily.ai` | `com.noesis.app` |
| productName, window title, Cargo package | `meetily` | `Noesis` / `noesis` |
| App data dir | `~/Library/Application Support/com.meetily.ai` | `.../com.noesis.app` |
| Secondary config dir (templates, notifications.json, model fallbacks) | `.../Application Support/Meetily` (case-insensitive, also `meetily`) | `.../Application Support/Noesis` |
| Recordings root | `~/Movies/meetily-recordings` | `~/Movies/noesis-recordings` |
| Hook env vars | `MEETILY_FOLDER/PROJECT/TITLE/MEETING_ID` | `NOESIS_*`, and `MEETILY_*` still set for compatibility |
| MCP server | `meetily` (`meetily-mcp`), root from `MEETILY_RECORDINGS` | `noesis` (`noesis-mcp`), root from `NOESIS_RECORDINGS`, then `MEETILY_RECORDINGS`, then `~/Movies/noesis-recordings`, then the old root if only it exists |
| Wiki automation allowed tools | `mcp__meetily` | `mcp__noesis` |
| User-visible strings | Meetily | Noesis |

Kept on purpose: remote asset names and URLs (onnxruntime zip, parakeet download host, release workflows and updater keys), the legacy Homebrew DB probe path, browser-storage keys (`meetily.*` localStorage keys, `MeetilyRecoveryDB`) because they are internal and renaming them would drop state.

### 5.1 Migration

A new module `migration.rs` runs once at the very start of `.setup()`, before the store plugin, model directories and database are touched. For each pair (app data dir, secondary config dir, recordings root): if the old path exists and the new one does not, rename it (same volume, atomic). If both exist, leave both, log a warning, use the new one. If only the new one exists, do nothing. After the database opens, if the recordings root was moved, rewrite `meetings.folder_path` by replacing the old root prefix with the new one, and rewrite `save_folder` in `recording_preferences.json` the same way. Every step logs. The migration is pure functions over paths with tests on temp dirs; it never deletes.

macOS ties microphone and screen-recording permissions to the bundle identifier, so Noesis asks for them again on first launch. That is expected.

## 6. Contracts

Section 5 of the personal-recorder spec changes only in names: the folder layout under the new root, metadata.json v2 and transcript.md are unchanged, MCP tool names and schemas are unchanged, the server is registered as `noesis`. The owner re-registers it with `claude mcp add --scope user noesis -- node <repo>/mcp/src/index.ts` and removes `meetily`.

## 7. Testing

Rust unit tests for the migration planner and executor (temp dirs), the folder_path rewrite (in-memory SQLite), the pending-project move, and Trash-on-delete path selection. MCP `node --test` for the root lookup order. Frontend type-check and build. A manual smoke run on real data is done by the owner, after a backup, because the first launch migrates real directories.
