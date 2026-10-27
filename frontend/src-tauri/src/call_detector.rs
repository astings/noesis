//! spec 4.5: Zoom call detection. Zoom on macOS runs the helper process `CptHost`
//! only while a meeting is in progress. A background task samples it every
//! 5 seconds; a pure state machine turns the samples into Started and Ended
//! events, debouncing the end so a reconnect blip does not stop a recording.

use log::{info, warn};
use tauri::{AppHandle, Runtime};

/// Poll period for the helper process check.
pub const POLL_SECONDS: u64 = 5;
/// Idle samples required after a call before `Ended` fires (4 x 5 s = 20 s).
pub const END_DEBOUNCE_TICKS: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallEvent {
    Started,
    Ended,
}

#[derive(Debug, Default, Clone)]
pub struct CallState {
    in_call: bool,
    idle_ticks: u32,
}

/// Feed one sample. Returns `Started` on the first in-call sample after idle, and
/// `Ended` once `end_debounce_ticks` consecutive idle samples follow a call.
pub fn step(state: &mut CallState, in_call: bool, end_debounce_ticks: u32) -> Option<CallEvent> {
    if in_call {
        state.idle_ticks = 0;
        if !state.in_call {
            state.in_call = true;
            return Some(CallEvent::Started);
        }
        return None;
    }
    if state.in_call {
        state.idle_ticks += 1;
        if state.idle_ticks >= end_debounce_ticks {
            state.in_call = false;
            state.idle_ticks = 0;
            return Some(CallEvent::Ended);
        }
    }
    None
}

/// True while Zoom's in-meeting helper `CptHost` is running.
/// ponytail: shells out to pgrep every 5 s instead of binding sysinfo's process API; swap if the cost ever shows.
#[cfg(target_os = "macos")]
pub fn zoom_meeting_active() -> Option<bool> { // spec 4.5
    std::process::Command::new("pgrep")
        .args(["-x", "CptHost"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .and_then(|s| s.code())
        .and_then(|c| match c { 0 => Some(true), 1 => Some(false), _ => None })
}

/// Background loop: sample, step, act according to the preferences.
#[cfg(target_os = "macos")]
pub async fn run<R: Runtime>(app: AppHandle<R>) {
    use tauri::Manager;
    let mut state = CallState::default();
    info!("Call detector started (CptHost, every {}s, end debounce {} ticks)", POLL_SECONDS, END_DEBOUNCE_TICKS);
    let mut recorded_during_call = false; // spec 4.5
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(POLL_SECONDS)).await;
        let Some(sample) = tokio::task::spawn_blocking(zoom_meeting_active).await.ok().flatten() else { continue };
        if sample && crate::audio::recording_commands::recording_live() { recorded_during_call = true; } // spec 4.5
        let Some(event) = step(&mut state, sample, END_DEBOUNCE_TICKS) else { continue };
        // spec 4.5: a new call starts fresh and an ended call resets, whatever happens next
        let during_call = recorded_during_call;
        if matches!(event, CallEvent::Started | CallEvent::Ended) { recorded_during_call = false; }
        let prefs = match crate::audio::recording_preferences::load_recording_preferences(&app).await {
            Ok(p) => p,
            Err(e) => {
                warn!("Call detector: could not load preferences, ignoring event {:?}: {}", event, e);
                continue;
            }
        };
        let recording = crate::audio::recording_commands::recording_live(); // spec 4.5
        match event {
            CallEvent::Ended if recording && during_call && prefs.auto_stop_on_call_end => {
                info!("Zoom meeting ended; stopping the recording");
                let app2 = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::audio::recording_commands::stop_for_system(app2, "auto-stop").await {
                        warn!("Call detector: auto-stop failed: {}", e);
                    }
                });
            }
            CallEvent::Started if !recording && prefs.auto_start_on_call => {
                info!("Zoom meeting started; asking the frontend to start a recording");
                match app.get_webview_window("main") {
                    Some(window) => {
                        let _ = window.eval("sessionStorage.setItem('autoStartRecording', 'true')");
                        let _ = window.eval("window.location.assign('/')");
                    }
                    None => warn!("Call detector: main window not available, auto-start skipped"),
                }
            }
            _ => {
                log::debug!("Call detector: {:?} ignored (recording={}, during_call={}, prefs stop={} start={})", event, recording, during_call, prefs.auto_stop_on_call_end, prefs.auto_start_on_call)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(samples: &[bool], debounce: u32) -> Vec<Option<CallEvent>> {
        let mut s = CallState::default();
        samples.iter().map(|&x| step(&mut s, x, debounce)).collect()
    }

    #[test]
    fn started_fires_on_first_in_call_sample() {
        assert_eq!(feed(&[false, false, true, true], 4), vec![None, None, Some(CallEvent::Started), None]);
    }

    #[test]
    fn ended_fires_after_debounce_ticks_of_idle() {
        let out = feed(&[true, false, false, false, false, false], 4);
        assert_eq!(out, vec![Some(CallEvent::Started), None, None, None, Some(CallEvent::Ended), None]);
    }

    #[test]
    fn a_blip_resets_the_debounce() {
        let out = feed(&[true, false, false, true, false, false, false, false], 4);
        assert_eq!(
            out,
            vec![Some(CallEvent::Started), None, None, None, None, None, None, Some(CallEvent::Ended)]
        );
    }

    #[test]
    fn idle_without_a_call_never_ends() {
        assert!(feed(&[false; 10], 4).iter().all(|e| e.is_none()));
    }

    #[test]
    fn a_second_call_starts_again() {
        let out = feed(&[true, false, false, false, false, true], 4);
        assert_eq!(out[4], Some(CallEvent::Ended));
        assert_eq!(out[5], Some(CallEvent::Started));
    }
}
