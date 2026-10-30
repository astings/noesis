# Menu bar icon toggle and call overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A preference to hide the menu bar (tray) icon, and an opt-in always-on-top pill overlay during recordings with timer, pause/resume, stop, project and quick ask.

**Architecture:** Two new preferences in `RecordingPreferences`. Tray visibility applied at setup and on save. A new Rust module `overlay.rs` owns a second webview window `overlay` (route `/overlay`), opened/closed by app-level listeners on the existing `recording-started` / `recording-stopped` events. The overlay page reuses existing commands (`pause_recording`, `resume_recording`, `set_recording_project`, `ask_meeting`) plus three small new ones.

**Tech Stack:** Tauri 2.6 (Rust), Next.js 14 static export, React 18, TypeScript.

Spec: `docs/superpowers/specs/2026-10-30-tray-toggle-and-call-overlay-design.md`.

## Global Constraints

- Branch `enhance/call-overlay`. Commit after each task. Use `git` from `/Users/lainealexandre/Code/Perso/noesis`.
- Additive changes, new modules where possible (upstream merges stay feasible).
- Preference names exactly: `show_tray_icon` (default `true`), `show_call_overlay` (default `false`). Old prefs JSON without them must still load.
- Window label exactly `overlay`; route `/overlay`; tray id stays `main-tray`.
- Rust gates: from `frontend/src-tauri`: `cargo check` and `cargo test --lib <filter>` (one filter per invocation).
- Frontend gates: from `frontend`: `pnpm exec tsc --noEmit` and `pnpm build`. Do NOT run `pnpm lint` (interactive, unusable).
- Rust errors returned to the frontend as `Result<_, String>`; log with `log::info!/error!`.
- Copy: "Show menu bar icon", "Show call overlay while recording".

---

### Task 1: Preferences, tray visibility, Settings switches

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs` (struct ~line 15, `Default` impl ~line 130-160, `set_recording_preferences` ~line 317, tests module ~line 570)
- Modify: `frontend/src-tauri/src/tray.rs` (add `apply_tray_visibility`)
- Modify: `frontend/src-tauri/src/lib.rs` (~line 567, after `tray::create_tray`)
- Modify: `frontend/src/components/RecordingSettings.tsx` (prefs interface ~line 27, defaults ~line 56, handlers ~line 75, rows in the group ending ~line 311)

**Interfaces:**
- Produces: `RecordingPreferences.show_tray_icon: bool`, `RecordingPreferences.show_call_overlay: bool`; `pub fn tray::apply_tray_visibility<R: Runtime>(app: &AppHandle<R>, visible: bool)`.

- [ ] **Step 1: Failing test** in the `tests` module of `recording_preferences.rs`:

```rust
#[test]
fn tray_icon_defaults_on_and_overlay_off() {
    let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
    let p: RecordingPreferences = serde_json::from_str(old).unwrap();
    assert!(p.show_tray_icon);
    assert!(!p.show_call_overlay);
    let d = RecordingPreferences::default();
    assert!(d.show_tray_icon && !d.show_call_overlay);
}
```

- [ ] **Step 2:** `cargo test --lib tray_icon_defaults` → fails to compile (no field).

- [ ] **Step 3: Fields** in the struct (next to the other booleans):

```rust
    /// Show the menu bar (tray) icon. The Dock icon stays either way.
    #[serde(default = "default_true")]
    pub show_tray_icon: bool,
    /// Show the always-on-top call overlay while recording (spec 2026-10-30 tray-toggle-and-call-overlay).
    #[serde(default)]
    pub show_call_overlay: bool,
```

and in `Default`: `show_tray_icon: true, show_call_overlay: false,`.

- [ ] **Step 4:** `cargo test --lib tray_icon_defaults` → PASS.

- [ ] **Step 5: Tray visibility.** In `tray.rs`:

```rust
/// Shows or hides the menu bar icon (preference `show_tray_icon`).
pub fn apply_tray_visibility<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        if let Err(e) = tray.set_visible(visible) {
            log::error!("Failed to set tray visibility: {}", e);
        }
    }
}
```

In `lib.rs` right after the `tray::create_tray` block in setup:

```rust
            let app_for_tray = _app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(p) = audio::recording_preferences::load_recording_preferences(&app_for_tray).await {
                    tray::apply_tray_visibility(&app_for_tray, p.show_tray_icon);
                }
            });
