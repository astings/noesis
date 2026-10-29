//! Generic settings section 2 (was spec 4.5 Zoom only): call detection. A call is another
//! process reading the microphone, read from CoreAudio's process objects (macOS 14+). On
//! macOS 13, or when CoreAudio does not answer, the Zoom helper process `CptHost` is the
//! fallback. A background task samples every 5 seconds; a pure state machine turns the
//! samples into Started and Ended events, debouncing the end so a reconnect blip does not
//! stop a recording.

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

/// Pure: pid and running-input flag per process object, minus our own pid.
pub fn others_reading_input(states: &[(i32, bool)], own_pid: i32) -> bool {
    states.iter().any(|&(pid, running)| pid != own_pid && running)
}

/// Pure: CoreAudio when it answered, else the Zoom process check.
pub fn choose_signal(mic: Option<bool>, zoom: Option<bool>) -> Option<bool> {
    mic.or(zoom)
}

/// Every CoreAudio process object as (pid, is running input). None when the property is
/// unavailable (macOS 13) or CoreAudio fails; a single unreadable object is skipped.
#[cfg(target_os = "macos")]
pub fn process_input_states() -> Option<Vec<(i32, bool)>> {
    use coreaudio_sys::{AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID, AudioObjectPropertyAddress};
    use std::mem::size_of;
    use std::ptr::null;

    const fn fourcc(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }
    // AudioHardware.h, macOS 14 SDK. Defined here so the bindgen output's SDK does not matter.
    const SYSTEM_OBJECT: AudioObjectID = 1;
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");
    const PROCESS_OBJECT_LIST: u32 = fourcc(b"prs#");
    const PROCESS_PID: u32 = fourcc(b"ppid");
    const PROCESS_IS_RUNNING_INPUT: u32 = fourcc(b"piri");

    fn addr(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress { mSelector: selector, mScope: SCOPE_GLOBAL, mElement: 0 }
    }

    /// One fixed-size property of one object.
    fn get<T: Copy + Default>(object: AudioObjectID, selector: u32) -> Option<T> {
        let a = addr(selector);
        let mut value = T::default();
        let mut size = size_of::<T>() as u32;
        // SAFETY: `value` is a valid, writable T of exactly `size` bytes; CoreAudio writes at most `size`.
        let status = unsafe {
            AudioObjectGetPropertyData(object, &a, 0, null(), &mut size, &mut value as *mut T as *mut _)
        };
        (status == 0).then_some(value)
    }

    let list = addr(PROCESS_OBJECT_LIST);
    let mut size: u32 = 0;
    // SAFETY: `size` is a valid out-pointer; no data buffer is written.
    let status = unsafe { AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &list, 0, null(), &mut size) };
    if status != 0 {
        return None;
    }
    let mut ids = vec![0 as AudioObjectID; size as usize / size_of::<AudioObjectID>()];
    // SAFETY: `ids` holds exactly `size` bytes of AudioObjectID; CoreAudio writes at most `size`.
    let status = unsafe {
        AudioObjectGetPropertyData(SYSTEM_OBJECT, &list, 0, null(), &mut size, ids.as_mut_ptr() as *mut _)
    };
    if status != 0 {
        return None;
    }
    ids.truncate(size as usize / size_of::<AudioObjectID>());
    let states = ids
        .into_iter()
        .filter_map(|id| {
            let pid: i32 = get(id, PROCESS_PID)?;
            let running: u32 = get(id, PROCESS_IS_RUNNING_INPUT)?;
            Some((pid, running != 0))
        })
        .collect();
    Some(states)
}

/// True while a process other than Noesis reads the microphone. None when CoreAudio cannot say.
#[cfg(target_os = "macos")]
pub fn mic_in_use_by_others() -> Option<bool> {
    process_input_states().map(|s| others_reading_input(&s, std::process::id() as i32))
}

/// What the loop polls: CoreAudio first, the Zoom process as the fallback.
#[cfg(target_os = "macos")]
pub fn sample_in_call() -> Option<bool> {
    choose_signal(mic_in_use_by_others(), zoom_meeting_active())
}

/// Background loop: sample, step, act according to the preferences.
#[cfg(target_os = "macos")]
pub async fn run<R: Runtime>(app: AppHandle<R>) {
    use tauri::Manager;
    let mut state = CallState::default();
    let signal = if mic_in_use_by_others().is_some() { "microphone in use (CoreAudio)" } else { "Zoom process CptHost (macOS 13 fallback)" };
    info!("Call detector started ({}, every {}s, end debounce {} ticks)", signal, POLL_SECONDS, END_DEBOUNCE_TICKS);
    let mut recorded_during_call = false; // spec 4.5
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(POLL_SECONDS)).await;
        let Some(sample) = tokio::task::spawn_blocking(sample_in_call).await.ok().flatten() else { continue };
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
                info!("Call ended; stopping the recording");
                let app2 = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::audio::recording_commands::stop_for_system(app2, "auto-stop").await {
                        warn!("Call detector: auto-stop failed: {}", e);
                    }
                });
            }
            CallEvent::Started if !recording && prefs.auto_start_on_call => {
                info!("Call started; asking the frontend to start a recording");
                let title = if prefs.calendar_meeting_name {
                    tokio::task::spawn_blocking(crate::calendar_lookup::current_meeting_title).await.ok().flatten()
                } else {
                    None
                };
                match app.get_webview_window("main") {
                    Some(window) => {
                        if let Some(title) = title {
                            info!("Calendar event in progress: {}", title);
                            let js = serde_json::to_string(&title).unwrap_or_default();
                            let _ = window.eval(&format!("sessionStorage.setItem('autoStartTitle', {})", js));
                        }
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

    #[test]
    fn the_mic_signal_wins_and_zoom_is_the_fallback() {
        assert_eq!(choose_signal(Some(true), Some(false)), Some(true));
        assert_eq!(choose_signal(Some(false), Some(true)), Some(false), "CoreAudio answered: Zoom is not consulted");
        assert_eq!(choose_signal(None, Some(true)), Some(true));
        assert_eq!(choose_signal(None, Some(false)), Some(false));
        assert_eq!(choose_signal(None, None), None);
    }

    #[test]
    fn others_means_any_pid_but_ours_with_running_input() {
        let me = std::process::id() as i32;
        assert!(!others_reading_input(&[(me, true), (42, false)], me));
        assert!(others_reading_input(&[(me, true), (42, true)], me));
        assert!(!others_reading_input(&[], me));
    }

    /// Run by hand while on a call: `cargo test --lib call_detector::tests::probe -- --ignored --nocapture`.
    /// Prints every process CoreAudio knows about, with its pid, name and whether it reads input.
    #[test]
    #[ignore]
    #[cfg(target_os = "macos")]
    fn probe_process_objects() {
        for (pid, running) in process_input_states().expect("process objects need macOS 14") {
            let name = std::process::Command::new("ps").args(["-p", &pid.to_string(), "-o", "comm="]).output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
            println!("{pid:>6} input={running} {name}");
        }
        println!("own pid {}", std::process::id());
    }
}
