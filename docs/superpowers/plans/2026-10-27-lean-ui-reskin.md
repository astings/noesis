# Lean UI Reskin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Strip the UI down to the recorder flow and reskin it (zinc + indigo, system font, dark mode following macOS).

**Architecture:** Frontend-only removals first, behind two decoupling moves (no-op analytics, ungated Enhance), so every commit builds. Then a token pass that swaps hardcoded Tailwind colors for the shadcn CSS variables already in `globals.css`, with `darkMode: 'media'`. Then layout polish matching the approved mockup.

**Tech Stack:** Next.js 14 (static export), React 18, Tailwind 3, shadcn/ui, Tauri 2 (Rust).

Spec: `docs/superpowers/specs/2026-10-27-lean-ui-reskin-design.md`. Mockup: https://claude.ai/artifact/HGMthAj8XMrt756v3Q6KVJ (also at the scratchpad file the controller can share).

## Global Constraints

- Branch `reskin/lean-ui`, work in `/Users/lainealexandre/Code/Perso/ok-meetily`. One commit per task, conventional prefix (`refactor:`, `style:`, `chore:`).
- No frontend test suite exists. The gate for every task is, run in `frontend/`:
  - `pnpm exec tsc --noEmit` — error count must not exceed the baseline recorded in Task 1.
  - `pnpm build` — must succeed.
  - Tasks touching Rust also: `cargo check` in `frontend/src-tauri` (macOS; use `--features metal` only if plain check fails on features).
- Never touch `audio_v2/`, `lib_old_complex.rs`, `audio/stt.rs`, `whisper_engine/parallel_processor.rs`.
- Keep in Rust: `summary/`, LLM code, `analytics/` module (dormant, no UI caller). Keep `initialize_fresh_database`.
- Keep in frontend: `lib/summary-language*`, `LanguagePickerPopover`, `TranscriptRecovery`, onboarding (minus summary step), `ProjectSelect`, `RetranscribeDialog`, `WikiAutomationSettings`, `@tauri-apps/plugin-store`, `ui/popover`, `date-fns`, `radix-ui`.
- Contracts from the recorder spec section 5 (folder layout, `metadata.json`, `transcript.md`, MCP tools) must not change.
- Colors: zinc neutrals, accent indigo (light `#4f46e5`, dark `#818cf8`), red only for record and destructive. Dark background `#0b0b0c`, raised `zinc-900` (`#18181b`), borders `#27272a`. Radius 8px. Font: `-apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", system-ui, sans-serif`.
- Copy: Me/Them labels, "New recording", "Enhance", "Open folder". No new user-facing jargon.
- Before deleting a file, `grep -rn "<ModuleName>" frontend/src` to find every importer and fix them in the same task.

---

### Task 1: Baseline, no-op analytics, ungate Enhance, drop Beta

**Files:**
- Modify: `frontend/src/lib/analytics.ts` (make every exported method a no-op, keep signatures)
- Modify: `frontend/src/components/MeetingDetails/TranscriptButtonGroup.tsx` (~lines 101, 122: remove the `importAndRetranscribe` gate so Enhance always renders)
- Delete: `frontend/src/components/BetaSettings.tsx`, `frontend/src/types/betaFeatures.ts` and any beta-feature context/hook that only serves them (grep `betaFeatures`, `BetaFeature`)
- Modify: `frontend/src/app/settings/page.tsx` (remove Beta tab and import)

**Interfaces:**
- Produces: `Analytics.*` remains importable with identical signatures but does nothing. Task 5 deletes the call sites.

- [ ] **Step 1: Record baseline.** In `frontend/`: `pnpm install` if needed, then `pnpm exec tsc --noEmit 2>&1 | grep -c 'error TS'` and `pnpm build`. Write the number in the commit message body as `tsc-baseline: N`. If `pnpm build` fails at baseline, stop and report.
- [ ] **Step 2: Stub analytics.** Replace each method body in `lib/analytics.ts` with an empty body returning the declared type (`Promise.resolve()` for async, `false`/`null` where a value is returned). Remove all `invoke(...)` calls from that file.
- [ ] **Step 3: Ungate Enhance.** In `TranscriptButtonGroup.tsx` delete the beta-flag read and condition; Enhance button and `RetranscribeDialog` render unconditionally. Import-audio code may still read the flag; replace those reads with `false` for now (Task 3 deletes them).
- [ ] **Step 4: Remove Beta tab and files.** Delete Beta tab entry from `TABS` and its `TabsContent`. Delete files listed above after grep shows no remaining importers.
- [ ] **Step 5: Gate.** tsc count ≤ baseline, `pnpm build` passes.
- [ ] **Step 6: Commit** `refactor: no-op analytics, always show Enhance, drop Beta tab`.