```

(Check the exact name/visibility of `load_recording_preferences`; it is used by `get_recording_preferences`.)

In `set_recording_preferences`, after a successful save: `crate::tray::apply_tray_visibility(&app, preferences.show_tray_icon);`

- [ ] **Step 6: Settings UI.** In `RecordingSettings.tsx` add `show_tray_icon: boolean; show_call_overlay: boolean;` to the prefs interface, `show_tray_icon: true, show_call_overlay: false` to the defaults, two handlers in the same style as `handleAutoStopToggle`, and two `SettingRow`s at the end of the same `SettingsGroup` as "Stop recording when the call ends":

```tsx
        <SettingRow
          label="Show menu bar icon"
          hint="Hidden, the app stays reachable from the Dock"
          control={<Switch checked={preferences.show_tray_icon} onCheckedChange={handleTrayIconToggle} disabled={saving} />}
        />
        <SettingRow
          label="Show call overlay while recording"
          hint="A small always-on-top bar with timer, pause, stop, project and a quick question"
          control={<Switch checked={preferences.show_call_overlay} onCheckedChange={handleCallOverlayToggle} disabled={saving} />}
        />
```

- [ ] **Step 7:** `cargo check` (src-tauri) and `pnpm exec tsc --noEmit` (frontend) pass.

- [ ] **Step 8: Commit** `feat(settings): menu bar icon toggle and call overlay preference`.

---

### Task 2: Rust overlay window and commands

**Files:**
- Create: `frontend/src-tauri/src/overlay.rs`
- Modify: `frontend/src-tauri/src/lib.rs` (`mod overlay;`, call `overlay::init` in setup after the tray block, register commands in `generate_handler!`)
- Modify: `frontend/src-tauri/src/tray.rs` (extract stop path)
- Modify: `frontend/src-tauri/src/audio/recording_commands.rs` (`get_recording_project`, emit in `set_recording_project` ~line 1324)
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs` (`set_recording_preferences` calls `overlay::sync`)
- Modify: `frontend/src-tauri/tauri.conf.json` (capability `main` windows list ~line 46)

**Interfaces:**
- Consumes: `RecordingPreferences.show_call_overlay` (Task 1).
- Produces (for Task 3): Tauri commands `overlay_resize(height: f64) -> Result<(), String>`, `overlay_stop_recording() -> Result<(), String>`, `get_recording_project() -> Result<String, String>`; event `recording-project-changed` with payload `String` (normalized project name); window label `overlay` loading route `/overlay`.

- [ ] **Step 1: Extract tray stop.** In `tray.rs`, move the body of the `if crate::is_recording().await { ... }` branch of `toggle_recording_handler` (from `set_tray_state(.., Stopping)` through the `match stop_result`) into:

```rust
/// Stops the recording the way the tray does: Rust stop, then `recording-stop-complete`
/// so the main window runs its save and navigation (used by the tray and the call overlay).
pub async fn stop_from_shortcut<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> { ... }
```

returning `Err(e)` on failure (keep the existing logging and tray-state revert). `toggle_recording_handler` calls `let _ = stop_from_shortcut(&app_clone).await;`. Behaviour of the tray must be unchanged.

- [ ] **Step 2: `overlay.rs`:**

