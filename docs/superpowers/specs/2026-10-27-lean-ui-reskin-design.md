# Lean UI reskin

Date: 2026-10-27. Branch: `reskin/lean-ui`.

Supersedes the non-goal in the recorder design spec (section 1 and decision 5 of section 9) that kept summaries and analytics "as they are". The user chose to remove them from the UI. Upstream merges of frontend summary code will conflict; that cost is accepted.

## Goal

A lean, modern UI with working dark mode that only shows the recorder flow:
record, live transcript, project folder, enhance, wiki automation.

## Removals

Frontend unless stated.

1. **About**: `About.tsx`, `Info.tsx`, the Info buttons in the sidebar, the About-on-click in `Logo.tsx`.
2. **Import audio**: `components/ImportAudio/*`, `contexts/ImportDialogContext.tsx`, `hooks/useImportAudio.ts`, the drag-drop listeners and overlay in `app/layout.tsx`, the sidebar entries. Rust: `audio/import.rs` and its 5 commands in `lib.rs`.
3. **Beta flag**: `importAndRetranscribe` gated both import and Enhance. Enhance becomes always available. `BetaSettings`, `types/betaFeatures.ts` and the Beta settings tab go.
4. **Fake level bars** next to the record button (`RecordingControls.tsx`, `barHeights` in `app/page.tsx`) and the dead `showPlayback` branch.
5. **Summaries**: `components/AISummary/*`, `components/BlockNoteEditor/*`, `MeetingDetails/Summary*`, `EmptyStateSummary`, `SummaryModelSettings`, `SummaryLanguageSettings`, `ModelSettingsModal`, the summary hooks under `hooks/meeting-details/`, `lib/blocknote-markdown.ts`, `lib/summary-content.ts`, `lib/onboarding-summary-model.ts`, `OllamaDownloadContext`, the Summary settings tab, the home `modelSettings` modal, the Sidebar LLM config fetch/save, the summary step of onboarding, BlockNote CSS in `globals.css`. Meeting details shows the transcript only, no tabs.
   - `lib/summary-language*`, `LanguagePickerPopover`, `useRecentLanguages` and `EditableTitle` were first kept, then deleted once the final review found no caller left. Transcription language selection (`LanguageSelection`) never depended on them.
   - Kept in Rust: `summary/` and the LLM code. They are entangled with retranscription and recording commands; with no UI caller they are dormant.
6. **Analytics**: `AnalyticsProvider`, `AnalyticsConsentSwitch`, `AnalyticsDataModal`. `lib/analytics.ts` first becomes no-op stubs so the 26 call sites keep compiling, then the call sites are removed. Rust `analytics/` stays registered but is never called.
7. **Update checker**: `UpdateCheckProvider`, `UpdateDialog`, `UpdateNotification`, `useUpdateCheck`, `services/updateService.ts`, `@tauri-apps/plugin-updater`, `@tauri-apps/plugin-process`, and their Rust plugin registration and `tauri.conf.json` updater config.
8. **Demo and version**: `app/notes/[id]`, the Sidebar fallback that routes to it, the hardcoded `v0.4.1`.
9. **Dead code**: `ComplianceNotification`, `ConsoleToggle`, `BluetoothPlaybackWarning`, `MainNav`, `CustomDialog`, `AudioPlayer.tsx`, `SettingTabs`, `MessageToast` import, `DatabaseImport/*` components, `molecules/form-components`, `ui/form`, `ui/command`, `ui/dropdown-menu` if unused after step 5, `BasicBlockNoteTest`.
10. **npm deps** that end up unused: `@remirror/*`, `@tiptap/*`, `@blocknote/*` (and the `prosemirror-*` overrides), `react-markdown`, `remark-gfm`, `lodash`, `@types/lodash`, `@hookform/resolvers`, `react-hook-form`, `@heroicons/react`, `zod`, `cmdk`, `@radix-ui/react-dropdown-menu`, `@tailwindcss/typography`. Each is removed only after `grep` confirms no import.

Kept: transcript recovery, onboarding (minus the summary step; `initialize_fresh_database` stays), projects, Enhance, wiki automation, Zoom automation, MCP, transcript and recording settings.

## Look

- Font: system stack (`-apple-system`, SF Pro). Source Sans 3 goes.
- Palette: zinc neutrals, one accent (indigo 500/400), red only for record and destructive actions.
- Theme: follows macOS via `prefers-color-scheme`. `tailwind.config.js` switches to `darkMode: 'media'`; `globals.css` holds light tokens in `:root` and dark tokens in `@media (prefers-color-scheme: dark)`. No toggle.
- Dark surfaces: background `#0b0b0c`, raised `zinc-900`, 1px borders instead of shadows.
- Density: radius 8px, tighter sidebar, settings title `text-lg`, quieter headers.
- Home: project pill, live transcript, one record button.
- Meeting details: transcript only; toolbar Copy, Open folder, Enhance, Project.

## Color pass

All hardcoded `gray/slate/white/black/blue` utilities move to the shadcn tokens already defined (`bg-background`, `bg-card`, `bg-muted`, `text-foreground`, `text-muted-foreground`, `border-border`, `bg-primary`, `text-primary`). Status colors (red, green, amber) stay as utilities with a `dark:` variant where contrast needs it.

## Order

Each step builds and is committed on its own.

1. Stub analytics, ungate Enhance.
2. Removals 1–10.
3. Tokens and dark mode, color pass over all files.
4. Visual polish: font, spacing, sidebar, home and meeting-details layout.
5. Verify.

## Verification

- `pnpm build` passes, and `cargo check` passes in `frontend/src-tauri`.
- `grep` finds no imports of removed modules.
- Screenshots of home, meeting details, settings tabs and onboarding in light and dark.
- Manual smoke: record, stop, open the meeting, Enhance, move it to another project.