### Task 2: Remove About, notes demo, version label, fake level bars, dead code

**Files:**
- Delete: `components/About.tsx`, `components/Info.tsx`, `app/notes/` (whole dir), `components/ComplianceNotification.tsx`, `components/ConsoleToggle.tsx`, `components/BluetoothPlaybackWarning.tsx`, `components/MainNav*`, `components/CustomDialog*`, `components/AudioPlayer.tsx`, `components/SettingTabs.tsx`, `components/DatabaseImport/` (components only; `OnboardingContext` Rust calls stay), `components/molecules/form-components/`, `components/ui/form.tsx`, `components/BlockNoteEditor/BasicBlockNoteTest.tsx`. Confirm each is an orphan or fix importers.
- Modify: `components/Logo.tsx` (no About on click; plain logo), `components/Sidebar/index.tsx` (remove Info buttons ~:543 and ~:816, hardcoded `v0.4.1` ~:818, `/notes/` fallback route ~:585, unused imports `SettingTabs`, `MessageToast`, `ComplianceNotification`).
- Modify: `components/RecordingControls.tsx` (remove the 3-bar animation ~:466-478, the `barHeights` prop, the dead `showPlayback` branch ~:345-379), `app/page.tsx` (remove `barHeights` state and its interval ~:30, :182-195).

- [ ] **Step 1:** For each delete target run `grep -rn "<basename without ext>" frontend/src` and fix importers.
- [ ] **Step 2:** Apply the deletions and edits above. In Sidebar the `/notes/` fallback: meetings whose id has no `-` now route to `/meeting-details?id=...` like the rest.
- [ ] **Step 3: Gate** (tsc ≤ baseline, build).
- [ ] **Step 4: Commit** `refactor: remove About, notes demo, fake level bars and dead components`.

### Task 3: Remove audio import (frontend + Rust)

**Files:**
- Delete: `components/ImportAudio/` (dir), `contexts/ImportDialogContext.tsx`, `hooks/useImportAudio.ts`
- Modify: `app/layout.tsx` (remove `ConditionalImportDialog` ~:39, `tauri://drag-*` listeners ~:161-206, `ImportDropOverlay` ~:256, `ImportDialogProvider` ~:248), `components/Sidebar/index.tsx` (Import Audio entries ~:494, ~:799 and `useImportDialog` ~:84)
- Delete: `frontend/src-tauri/src/audio/import.rs`; Modify: `audio/mod.rs` (drop `mod import`/re-exports), `lib.rs` (~:853-857 remove `select_and_validate_audio_command`, `validate_audio_file_command`, `start_import_audio_command`, `cancel_import_command`, `is_import_in_progress_command`)

- [ ] **Step 1:** `grep -rn "import::\|ImportAudio\|useImportDialog\|useImportAudio\|_import_\|import_audio" frontend/src frontend/src-tauri/src` and list hits. Confirm `audio/retranscription.rs` does not depend on `import.rs`; if it uses a helper from it, move that helper into `retranscription.rs` instead of deleting.
- [ ] **Step 2:** Apply frontend removals.
- [ ] **Step 3:** Apply Rust removals.
- [ ] **Step 4: Gate:** tsc, build, `cargo check`.
- [ ] **Step 5: Commit** `refactor: remove audio import`.

### Task 4: Remove summary UI