```rust
//! Call overlay window (spec 2026-10-30 tray-toggle-and-call-overlay).
use tauri::{AppHandle, Listener, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "overlay";
const WIDTH: f64 = 380.0;
const COLLAPSED: f64 = 48.0;
const MAX_HEIGHT: f64 = 360.0;

pub fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.show();
        return;
    }
    let mut b = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay".into()))
        .title("Noesis overlay")
        .inner_size(WIDTH, COLLAPSED)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .shadow(false);
    if let Ok(Some(m)) = app.primary_monitor() {
        let scale = m.scale_factor();
        let size = m.size().to_logical::<f64>(scale);
        let pos = m.position().to_logical::<f64>(scale);
        b = b.position(pos.x + (size.width - WIDTH) / 2.0, pos.y + 40.0);
    }
    if let Err(e) = b.build() {
        log::error!("Failed to open call overlay: {}", e);
    }
}

pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.close();
    }
}

/// Applies the preference now: off hides, on shows during a recording.
pub fn sync<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    if !enabled { hide(app); return; }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if crate::is_recording().await { show(&app); }
    });
}

pub fn init<R: Runtime>(app: &AppHandle<R>) {
    let a = app.clone();
    app.listen("recording-started", move |_| {
        let a = a.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(p) = crate::audio::recording_preferences::load_recording_preferences(&a).await {
                if p.show_call_overlay { show(&a); }
            }
        });
    });
    let a = app.clone();
    app.listen("recording-stopped", move |_| hide(&a));
}

#[tauri::command]
pub async fn overlay_resize<R: Runtime>(app: AppHandle<R>, height: f64) -> Result<(), String> {
    let w = app.get_webview_window(LABEL).ok_or("No overlay")?;
    w.set_size(tauri::LogicalSize::new(WIDTH, height.clamp(COLLAPSED, MAX_HEIGHT)))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn overlay_stop_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    crate::tray::stop_from_shortcut(&app).await
}
```

Window creation must happen on the main thread on macOS: if `build()` misbehaves from the async task, wrap `show` body in `app.run_on_main_thread(...)`. Verify `Listener` is the right trait import for `app.listen` in Tauri 2.6 (it is `tauri::Listener`). `is_recording` lives at `crate::is_recording` (tray.rs uses it).

- [ ] **Step 3: Wire up** in `lib.rs`: `mod overlay;` with the other mods; in setup after the tray block `overlay::init(_app.handle());`; add `overlay::overlay_resize, overlay::overlay_stop_recording, audio::recording_commands::get_recording_project,` to `generate_handler!`. In `set_recording_preferences` after the tray call: `crate::overlay::sync(&app, preferences.show_call_overlay);`.

- [ ] **Step 4: Project command and event** in `recording_commands.rs`:

```rust
/// The live recording's project: the pending change if any, else the start project, else Inbox.
#[tauri::command]
pub async fn get_recording_project() -> Result<String, String> {
    let guard = RECORDING_MANAGER.lock().unwrap();
    let Some(manager) = guard.as_ref() else { return Err("No recording in progress.".to_string()) };
    let id = manager.get_meeting_id();
    if let Some((mid, p)) = PENDING_PROJECT.lock().unwrap().as_ref() {
        if Some(mid) == id.as_ref() { return Ok(p.clone()); }
    }
    Ok(manager.get_project().unwrap_or_else(|| super::library::INBOX.to_string()))
}
```

(Adapt to the real types of `PENDING_PROJECT` and `get_meeting_id()`; check what `pending_move` returns: if it yields `None` when the new project equals the start project, the fallback to `get_project()` covers it.)

In `set_recording_project`, before `Ok(normalized)`: `let _ = app.emit("recording-project-changed", normalized.clone());` (needs `tauri::Emitter` in scope; it already emits elsewhere in the file). Drop the manager guard before emitting if the borrow checker requires.

- [ ] **Step 5: Capability.** In `tauri.conf.json` capability `main`: `"windows": ["main", "overlay"]`, and add `"core:window:allow-set-position"` to its permissions.

- [ ] **Step 6:** `cargo check` passes; `cargo test --lib tray_icon_defaults` still passes.

- [ ] **Step 7: Commit** `feat(overlay): call overlay window, stop/resize/project commands`.

---

### Task 3: Overlay page, layout bypass, home sync

**Files:**
- Create: `frontend/src/app/overlay/page.tsx`
- Modify: `frontend/src/app/layout.tsx` (bypass shell on `/overlay`)
- Modify: `frontend/src/app/page.tsx` (listen `recording-project-changed`)

