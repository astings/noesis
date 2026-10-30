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
    if !enabled {
        hide(app);
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if crate::is_recording().await {
            show(&app);
        }
    });
}

pub fn init<R: Runtime>(app: &AppHandle<R>) {
    let a = app.clone();
    app.listen("recording-started", move |_| {
        let a = a.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(p) = crate::audio::recording_preferences::load_recording_preferences(&a).await {
                if p.show_call_overlay {
                    show(&a);
                }
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