**Files:**
- Delete: `components/AISummary/`, `components/BlockNoteEditor/`, `components/MeetingDetails/SummaryPanel.tsx`, `SummaryGeneratorButtonGroup.tsx`, `SummaryUpdaterButtonGroup.tsx`, `components/EmptyStateSummary.tsx`, `components/SummaryModelSettings.tsx`, `components/SummaryLanguageSettings.tsx`, `components/ModelSettingsModal.tsx`, `hooks/meeting-details/useSummaryGeneration*`, `useTemplates*`, `useModelConfiguration*`, `lib/blocknote-markdown.ts`, `lib/summary-content.ts`, `lib/onboarding-summary-model.ts`, `contexts/OllamaDownloadContext.tsx`, plus `components/BuiltInModelManager.tsx` if only summary uses it (grep).
- Modify:
  - `components/MeetingDetails/MeetingDetailsSplitView.tsx`: no tabs, render the transcript panel full width.
  - `app/meeting-details/page.tsx` and `page-content.tsx`: drop `api_get_summary` call (~:219), summary state, `ModelConfig` usage.
  - `hooks/meeting-details/useMeetingData*`, `useCopyOperations*`: drop BlockNote ref and summary copy; keep transcript copy.
  - `app/_components/SettingsModal.tsx`: remove `modelSettings` modal; keep device, language, model selector (transcription), error, chunk-drop.
  - `components/Sidebar/index.tsx` (~:97-270): remove LLM model config fetch/save, `showModelSettings`, `window.openSettings` (if the Rust tray calls `window.openSettings`, make it `router.push('/settings')` instead of deleting; grep `openSettings` in `src-tauri`).
  - `app/settings/page.tsx`: remove Summary tab.
  - `app/layout.tsx`: remove `OllamaDownloadProvider`.
  - `contexts/ConfigContext*`: remove summary/LLM model config state if nothing else reads it.
  - Onboarding: `components/onboarding/steps/DownloadProgressStep.tsx` and `SetupOverviewStep.tsx` drop the summary model (`builtin_ai_*` calls); `contexts/OnboardingContext*` drop `summaryModelDownloaded` and treat completion as transcription model + permissions. Onboarding status write must still mark completed.
  - `app/globals.css`: remove BlockNote/ProseMirror rules (~:184-210).
- Where `ModelConfig` type is still imported by kept code, delete the usage; do not recreate the type.

- [ ] **Step 1:** `grep -rln "ModelSettingsModal\|ModelConfig\b\|AISummary\|BlockNote\|SummaryPanel\|useSummaryGeneration\|OllamaDownload\|summaryModel\|builtin_ai_\|api_get_summary\|api_save_model_config\|api_get_model_config" frontend/src` — this is the work list.
- [ ] **Step 2:** Meeting details + hooks.
- [ ] **Step 3:** Settings, home modals, Sidebar, ConfigContext, layout.
- [ ] **Step 4:** Onboarding summary step.
- [ ] **Step 5:** Delete files; re-run the Step 1 grep, expect no hits except `lib/summary-language*` and its users.
- [ ] **Step 6: Gate** (tsc, build).
- [ ] **Step 7: Commit** `refactor: remove summary UI and summary onboarding step`.

### Task 5: Remove analytics UI and call sites, remove update checker

**Files:**
- Delete: `components/AnalyticsProvider.tsx`, `AnalyticsConsentSwitch.tsx`, `AnalyticsDataModal.tsx`, `lib/analytics.ts`, `components/UpdateCheckProvider.tsx`, `UpdateDialog.tsx`, `UpdateNotification*`, `hooks/useUpdateCheck*`, `services/updateService.ts`
- Modify: every importer of `@/lib/analytics` (≈26 files: delete the `Analytics.*` lines and import), `components/PreferenceSettings.tsx` (consent switch ~:225), `app/layout.tsx` (providers)
- Rust: `lib.rs` remove `.plugin(tauri_plugin_updater...)` and `.plugin(tauri_plugin_process::init())` (~:473-474) and any tray `check-updates-from-tray` emit (grep); `Cargo.toml` drop `tauri-plugin-updater`, `tauri-plugin-process`; `tauri.conf.json` drop `updater:default`, `process:default` capabilities and the `plugins.updater` block (~:114). Also `frontend/src-tauri/capabilities/*.json` if they list them.
- npm: remove `@tauri-apps/plugin-updater`, `@tauri-apps/plugin-process` (`pnpm remove`).
- Leave Rust `analytics/` and its registered commands untouched.

- [ ] **Step 1:** `grep -rln "lib/analytics\|Analytics\.\|useUpdateCheck\|updateService\|plugin-updater\|plugin-process\|check-updates" frontend/src frontend/src-tauri/src frontend/src-tauri/*.json frontend/src-tauri/capabilities`.
- [ ] **Step 2:** Remove call sites and files. Do not leave empty `try {}` blocks or unused variables behind.
- [ ] **Step 3:** Rust/config removals, `pnpm remove`.
- [ ] **Step 4: Gate:** tsc, build, `cargo check`.
- [ ] **Step 5: Commit** `refactor: remove analytics UI and update checker`.

