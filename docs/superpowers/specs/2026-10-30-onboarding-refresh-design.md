# Onboarding refresh

Date: 2026-10-30. Branch: `enhance/onboarding-refresh`.

## Goal

The first-launch onboarding is for people the owner shares Noesis with. It sets up a working recorder with defaults that work as-is: a colleague who presses Continue on every screen ends up recording, transcribing and enhancing. Today the flow is the upstream four screens (Welcome, Setup Overview, Download, Permissions), downloads only Parakeet and writes a dead summary config on finish.

## Screens (macOS: 6, other platforms: 5, no Permissions)

Each screen has defaults filled in. Continue saves the screen's settings, then advances. Back is available from screen 2 on. `current_step` keeps being saved, so quitting resumes on the same screen.

1. **Welcome.** Title "Welcome to Noesis", text "Records your calls and writes transcripts into project folders that Claude can read." Two lines: data stays on the device, works offline. "Get Started" starts the Parakeet download in the background (`startBackgroundDownloads`) and advances.
2. **You and your files.**
   - Recordings folder: `save_folder`, shown as a path with "Change…" (`select_directory`). Default from `get_recording_preferences` (which already resolves the default root).
   - Your name: `owner_name`, text input, prefilled.
   - Meeting languages: `transcription_languages`, the same chip buttons as Settings > Models (en, fr, de, es, it, nl, pt). At least one must stay selected.
3. **Transcription.**
   - Live: Parakeet `parakeet-tdt-0.6b-v3-int8`, required, with the progress bar and retry from the current `DownloadProgressStep`.
   - Enhance after each call: `auto_enhance` switch, default on.
   - Enhance model, shown when the switch is on: radio of Whisper `small` (~470 MB, fastest), `medium` (~1.5 GB), `large-v3` (~3 GB, most accurate, default). Preselected from `enhance_model` when it is one of the three, else `large-v3`.
   - Continue saves `auto_enhance`, `enhance_provider = "whisper"`, `enhance_model`, and calls `whisper_download_model` for the chosen model when it is not downloaded yet (`whisper_get_available_models`). The download runs in the background; Continue does not wait. A one-line note says it keeps downloading after setup.
4. **Permissions** (macOS only). The current `PermissionsStep` rows (microphone, system audio), without its Finish and Skip buttons: Continue advances, with a "Skip for now" link when not both are granted. The "Recording won't work without permissions" note stays.
5. **Calls.** Switches, saved on Continue:
   - Start recording when a call begins: `auto_start_on_call`, default off. Note under it: "Get consent from everyone on the call. Recording rules differ by country."
   - Stop recording when the call ends: `auto_stop_on_call_end`, default on.
   - Floating call overlay: `show_call_overlay`, default on.
   - Menu bar icon: `show_tray_icon`, default on.
   - Name meetings from your calendar: `calendar_meeting_name`, default off. Turning it on calls `calendar_request_access`; when access is refused the switch goes back off with a one-line message.
6. **Claude Code (optional).** Text: "Let Claude Code read your meetings." Embeds the existing `McpServerSettings` component (Register button, copy command, error when `claude` is missing). Buttons: **Finish**. Finish is disabled until Parakeet is downloaded, with the progress shown above it. No separate Skip: Finish without registering is the skip.

Finish calls `completeOnboarding()` then reloads, as today.

## Saving settings

A helper in the onboarding context, `savePreferences(patch)`, reads `get_recording_preferences`, merges the patch, writes `set_recording_preferences`. Each screen calls it on Continue with its keys only. On a write error the screen shows the error under its Continue button and stays.

## Rust: `complete_onboarding`

- Drop the `save_model_config("builtin-ai", model, …)` write. It replaced the summary model with an empty one, which auto titles/tags and the Ask fallback read.
- Save the Parakeet transcription config only when `get_transcript_config` returns none, so a re-run keeps a user's live model.
- Keep the `model` argument (ignored) so old callers compile; keep `model_status` fields readable.

## Steps and status

- Steps are numbered 1 to 6 as above on every platform. On other platforms `goNext` from 3 goes to 5 and `goPrevious` from 5 goes to 3; step 4 never renders there. The context exposes `isMac`, set once from `@tauri-apps/plugin-os`, and clamps to 1..6.
- A saved `current_step` above 6 is clamped to 6 (today it jumps to 3).
- Status `version` becomes `"2.0"`. Already-completed users are not asked again.

## Small fixes

- "Report issues on GitHub" goes to `https://github.com/astings/noesis/issues`. It moves to the Welcome screen.
- `SetupOverviewStep` is removed (its content is covered by screen 3). `WelcomeStep`, `DownloadProgressStep` (renamed in place to the Transcription screen), `PermissionsStep` are edited; `YouStep` and `CallsStep` and `ClaudeCodeStep` are new files in `components/onboarding/steps/`.
- Settings > Recordings gets a "Run setup again" button: `reset_onboarding_status_cmd`, then reload. Safe because finishing no longer overwrites settings.

## Out of scope

Summary and LLM providers, the Ask model, the title/tag prompt, audio retention, device and backend choice, file format, project overrides. They keep their defaults and stay in Settings. The legacy database import in `OnboardingContext` (`check_first_launch` and friends) stays as is. The Meetily migration dialog runs before onboarding and is unchanged.

## Testing

- Rust unit test: `complete_onboarding`'s config logic, as a function taking the existing transcript config: with none it returns "save parakeet", with one it returns "keep".
- `pnpm build` passes (type check).
- Manual pass, by the owner: back up `onboarding-status.json`, use "Run setup again", walk the six screens, check `recording_preferences.json` and that the summary model in Settings > Models is unchanged.
