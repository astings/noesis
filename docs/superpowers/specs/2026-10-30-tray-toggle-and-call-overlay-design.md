# Menu bar icon toggle and call overlay

Date: 2026-10-30. Branch `enhance/call-overlay`.

## 1. Menu bar icon toggle

- Preference `show_tray_icon: bool`, `#[serde(default = "default_true")]`, in `RecordingPreferences`.
- `tray::create_tray` still runs at setup; right after, `tray::apply_tray_visibility(app)` reads the preference and calls `app.tray_by_id("main-tray").set_visible(..)`.
- `save_recording_preferences` calls `apply_tray_visibility` after a successful save, so the toggle applies live.
- Tray menu updates keep running while hidden (harmless).
- The Dock icon stays; `RunEvent::Reopen` already brings the window back.
- UI: a switch "Show menu bar icon" in Settings, Recordings.

## 2. Call overlay

### Preference
`show_call_overlay: bool`, `#[serde(default)]` (off). Switch "Show call overlay while recording" in Settings, Recordings.

### Window (new module `src-tauri/src/overlay.rs`)
- `overlay::show(app)` creates (or shows) a `WebviewWindow` labelled `overlay`, URL `/overlay` (static export path `overlay.html` in release, matching how the main window URL resolves), size 380x48, `decorations(false)`, `transparent(true)`, `always_on_top(true)`, `visible_on_all_workspaces(true)`, `skip_taskbar(true)`, `resizable(false)`, `focused(false)`, `shadow(false)`. Default position top-center of the primary monitor.
- `overlay::hide(app)` closes it if present.
- `overlay::init(app)` at setup registers app-level listeners: `recording-started` shows it when the preference is on, `recording-stopped` hides it. No edits inside `recording_commands`' emit sites. `set_recording_preferences` calls `overlay::sync(app, show_call_overlay)`: off hides it, on shows it when a recording is in progress.
- `overlay_resize(height)` command: the page asks for 48 (collapsed) or up to 360 (answer open); width stays.
- Capabilities: add `overlay` to the windows list of the capability that grants `core:default`, events, and window drag (`core:window:allow-start-dragging`, `core:window:allow-set-position`).

### Page (`frontend/src/app/overlay/page.tsx`)
Pill, transparent body, rounded dark card:
`● 12:34 | ⏸/▶ | ⏹ | project ▾ | ? Ask…`
- Drag: `data-tauri-drag-region` on the pill background. After a move, the position is saved in `localStorage` (try/catch) and restored on mount via `getCurrentWindow().setPosition`.
- Timer: on mount `get_recording_state` gives the active duration and paused flag; ticks locally each second; `recording-paused` / `recording-resumed` events stop and restart it.
- Pause/Resume: `pause_recording` / `resume_recording`.
- Stop: new command `overlay_stop_recording`, which runs the existing tray stop path (refactor `tray::toggle_recording_handler`'s stop branch into `pub async fn stop_from_shortcut(app)` and call it from both). That emits `recording-stop-complete`, so the main window does its save and navigation as today.
- Project: `ProjectSelect` compact, value from new command `get_recording_project` (pending project, else the manager's project, else Inbox); change calls `set_recording_project`.
- `set_recording_project` emits `recording-project-changed` with the normalized name; the home page listens and updates its `project` state.
- Ask: an input; Enter calls `ask_meeting { question, meetingId: null, lastMinutes: null }`; the window grows to show the answer (scrollable, max 360px tall); Esc or × collapses back to 48. One answer at a time; a new question replaces it. Errors show inline.
- The page must not render the app shell (sidebar, title bar). The root layout gets a check: on `/overlay` it renders `children` only.

### Out of scope
Showing the overlay before a recording starts; hotkeys; multiple answers history.

## 3. Testing
- Rust unit test: old preferences JSON without the two keys deserializes to `show_tray_icon = true`, `show_call_overlay = false`.
- `cargo check`, `pnpm build` (static export includes `overlay.html`).
- Manual: toggle the menu bar icon both ways; record with the overlay on: timer, pause, resume, project change reflected on home, ask, stop lands on the meeting page; drag and relaunch keeps position.