### Task 6: Drop unused npm packages

**Files:** `frontend/package.json`, `frontend/pnpm-lock.yaml`, `frontend/tailwind.config.js` (typography plugin), and `components/ui/command.tsx` / `ui/dropdown-menu.tsx` if orphaned.

- [ ] **Step 1:** For each candidate: `@remirror/*`, `@tiptap/*`, `@blocknote/*`, `react-markdown`, `remark-gfm`, `lodash`, `@types/lodash`, `@hookform/resolvers`, `react-hook-form`, `@heroicons/react`, `zod`, `cmdk`, `@radix-ui/react-dropdown-menu`, `@tailwindcss/typography` run `grep -rn "from '<pkg>\|require('<pkg>" frontend/src frontend/*.js frontend/*.mjs`. Keep any with hits.
- [ ] **Step 2:** Delete orphan `ui/command.tsx`, `ui/dropdown-menu.tsx` first if no importer, then `pnpm remove <unused...>`. Remove `prosemirror-*` entries from `pnpm.overrides` in `package.json` if present. Remove the typography plugin line from `tailwind.config.js` if removed.
- [ ] **Step 3: Gate** (tsc, build).
- [ ] **Step 4: Commit** `chore: drop unused npm packages`.

### Task 7: Theme tokens, dark mode, system font

**Files:** `frontend/src/app/globals.css`, `frontend/tailwind.config.js`, `frontend/src/app/layout.tsx`

**Interfaces:**
- Produces: Tailwind token classes used by Tasks 8–10: `bg-background`, `bg-card` (raised), `bg-muted`, `text-foreground`, `text-muted-foreground`, `border-border`, `bg-primary text-primary-foreground` (indigo), `text-primary`, `bg-accent` (hover surface), `bg-destructive`, `ring-ring`. Plus new: `bg-recording` / `text-recording` (red), `text-success`, `text-warning`.

- [ ] **Step 1:** `tailwind.config.js`: `darkMode: 'media'`; `fontFamily.sans` = the system stack from Global Constraints; add colors `recording: 'hsl(var(--recording))'`, `success: 'hsl(var(--success))'`, `warning: 'hsl(var(--warning))'`; `--radius` stays the source of `borderRadius`.
- [ ] **Step 2:** `globals.css`: replace the `:root` and `.dark` blocks with `:root` (light) and `@media (prefers-color-scheme: dark) { :root { ... } }` using these HSL values:

```css
:root {
  --background: 0 0% 100%;
  --foreground: 240 6% 10%;
  --card: 0 0% 98%;
  --card-foreground: 240 6% 10%;
  --popover: 0 0% 100%;
  --popover-foreground: 240 6% 10%;
  --primary: 243 75% 59%;
  --primary-foreground: 0 0% 100%;
  --secondary: 240 5% 96%;
  --secondary-foreground: 240 6% 10%;
  --muted: 240 5% 96%;
  --muted-foreground: 240 4% 46%;
  --accent: 240 5% 94%;
  --accent-foreground: 240 6% 10%;
  --destructive: 0 72% 51%;
  --destructive-foreground: 0 0% 100%;
  --border: 240 6% 90%;
  --input: 240 6% 90%;
  --ring: 243 75% 59%;
  --recording: 0 72% 51%;
  --success: 142 71% 36%;
  --warning: 32 95% 44%;
  --radius: 0.5rem;
  color-scheme: light;
}
@media (prefers-color-scheme: dark) {
  :root {
    --background: 240 5% 4%;
    --foreground: 240 5% 96%;
    --card: 240 6% 10%;
    --card-foreground: 240 5% 96%;
    --popover: 240 6% 10%;
    --popover-foreground: 240 5% 96%;
    --primary: 234 89% 74%;
    --primary-foreground: 240 6% 10%;
    --secondary: 240 5% 13%;
    --secondary-foreground: 240 5% 96%;
    --muted: 240 5% 13%;
    --muted-foreground: 240 5% 65%;
    --accent: 240 5% 15%;
    --accent-foreground: 240 5% 96%;
    --destructive: 0 84% 60%;
    --destructive-foreground: 0 0% 100%;
    --border: 240 4% 16%;
    --input: 240 4% 16%;
    --ring: 234 89% 74%;
    --recording: 0 84% 60%;
    --success: 142 69% 45%;
    --warning: 38 92% 50%;
    color-scheme: dark;
  }
}
```
  Keep chart vars only if something uses `chart-` (grep). `body` gets `@apply bg-background text-foreground antialiased;`.
