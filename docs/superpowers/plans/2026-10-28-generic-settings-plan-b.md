# Generic Settings, Plan B: calls instead of Zoom, distribution hygiene

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect any call, not just Zoom, and make a first launch on someone else's Mac safe: ask before moving Meetily data, install the MCP server without a checkout. Sections 2 and 5 of `docs/superpowers/specs/2026-10-28-generic-settings-for-distribution-design.md`.

**Architecture:** The call signal becomes "another process is reading the microphone", read from CoreAudio process objects (macOS 14+), with the `CptHost` process check kept as the macOS 13 fallback; the state machine is untouched. The Meetily migration gains a native dialog in front of it. The MCP server is published to npm and a Settings button registers it with Claude Code.

**Tech Stack:** Rust (coreaudio-sys 0.2.17, rfd 0.16, tauri 2), TypeScript settings UI, npm.

## Global Constraints

- Runs after Plan A on the same branch `enhance/generic-settings` (Task 6 edits `PostProcessingSettings.tsx`, which Plan A creates).
- Rust tests: `cd frontend/src-tauri && cargo test --lib <filter>`. Known unrelated failure: `audio::device_detection::tests::test_builtin_mic_detection`. The lock file is `Cargo.lock` at the repository root.
- Type check: `cd frontend && ./node_modules/.bin/tsc --noEmit -p tsconfig.json`.
- Before editing Rust, `pgrep -fl "tauri dev"` must print nothing.
- macOS only. New native code lives under `#[cfg(target_os = "macos")]`.
- Generic call detection requires macOS 14; macOS 13 keeps the Zoom-only check. Poll period and debounce stay 5 s and 4 ticks.
- Copy: the settings group is "Calls". The dialog title is "Meetily data found". The npm package is `@alexandrelaine/noesis-mcp`; `noesis-mcp` is taken.
- Auto-start defaults off (done in Plan A); nothing in this plan turns it on.

---

### Task 0: Verify the CoreAudio signal on this Mac

**Files:**
- Modify: `frontend/src-tauri/src/call_detector.rs` (tests module, an ignored probe)
- Modify: `frontend/src-tauri/Cargo.toml:163-189` (macOS dependencies)

**Interfaces:**
- Produces: the four-character codes confirmed against the SDK header, used verbatim by Task 1.

- [ ] **Step 1: Confirm the constants in the SDK header**

```bash
H="$(xcrun --show-sdk-path)/System/Library/Frameworks/CoreAudio.framework/Headers/AudioHardware.h"
grep -n "kAudioHardwarePropertyProcessObjectList\|kAudioProcessPropertyPID\b\|kAudioProcessPropertyIsRunningInput\|kAudioObjectPropertyScopeGlobal\b\|kAudioObjectSystemObject\b" "$H"
```

Expected: five lines. The codes are `'prs#'`, `'ppid'`, `'piri'`, `'glob'` and `1`. If any differs, use the header's value in Task 1 and note it in the commit message.

- [ ] **Step 2: Add the dependency**

Under `[target.'cfg(target_os = "macos")'.dependencies]` (after line 171, `core-graphics`), add:

```toml
coreaudio-sys = "0.2.17"
```

- [ ] **Step 3: Add an ignored probe test**

At the end of the `tests` module in `call_detector.rs`:

```rust
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
```

This does not compile until Task 1 adds `process_input_states`. Do Task 1 Steps 1 to 4 first, then come back and run the probe:

1. Nothing recording, no call: the list shows `input=false` for every row, or only the Noesis test binary. `mic_in_use_by_others()` must be `Some(false)`.
2. Join a Teams or Google Meet test call (or a Zoom one): a row for that app (or the browser) shows `input=true`.
3. While recording in Noesis with no call: only Noesis's own pid shows `input=true`.

Record the three observations in the Task 1 commit message. If observation 2 fails for an app, note the app; the `CptHost` fallback still covers Zoom.