**Interfaces:**
- Consumes: commands `get_recording_state` (returns JSON; read it to find the active duration seconds and paused fields — see `recording_commands.rs` ~line 1287), `pause_recording`, `resume_recording`, `overlay_stop_recording`, `get_recording_project`, `set_recording_project { project }` → `string`, `ask_meeting { question, meetingId: null, lastMinutes: null }` → `{ answer, from_seconds, to_seconds, segments }`, `overlay_resize { height }`; events `recording-paused`, `recording-resumed`, `recording-project-changed` (string). Component `ProjectSelect` (`value`, `onChange`, `compact`, `className`) from `@/components/ProjectSelect`.

- [ ] **Step 1: Layout bypass.** In `layout.tsx` add `import { usePathname } from 'next/navigation'`, `const pathname = usePathname()` with the other hooks, and before the main `return`:

```tsx
  if (pathname?.startsWith('/overlay')) {
    return (
      <html lang="en" style={{ background: 'transparent' }}>
        <body className="font-sans" style={{ background: 'transparent' }}>
          <TooltipProvider delayDuration={150}>{children}</TooltipProvider>
        </body>
      </html>
    )
  }
```

Hooks stay unconditional above it. Check `globals.css` sets no opaque `html`/`body` background that would override (line ~152); if it does, the inline style wins.

- [ ] **Step 2: Overlay page** `src/app/overlay/page.tsx` (`'use client'`). Requirements:
  - Root: `div` full window, transparent; pill `div` with `data-tauri-drag-region`, rounded-full, dark translucent (`bg-neutral-900/90 text-white`), height 40px, items in a row: red dot (pulses unless paused) + `mm:ss` (or `h:mm:ss`) timer, pause/resume icon button, stop icon button (red), `ProjectSelect compact`, ask input (`placeholder="Ask…"`). Use `lucide-react` icons already in the repo (`Pause`, `Play`, `Square`, `X`).
  - Timer: on mount `invoke('get_recording_state')` sets seconds and paused; `setInterval` 1s increments when not paused; `listen('recording-paused')` / `listen('recording-resumed')` flip paused.
  - Project: on mount `invoke<string>('get_recording_project')`; `onChange` → `invoke<string>('set_recording_project', { project })` and set the returned value; `listen<string>('recording-project-changed', e => setProject(e.payload))`.
  - Stop: `invoke('overlay_stop_recording')`; the window closes itself via the Rust listener.
  - Ask: Enter → set loading, `overlay_resize { height: 360 }`, `invoke('ask_meeting', ...)`, show answer (or error string) in a scrollable panel under the pill with a × button; × or Esc → clear and `overlay_resize { height: 48 }`. Render answer text with `whitespace-pre-wrap` (no markdown lib).
  - Position: on mount read `localStorage.getItem('overlayPos')` (try/catch, JSON `{x,y}` logical) → `getCurrentWindow().setPosition(new LogicalPosition(x, y))`; `getCurrentWindow().onMoved(({ payload }) => ...)` saves `payload.toLogical(await getCurrentWindow().scaleFactor())` in a try/catch. Imports from `@tauri-apps/api/window` and `@tauri-apps/api/dpi`.
  - Errors from any invoke: show a short inline red text in the panel; no toasts (no Toaster in this window).

- [ ] **Step 3: Home sync.** In `src/app/page.tsx` add an effect: `listen<string>('recording-project-changed', e => { setProject(e.payload); setLastProject(e.payload); })` with unlisten cleanup.

- [ ] **Step 4:** `pnpm exec tsc --noEmit` passes; `pnpm build` passes and `ls out | grep overlay` shows `overlay.html` (or `overlay/`).

- [ ] **Step 5: Commit** `feat(overlay): pill page with timer, pause, stop, project and ask`.

---

### Task 4: Docs

- [ ] Add a bullet to `CLAUDE.md` "This fork" section: menu bar icon pref `show_tray_icon`, call overlay (`overlay.rs`, window `overlay`, route `/overlay`, pref `show_call_overlay`, shows on `recording-started` and closes on `recording-stopped`), spec link.
- [ ] Commit `docs: call overlay and menu bar toggle in CLAUDE.md`.