- [ ] **Step 3:** `layout.tsx`: remove the Source Sans 3 `next/font` import and its className/CSS var; keep `<html lang="en">` without a theme class. Sonner `<Toaster>`: pass `theme="system"`.
- [ ] **Step 4: Gate** (tsc, build).
- [ ] **Step 5: Commit** `style: zinc/indigo tokens, system font, dark mode via prefers-color-scheme`.

### Task 8: Color pass A — shell and recorder flow

**Files:** `app/` (all .tsx), `components/Sidebar/*`, `components/MeetingDetails/*`, `components/RecordingControls.tsx`, `components/ProjectSelect.tsx`, `components/EditableTitle.tsx`, `components/TranscriptView.tsx`, `components/VirtualizedTranscriptView.tsx`, `components/RecordingSettings.tsx`, `components/PreferenceSettings.tsx`, `components/TranscriptSettings.tsx`, `components/WikiAutomationSettings.tsx`, `components/DeviceSelection.tsx`, `components/LanguagePickerPopover.tsx`, `components/TranscriptRecovery/*`, `components/PermissionWarning.tsx`, `components/ChunkProgressDisplay.tsx`, `components/AudioLevelMeter.tsx`, `components/AudioBackendSelector.tsx`, `components/LanguageSelection.tsx`, `components/shared/*`.

Mapping (apply mechanically, then eyeball):

| From | To |
|---|---|
| `bg-white`, `bg-gray-50` (page) | `bg-background` |
| `bg-gray-50`/`bg-gray-100` (panels, sidebar, cards) | `bg-card` or `bg-muted` |
| `hover:bg-gray-100`/`hover:bg-gray-200`, `bg-gray-200` (selected) | `hover:bg-accent` / `bg-accent` |
| `text-gray-900`/`800`/`700`, `text-black` | `text-foreground` |
| `text-gray-600`/`500`/`400` | `text-muted-foreground` |
| `border-gray-100`/`200`/`300` | `border-border` |
| `bg-blue-600`/`500` + `text-white` | `bg-primary text-primary-foreground` |
| `hover:bg-blue-700` | `hover:bg-primary/90` |
| `text-blue-600`/`500`/`700` | `text-primary` |
| `bg-blue-50`/`100` | `bg-primary/10` |
| `border-blue-*` | `border-primary` (or `/40`) |
| `ring-blue-*`, `focus:ring-blue-*` | `ring-ring` |
| `bg-red-500/600` on record/stop | `bg-recording` |
| `bg-red-*` destructive buttons, `text-red-*` errors | `bg-destructive` / `text-destructive` |
| `bg-red-50`, `bg-yellow-50`, `bg-green-50` alerts | `bg-destructive/10`, `bg-warning/10`, `bg-success/10` |
| `text-green-*`, `text-yellow-*`/`amber-*` | `text-success`, `text-warning` |
| `shadow-*` on cards | remove, rely on `border border-border` |
| `rounded-xl`/`2xl` | `rounded-lg` |

Inline `style={{ color: '#...' }}` / hex in TSX: convert to token classes the same way.

- [ ] **Step 1:** `grep -rnoE '(bg|text|border|ring|from|to|divide|placeholder)-(white|black|gray|slate|zinc|neutral|blue|indigo|red|green|yellow|amber)(-[0-9]+)?|#[0-9a-fA-F]{6}' <files>` and record the count.
- [ ] **Step 2:** Apply the mapping file by file.
- [ ] **Step 3:** Re-run the grep. Expected: only intentional leftovers (e.g. `text-white` on a colored badge). List them in the commit body.
- [ ] **Step 4: Gate** (tsc, build).
- [ ] **Step 5: Commit** `style: tokenize colors in shell and recorder flow`.

### Task 9: Color pass B — everything else

**Files:** all remaining `frontend/src/components/**` (onboarding, `WhisperModelManager`, `ParakeetModelManager`, `ModelDownloadProgress`, `components/ui/*` shadcn primitives, any other), plus `frontend/src/app/globals.css` leftovers.