---

### Task 1: Microphone-in-use detection with the Zoom fallback

**Files:**
- Modify: `frontend/src-tauri/src/call_detector.rs:1-12,48-72` and tests

**Interfaces:**
- Produces: `pub fn process_input_states() -> Option<Vec<(i32, bool)>>` (macOS; pid and running-input flag for every CoreAudio process object)
- Produces: `pub fn mic_in_use_by_others() -> Option<bool>` (macOS)
- Produces: `pub fn choose_signal(mic: Option<bool>, zoom: Option<bool>) -> Option<bool>` (pure)
- Produces: `pub fn sample_in_call() -> Option<bool>` (macOS; what the loop polls)
- Consumes: `step`, `CallState`, `zoom_meeting_active` as they are

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib call_detector 2>&1 | tail -8`
Expected: compile errors for `choose_signal` and `others_reading_input`.

- [ ] **Step 3: Implement the signal**

Replace the module doc comment (lines 1-4) with:

```rust
//! Generic settings section 2 (was spec 4.5 Zoom only): call detection. A call is another
//! process reading the microphone, read from CoreAudio's process objects (macOS 14+). On
//! macOS 13, or when CoreAudio does not answer, the Zoom helper process `CptHost` is the
//! fallback. A background task samples every 5 seconds; a pure state machine turns the
//! samples into Started and Ended events, debouncing the end so a reconnect blip does not
//! stop a recording.
```

Insert after `zoom_meeting_active` (after line 60):

```rust
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
```

- [ ] **Step 4: Use it in the loop**

In `run` (lines 64-72):
- the `info!` line becomes

```rust
    let signal = if mic_in_use_by_others().is_some() { "microphone in use (CoreAudio)" } else { "Zoom process CptHost (macOS 13 fallback)" };
    info!("Call detector started ({}, every {}s, end debounce {} ticks)", signal, POLL_SECONDS, END_DEBOUNCE_TICKS);
```

- `tokio::task::spawn_blocking(zoom_meeting_active)` becomes `tokio::task::spawn_blocking(sample_in_call)`.
- the two log lines `"Zoom meeting ended; stopping the recording"` and `"Zoom meeting started; asking the frontend to start a recording"` become `"Call ended; stopping the recording"` and `"Call started; asking the frontend to start a recording"`.

- [ ] **Step 5: Run the tests, then the probe**

Run: `cd frontend/src-tauri && cargo test --lib call_detector 2>&1 | tail -8`
Expected: all pass (the probe is ignored).
Then run the three probe observations from Task 0 Step 3 and note them.

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/Cargo.toml Cargo.lock frontend/src-tauri/src/call_detector.rs
git commit -m "feat(calls): detect any app reading the microphone via CoreAudio process objects; CptHost stays the macOS 13 fallback

Probe on this Mac: <observation 1>; <observation 2>; <observation 3>."
```

---

### Task 2: Calendar tie-break on any meeting link

**Files:**
- Modify: `frontend/src-tauri/src/calendar_lookup.rs:6-29,78-93,125,149-160`

**Interfaces:**
- Produces: `Candidate.has_link: bool` (renamed from `zoom`), `pub const MEETING_LINK_HOSTS: [&str; 5]`, `pub fn has_meeting_link(haystack: &str) -> bool`

- [ ] **Step 1: Write the failing tests**

Replace `zoom_link_wins_then_latest_start` (lines 148-160) with:

```rust
    #[test]
    fn a_meeting_link_wins_then_latest_start() {
        let now = 10_000.0;
        let mut linked = ev("Teams one", now - 1800.0, now + 600.0);
        linked.has_link = true;
        let later = ev("Later start", now - 60.0, now + 600.0);
        assert_eq!(pick_title(now, &[linked.clone(), later.clone()]).as_deref(), Some("Teams one"));
        assert_eq!(
            pick_title(now, &[ev("Early", now - 1800.0, now + 600.0), later]).as_deref(),
            Some("Later start")
        );
    }

    #[test]
    fn meeting_links_of_the_usual_apps_are_recognised() {
        for h in ["https://zoom.us/j/1", "https://teams.microsoft.com/l/meetup-join/x", "https://meet.google.com/abc-defg-hij", "https://bcg.webex.com/meet/x", "https://facetime.apple.com/join#v=1"] {
            assert!(has_meeting_link(&format!("Agenda {} notes", h)), "{}", h);
        }
        assert!(!has_meeting_link("Room 4.12, bring the deck"));
    }
```

and line 125 in `ev`: `zoom: false` becomes `has_link: false`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib calendar_lookup 2>&1 | tail -8`
Expected: compile errors for `has_link` and `has_meeting_link`.

- [ ] **Step 3: Implement**

Lines 14-15 of the struct become:

```rust
    /// A Zoom, Teams, Meet, Webex or FaceTime link in the location, notes or URL.
    pub has_link: bool,
```

After `EARLY_JOIN_SECONDS` (line 19) add:

```rust
/// Hosts that mark an event as a call. Matched as substrings, case-insensitively.
pub const MEETING_LINK_HOSTS: [&str; 5] = ["zoom.us", "teams.microsoft.com", "meet.google.com", "webex.com", "facetime.apple.com"];

pub fn has_meeting_link(haystack: &str) -> bool {
    let h = haystack.to_lowercase();
    MEETING_LINK_HOSTS.iter().any(|host| h.contains(host))
}
```

Line 22 doc: `A Zoom link wins, then the latest start.` becomes `A meeting link wins, then the latest start.`; line 28 `(a.zoom, a.start)` and `(b.zoom, b.start)` become `(a.has_link, a.start)` and `(b.has_link, b.start)`; line 92 `zoom: haystack.contains("zoom.us"),` becomes `has_link: super::has_meeting_link(&haystack),`.

- [ ] **Step 4: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib calendar_lookup 2>&1 | tail -8`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/calendar_lookup.rs
git commit -m "feat(calendar): any meeting link breaks the tie, not only Zoom"
```

---

### Task 3: Settings copy: Calls, not Zoom

**Files:**
- Modify: `frontend/src/components/RecordingSettings.tsx:294-313`

- [ ] **Step 1: Rewrite the group**

Replace lines 294-313 with:

```tsx
      {/* generic settings section 2: call detection (any app using the microphone; Zoom process on macOS 13) */}
      <SettingsGroup title="Calls">
        <SettingRow
          label="Start recording when a call begins"
          hint="A call is any app using the microphone (Zoom, Teams, Meet, Webex, FaceTime). Records into the project selected on the home screen. Off by default: check the recording rules where you are."
          control={<Switch checked={preferences.auto_start_on_call} onCheckedChange={handleAutoStartToggle} disabled={saving} />}
        />
        <SettingRow
          label="Name the recording after the calendar event"
          hint="Uses the event in progress in macOS Calendar (Outlook/Exchange included) when a call auto-starts a recording"
          control={<Switch checked={preferences.calendar_meeting_name} onCheckedChange={handleCalendarNameToggle} disabled={saving || !preferences.auto_start_on_call} />}
        />
        <SettingRow
          label="Stop recording when the call ends"
          hint="About 20 seconds after the other app releases the microphone, the recording stops and is saved"
          control={<Switch checked={preferences.auto_stop_on_call_end} onCheckedChange={handleAutoStopToggle} disabled={saving} />}
        />
      </SettingsGroup>
