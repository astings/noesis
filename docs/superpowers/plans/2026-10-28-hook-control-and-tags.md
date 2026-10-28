# Hook control, status tags and submenu polish: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Implement section 8 of `docs/superpowers/specs/2026-10-28-noesis-and-meeting-list-design.md`.

**Architecture:** Rust owns hook dispatch, status and the manual command (Task 1); the frontend adds the setting, buttons and tags (Task 2) and the submenu polish (Task 3).

## Global Constraints

- Branch `phase-8/noesis`, one commit per task (fix commits allowed).
- The dev app (`tauri dev` + `next dev` on 3118) is RUNNING for the owner's smoke test. Do NOT run `pnpm build` (it overwrites `.next` under the dev server and breaks it). Do NOT launch or kill anything. Frontend gate: `cd frontend && ./node_modules/.bin/tsc --noEmit` exits 0. Rust gates: `cd frontend/src-tauri && cargo check` and `cargo test --lib` (use `CARGO_TARGET_DIR` unchanged; the dev app shares the target dir, so a lock wait is normal).
- Never read, move or write the owner's real data dirs (`~/Library/Application Support/com.noesis.app`, `~/Movies/noesis-recordings`). Tests use temp dirs.
- Dead code untouched (`audio_v2/`, `lib_old_complex.rs`, `audio/stt.rs`, `whisper_engine/parallel_processor.rs`). Contracts unchanged (folder layout, metadata.json v2, transcript.md, MCP tools).
- UI uses the reskin tokens only (`text-muted-foreground`, `text-primary`, `text-destructive`, `text-success`, `bg-accent`, `border-border`), works in light and dark, lucide icons.
- No new dependencies.

---

### Task 1: Hook dispatch, manual run, status (Rust)

**Files:** `frontend/src-tauri/src/audio/recording_preferences.rs`, `audio/retranscription.rs` (~175-205), `audio/post_enhance_hook.rs`, `audio/wiki_automation.rs`, `lib.rs` (register commands); a new small module `audio/hook_status.rs` if status tracking does not fit cleanly in post_enhance_hook.rs.

- [ ] **Step 1:** Preference `hook_auto_run: bool`, `#[serde(default)]` false, included in Default as false. Test: default false, and deserializing a JSON without the key gives false.
- [ ] **Step 2:** Extract the dispatch now inline in `retranscription.rs` (wiki row wins, else custom command) into one function, e.g. `post_enhance_hook::dispatch(app, prefs, ctx, force: bool) -> Result<HookKind, String>` where `HookKind` says which ran, and an error string "No hook configured. Set one in Settings, Recordings." when neither applies. `force` makes wiki automation ignore `hook.done` (thread it into `wiki_automation::run`; the existing skip test keeps passing with force=false; add a force=true test).
- [ ] **Step 3:** `retranscription.rs` calls dispatch with `force=false` only when `prefs.hook_auto_run` is true (and segments_count > 0, as today); otherwise logs "hook not auto-run (hook_auto_run off)".
- [ ] **Step 4:** Custom command writes `hook.done` on exit 0 through the existing `spawn_logged` `on_exit` (same marker content style as wiki automation). Test with a temp dir and `sh -c 'exit 0'` / `exit 1` (wait for the marker with a short bounded poll).
- [ ] **Step 5:** Status: track running and failed meeting ids in memory (start → running; exit → done if success else failed; spawn error → failed). `#[tauri::command] list_hook_statuses() -> Vec<{meeting_id, status}>` returns running/failed from memory and `done` for meetings whose folder has `hook.done` (read folders from the meetings table like `list_enhance_statuses` does; look at how it is implemented and mirror it). Emit `hook-status-changed` `{meeting_id, status}` on each transition (needs an AppHandle; pass one into dispatch).
- [ ] **Step 6:** `#[tauri::command] run_meeting_hook(meeting_id: String) -> Result<String, String>`: look up the folder, refuse "This meeting is being recorded right now." if it equals the live folder (reuse the check `library::assign` uses), load prefs, `dispatch(force=true)`, return a short description ("Wiki automation started" / "Hook started").
- [ ] **Step 7:** Rust gates. Commit `feat: hook runs on demand, auto-run off by default, hook status`.

### Task 2: Settings toggle, Run hook buttons, status tags (frontend)

**Files:** `frontend/src/components/RecordingSettings.tsx` (preferences type + the hook/wiki section), `frontend/src/components/MeetingDetails/TranscriptButtonGroup.tsx` (Enhance button ~104), `frontend/src/components/Sidebar/index.tsx` (`EnhanceBadge` ~39-57, context menu), `frontend/src/components/Sidebar/SidebarProvider.tsx` (statuses ~96-140), meeting page header component (find where the title/project header renders in `app/meeting-details`).

- [ ] **Step 1:** Add `hook_auto_run` to the frontend `RecordingPreferences` type and a Switch "Run hook automatically after enhance" (help text: "Off: run it from the meeting page or the right-click menu.") in the hook / wiki automation area of RecordingSettings, saved like the other toggles.
- [ ] **Step 2:** SidebarProvider: fetch `list_hook_statuses` alongside `list_enhance_statuses`, store `hookStatus` on meetings and sidebar items, listen to `hook-status-changed` like `enhance-status-changed`. Toast on transition to failed: "Hook failed" with "See hook.log in the meeting folder."
- [ ] **Step 3:** Replace `EnhanceBadge` with a `MeetingStatusTags` component (new file `frontend/src/components/MeetingStatusTags.tsx`) taking `enhanceStatus?` and `hookStatus?`: two small lucide icons (`Sparkles` for enhance, `Webhook` for hook), 12px, with `title`/`aria-label` tooltips. Colors: done `text-muted-foreground`, queued `text-muted-foreground/50`, running `text-primary animate-pulse`, failed `text-destructive`; none renders nothing for that icon. Labels: "Enhanced", "Enhance queued", "Enhancing transcript", "Enhance failed", "Hook ran", "Hook running", "Hook failed". Use it in the sidebar row and in the meeting page header.
- [ ] **Step 4:** "Run hook" button next to Enhance in TranscriptButtonGroup (same button style, `Webhook` icon), and a "Run hook" item in the sidebar right-click menu (above the separator before Delete). Both call `invoke('run_meeting_hook', { meetingId })`, toast success with the returned string, toast errors with the error string. Disable the button while that meeting's hook status is running.
- [ ] **Step 5:** tsc gate. Commit `feat: run hook button, auto-run setting and status tags`.

### Task 3: Submenu chevron and New project

**Files:** `frontend/src/components/ui/context-menu.tsx` (`ContextMenuSubTrigger` ~50-60), `frontend/src/components/Sidebar/index.tsx` (Move to submenu), reuse `normalizeProjectName` from `frontend/src/lib/projects.ts` and look at how `ProjectSelect.tsx` handles "New project...".

- [ ] **Step 1:** `ContextMenuSubTrigger` renders its children then `<ChevronRight className="ml-auto h-4 w-4" />` (shadcn convention).
- [ ] **Step 2:** At the end of the Move to submenu: separator, then "New project…" (lucide `FolderPlus`). It opens a Dialog with an input; validation and normalization exactly like ProjectSelect's new-project path (reuse its helper; extract a shared one if the logic is inline there, rather than copying it). Confirm calls the existing `moveMeeting(meetingId, name)`. Check that `library::assign` creates the project directory when it does not exist (read it; if it does not, report BLOCKED rather than adding Rust code).
- [ ] **Step 3:** tsc gate. Commit `feat: submenu chevron and new project from the move menu`.