- [ ] **Step 1:** Run the Task 8 grep over `frontend/src` (all files). This is the work list.
- [ ] **Step 2:** Apply the same mapping table (copy from Task 8). For `components/ui/*` keep shadcn semantics; most already use tokens, only fix literals.
- [ ] **Step 3:** Re-run grep over `frontend/src`. Expected: only intentional leftovers, listed in commit body.
- [ ] **Step 4: Gate** (tsc, build).
- [ ] **Step 5: Commit** `style: tokenize remaining components`.

### Task 10: Layout polish to match the mockup

Reference: the mockup (Recording / Meeting / Settings screens).

**Files:** `components/Sidebar/index.tsx`, `app/page.tsx`, `app/_components/TranscriptPanel.tsx`, `components/RecordingControls.tsx`, `components/MeetingDetails/TranscriptPanel.tsx`, `TranscriptButtonGroup.tsx`, `MeetingDetailsSplitView.tsx`, `app/settings/page.tsx`, `components/RecordingSettings.tsx`, `components/WikiAutomationSettings.tsx`, `components/PreferenceSettings.tsx`.

- [ ] **Step 1: Sidebar.** `bg-card border-r border-border`, width ~220px expanded. Order: search, "Projects" label (`text-[11px] uppercase tracking-wide text-muted-foreground`), project tree (rows `px-2 py-1 rounded-md text-sm`, selected `bg-accent`), spacer, "New recording" button (`bg-foreground text-background`, red dot before label), Settings row. Remove the separate Home entry if the logo or "New recording" already routes to `/`; keep the collapsed rail with the same items as icons.
- [ ] **Step 2: Home.** Top bar `px-4 py-2.5 border-b border-border`: meeting title (`text-sm font-semibold`) left, `ProjectSelect` pill right. Transcript body `px-6 py-5`, rows as grid `[56px_1fr]`: mono tabular timestamp `text-xs text-muted-foreground`, speaker label `text-[11px] font-semibold uppercase` (`Me` = `text-primary`, `Them` = `text-muted-foreground`), text below. Max line width `max-w-[72ch]`. Bottom dock `border-t border-border py-3.5`, centered: red dot + mono timer, round 44px stop button (border, red square inside), Pause as outline button. Idle state: a single "Start recording" button in the dock. Move the Copy and Language buttons of `app/_components/TranscriptPanel.tsx` into the top bar as small ghost buttons.
- [ ] **Step 3: Meeting details.** Single top bar: title + `· date · duration` muted, then Copy, Open folder, Project select (outline buttons `px-2.5 py-1 text-[13px]`), Enhance as the only `bg-primary` button. Same transcript row style as home.
- [ ] **Step 4: Settings.** Header `text-lg font-semibold`, back button as ghost icon. Tabs as underline tabs (`border-b-2 border-foreground` active). Each section is a `rounded-lg border border-border` group with `divide-y divide-border` rows: label `font-medium` + `text-muted-foreground text-xs` hint on the left, control on the right. Wiki automation uses the same group style under Recordings. Remove the animated underline `motion` code if it no longer fits.
- [ ] **Step 5: Density pass.** Headings no larger than `text-lg`, body `text-sm`/`text-[13px]`, buttons `h-8`, radius `rounded-md`/`rounded-lg` only.
- [ ] **Step 6: Gate** (tsc, build).
- [ ] **Step 7: Commit** `style: lean layout for sidebar, home, meeting details and settings`.

### Task 11: Final verification

- [ ] **Step 1:** In `frontend/`: `pnpm exec tsc --noEmit` (≤ baseline), `pnpm build`, `pnpm lint` (report, do not mass-fix pre-existing warnings); in `frontend/src-tauri`: `cargo check`.
- [ ] **Step 2:** `grep -rn "About\b\|ImportAudio\|AISummary\|BlockNote\|Analytics\|UpdateCheck\|v0\.4\.1\|notes/\|barHeights" frontend/src` — expect no hits (except unrelated words; list any).
- [ ] **Step 3:** Run `pnpm run dev` in background, open http://localhost:3118 with Playwright, screenshot `/`, `/settings` in light and dark (`browser_emulate_media` colorScheme). Tauri `invoke` fails in a plain browser; note blank areas caused by that rather than fixing them. Save screenshots to the scratchpad and report paths.
- [ ] **Step 4:** Report to the controller: what was verified, what needs manual smoke in the real app (record, stop, open meeting, Enhance, move project, onboarding on a fresh profile).