```

- [ ] **Step 2: Type check and commit**

Run: `cd frontend && ./node_modules/.bin/tsc --noEmit -p tsconfig.json`. Expected: exit 0. `rg -n "Zoom" frontend/src/components/RecordingSettings.tsx` prints only the hint's app list.

```bash
git add frontend/src/components/RecordingSettings.tsx
git commit -m "feat(settings): Calls group replaces Zoom; hints name the microphone signal"
```

---

### Task 4: Ask before moving Meetily data

**Files:**
- Modify: `frontend/src-tauri/Cargo.toml:163-189` (add `rfd`)
- Modify: `frontend/src-tauri/src/migration.rs:191-195` and tests (from line 247)
- Modify: `frontend/src-tauri/src/lib.rs:501`

**Interfaces:**
- Produces: `pub const SKIP_MARKER: &str = "migration.skipped"`, `pub fn needs_prompt(steps: &[Step]) -> bool` (pure), `pub enum Answer { Move, NotNow, Never }`, `pub fn migrate_dirs_after_asking() -> MigrationOutcome`
- Consumes: `plan`, `legacy_pairs`, `app_data_pair`, `migrate_dirs`, `MigrationOutcome`, `Step`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module of `migration.rs`:

```rust
    #[test]
    fn a_prompt_is_due_only_when_something_would_be_renamed() {
        let m = Move { from: "/old".into(), to: "/new".into() };
        assert!(needs_prompt(&[Step::Nothing, Step::Rename(m.clone())]));
        assert!(!needs_prompt(&[Step::Nothing, Step::SkipBothExist(m)]));
        assert!(!needs_prompt(&[]));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend/src-tauri && cargo test --lib migration::tests::a_prompt 2>&1 | tail -5`
Expected: compile error, `needs_prompt` not found.

- [ ] **Step 3: Implement**

Add under `[target.'cfg(target_os = "macos")'.dependencies]` in `Cargo.toml` (next to `coreaudio-sys`):

```toml
rfd = "0.16"
```

(`tauri-plugin-dialog` already pins rfd 0.16.0 in `Cargo.lock`, so this adds no second copy.)

After `migrate_dirs` (line 195) in `migration.rs` add:

```rust
/// Written in the Noesis app data dir when the user answers "Never"; the dialog then stays away.
pub const SKIP_MARKER: &str = "migration.skipped";

/// Pure: a dialog is due when at least one legacy dir would be renamed.
pub fn needs_prompt(steps: &[Step]) -> bool {
    steps.iter().any(|s| matches!(s, Step::Rename(_)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Move,
    NotNow,
    Never,
}

/// Blocking native dialog. Runs before the Tauri window exists, so it uses rfd directly.
#[cfg(target_os = "macos")]
fn ask() -> Answer {
    use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
    let result = MessageDialog::new()
        .set_level(MessageLevel::Info)
        .set_title("Meetily data found")
        .set_description("Meetily data was found on this Mac. Move it to Noesis now? Nothing is deleted. \"Not now\" asks again next launch.")
        .set_buttons(MessageButtons::YesNoCancelCustom("Move".into(), "Not now".into(), "Never".into()))
        .show();
    match result {
        MessageDialogResult::Custom(s) if s == "Move" => Answer::Move,
        MessageDialogResult::Custom(s) if s == "Never" => Answer::Never,
        _ => Answer::NotNow,
    }
}

#[cfg(not(target_os = "macos"))]
fn ask() -> Answer {
    Answer::Move
}

/// Startup step 1 with consent: skip for good after "Never", ask when a rename is due, else behave as `migrate_dirs`.
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
        Answer::NotNow => {
            log::info!("migration: postponed by the user");
            MigrationOutcome::default()
        }
        Answer::Never => {
            if let Some(m) = marker {
                if let Err(e) = std::fs::create_dir_all(m.parent().unwrap_or(Path::new("/"))).and_then(|_| std::fs::write(&m, "")) {
                    log::warn!("migration: could not write {}: {}", m.display(), e);
                }
            }
            log::info!("migration: declined for good");
            MigrationOutcome::default()
        }
    }
}
```

`lib.rs` line 501: `migration::migrate_dirs()` becomes `migration::migrate_dirs_after_asking()`.

- [ ] **Step 4: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib migration 2>&1 | tail -8`
Expected: all pass.

- [ ] **Step 5: Try the dialog once**

```bash
mkdir -p "$HOME/Library/Application Support/com.meetily.ai" && cd frontend && pnpm run tauri:dev
```

Expected: the dialog appears before the window. Click "Not now": the app starts, the folder stays. Quit, launch again, click "Never": `~/Library/Application Support/com.noesis.app/migration.skipped` exists, the third launch is silent. Then clean up:

```bash
rmdir "$HOME/Library/Application Support/com.meetily.ai" && rm "$HOME/Library/Application Support/com.noesis.app/migration.skipped"
```

If `rfd` panics about the main thread, move the `migrate_dirs_after_asking()` call so it runs inside `.setup()` exactly where it is now (it already is; the panic would mean rfd needs `NSApplication` first, in which case call `objc2_app_kit::NSApplication::sharedApplication(MainThreadMarker::new().unwrap())` before `ask()` and add `objc2-app-kit = { version = "0.3", features = ["NSApplication", "NSResponder"] }`).

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/Cargo.toml Cargo.lock frontend/src-tauri/src/migration.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(migration): ask before moving Meetily data; Never writes migration.skipped"
```

---

### Task 5: Publish the MCP server to npm

**Files:**
- Modify: `mcp/package.json`, `mcp/README.md`

- [ ] **Step 1: Package metadata**

Replace `mcp/package.json` with:

```json
{
  "name": "@alexandrelaine/noesis-mcp",
  "version": "0.2.0",
  "type": "module",
  "description": "MCP server exposing the Noesis recordings library (projects, meetings, transcripts).",
  "license": "MIT",
  "repository": { "type": "git", "url": "https://github.com/alexandrelaine/noesis" },
  "bin": { "noesis-mcp": "src/index.ts" },
  "files": ["src", "README.md"],
  "engines": { "node": ">=22.18" },
  "scripts": {
    "start": "node src/index.ts",
    "test": "node --test test/"
  },
  "dependencies": {
    "@modelcontextprotocol/sdk": "1.32.1",
    "zod": "4.6.6"
  }
}
```

Fix the `repository.url` to the real remote (`git remote get-url origin`). `src/index.ts` already starts with `#!/usr/bin/env node`; make it executable: `chmod +x mcp/src/index.ts`.

- [ ] **Step 2: Dry run**

```bash
cd mcp && npm test && npm pack --dry-run 2>&1 | tail -15
```

Expected: tests pass; the tarball lists `src/index.ts`, `src/library.ts`, `src/paths.ts`, `src/search.ts`, `README.md`, `package.json` and nothing from `test/`.

- [ ] **Step 3: README**

In `mcp/README.md` replace the "Run" and "Register" paragraphs with:

```
Run without a checkout: `npx -y @alexandrelaine/noesis-mcp` (Node 22.18 or newer; the package ships TypeScript that Node runs directly).

Register for Claude Code (all projects): `claude mcp add --scope user noesis -- npx -y @alexandrelaine/noesis-mcp`, or click "Register MCP server with Claude Code" in Noesis, Settings, Recordings, Post-processing.

From a checkout instead: `node src/index.ts`, registered with `claude mcp add --scope user noesis -- node /ABSOLUTE/PATH/noesis/mcp/src/index.ts`.
```

Keep the root resolution sentence from Plan A Task 6.

- [ ] **Step 4: Commit, then the owner publishes**

```bash
git add mcp/package.json mcp/README.md mcp/src/index.ts
git commit -m "chore(mcp): publishable as @alexandrelaine/noesis-mcp with a bin entry"
```

Publishing is the owner's step (npm account, outward-facing): `cd mcp && npm login && npm publish --access public`. Until it is published, the Register button of Task 6 registers a command that fails on first use with "404 Not Found" from npx; the button's error text shows it.

---

### Task 6: Register button in Settings

**Files:**
- Modify: `frontend/src-tauri/src/audio/post_processing.rs` (new command and constants, tests)
- Modify: `frontend/src-tauri/src/lib.rs` (command registration, next to `get_default_agent_prompt_template`)
- Modify: `frontend/src/components/PostProcessingSettings.tsx` (new row before "Cost cap per run")

**Interfaces:**
- Produces: `pub const MCP_PACKAGE: &str`, `pub const MCP_REGISTER_ARGS: [&str; 9]`, `pub fn mcp_register_command() -> String`, Tauri commands `register_mcp_with_claude() -> Result<String, String>` and `get_mcp_register_command() -> String`
- Consumes: `resolve_agent`, `hook_path_env` from Plan A Task 2

- [ ] **Step 1: Write the failing test**

Add to the tests in `post_processing.rs`:

```rust
    #[test]
    fn the_register_command_uses_npx_and_user_scope() {
        assert_eq!(mcp_register_command(), "claude mcp add --scope user noesis -- npx -y @alexandrelaine/noesis-mcp");
        assert_eq!(MCP_REGISTER_ARGS.len(), 9);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend/src-tauri && cargo test --lib post_processing::tests::the_register 2>&1 | tail -5`
Expected: compile error.

- [ ] **Step 3: Implement**

After `resolve_agent` in `post_processing.rs` add:

```rust
/// Generic settings section 5: the published MCP server and the Claude Code registration for it.
pub const MCP_PACKAGE: &str = "@alexandrelaine/noesis-mcp";
pub const MCP_REGISTER_ARGS: [&str; 9] = ["mcp", "add", "--scope", "user", "noesis", "--", "npx", "-y", MCP_PACKAGE];

/// The same registration as a shell line, for Codex and Cursor users to adapt.
pub fn mcp_register_command() -> String {
    format!("claude {}", MCP_REGISTER_ARGS.join(" "))
}
```

After `get_default_agent_prompt_template` add:

```rust
#[tauri::command]
pub fn get_mcp_register_command() -> String {
    mcp_register_command()
}

/// Runs `claude mcp add ...` with the located or configured Claude Code. Returns its output, or the error text.
#[tauri::command]
pub async fn register_mcp_with_claude(agent_path: String) -> Result<String, String> {
    let claude = resolve_agent(&agent_path).ok_or_else(|| "Claude Code (claude) not found. Install it and sign in, then try again.".to_string())?;
    let output = tokio::task::spawn_blocking(move || {
        Command::new(claude).args(MCP_REGISTER_ARGS).env("PATH", hook_path_env()).output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("Could not start Claude Code: {}", e))?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)).trim().to_string();
    if output.status.success() {
        Ok(if text.is_empty() { "Registered. Restart Claude Code sessions to see the noesis tools.".to_string() } else { text })
    } else {
        Err(if text.is_empty() { format!("claude exited with {}", output.status) } else { text })
    }
}
```

Register both in `lib.rs` after `audio::post_processing::get_default_agent_prompt_template,`:

```rust
            audio::post_processing::get_mcp_register_command,
            audio::post_processing::register_mcp_with_claude,
```

- [ ] **Step 4: The row in the card**

In `PostProcessingSettings.tsx` add state after `deniedDraft`:

```tsx
  const [registerCommand, setRegisterCommand] = useState('');
  const [registering, setRegistering] = useState(false);
```

extend the `useEffect` with:

```tsx
    invoke<string>('get_mcp_register_command').then(setRegisterCommand).catch(() => setRegisterCommand(''));
```

add the handlers after `commitTools`:

```tsx
  const registerMcp = async () => {
    setRegistering(true);
    try {
      const message = await invoke<string>('register_mcp_with_claude', { agentPath: preferences.agent_path });
      toast.success('MCP server registered', { description: message });
    } catch (error) {
      toast.error('Could not register the MCP server', { description: error instanceof Error ? error.message : String(error) });
    } finally {
      setRegistering(false);
    }
  };
  const copyRegisterCommand = async () => {
    try {
      await navigator.clipboard.writeText(registerCommand);
      toast.success('Command copied');
    } catch {
      toast.error('Could not copy', { description: registerCommand });
    }
  };
```

with `import { toast } from 'sonner';` at the top, and insert this row before the "Cost cap per run" row:

```tsx
      <SettingRow
        label="MCP server"
        hint={
          <>
            Lets Claude Code read your meetings from any project. Needs Node 22.18 or newer.
            <span className="block mt-1 font-mono break-all">{registerCommand}</span>
          </>
        }
        control={
          <div className="flex gap-1">
            <Button type="button" variant="outline" size="sm" onClick={registerMcp} disabled={disabled || registering || (!claudePath && !preferences.agent_path)}>
              {registering ? 'Registering…' : 'Register with Claude Code'}
            </Button>
            <Button type="button" variant="ghost" size="sm" onClick={copyRegisterCommand} disabled={!registerCommand}>
              Copy command
            </Button>
          </div>
        }
      />
```

- [ ] **Step 5: Run the checks**

Run: `cd frontend/src-tauri && cargo test --lib post_processing 2>&1 | tail -8`. Expected: all pass.
Run: `cd frontend && ./node_modules/.bin/tsc --noEmit -p tsconfig.json`. Expected: exit 0.

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/src/audio/post_processing.rs frontend/src-tauri/src/lib.rs frontend/src/components/PostProcessingSettings.tsx
git commit -m "feat(settings): Register MCP server with Claude Code button, with the command to copy"
```

---

### Task 7: Docs and the spec pointers

**Files:**
- Modify: `CLAUDE.md:15` (sentence added), `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md` (section 4.5 and 4.6 pointers)

- [ ] **Step 1: CLAUDE.md**

Append to the fork paragraph (line 15, after the Plan A sentence about `recordings_root`):

```
Call detection is "another process reads the microphone" through CoreAudio process objects (`call_detector::mic_in_use_by_others`, macOS 14+), with the Zoom `CptHost` check as the macOS 13 fallback. The Meetily migration asks before moving (`migration::migrate_dirs_after_asking`; "Never" writes `migration.skipped` in the app data dir). The MCP server is published as `@alexandrelaine/noesis-mcp`; Settings has a Register button.
```

- [ ] **Step 2: Main spec pointers**

After the heading `### 4.5 Zoom call detection: auto-stop, optional auto-start` insert:

```
Generalised by [generic settings for distribution](2026-10-28-generic-settings-for-distribution-design.md), section 2: any process reading the microphone counts as a call on macOS 14; `CptHost` is the macOS 13 fallback; auto-start defaults off.
```

After the heading `### 4.6 MCP server` insert:

```
Distribution: published as `@alexandrelaine/noesis-mcp`, registered with `npx -y` or the Settings button (generic settings spec, section 5). The root also follows the app's `save_folder` preference (section 4).
```

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md
git commit -m "docs: pointers from the main spec and CLAUDE.md to the generic settings work"
```

---

## Manual acceptance (owner, after Task 7)

1. Dev app on a two-minute Teams or Google Meet test call with "Start recording when a call begins" on: the recording starts within 10 seconds of joining and stops within 30 seconds of leaving. The log shows `Call detector started (microphone in use (CoreAudio), ...)`.
2. Recording alone in Noesis with no call: no auto-stop fires.
3. The Meetily dialog sequence from Task 4 Step 5.
4. After `npm publish`: from a shell without the checkout on `PATH`, click "Register with Claude Code"; `claude mcp list` shows `noesis`, and a new Claude Code session lists the `mcp__noesis__*` tools.
5. Merge: `git checkout main && git merge --no-ff enhance/generic-settings`.
