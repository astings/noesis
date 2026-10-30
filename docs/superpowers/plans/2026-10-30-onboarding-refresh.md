# Onboarding refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the upstream 4-screen onboarding with the 6-screen flow in `docs/superpowers/specs/2026-10-30-onboarding-refresh-design.md`, and stop `complete_onboarding` from overwriting settings.

**Architecture:** The onboarding context (`src/contexts/OnboardingContext.tsx`) owns the step number, platform and a `savePreferences(patch)` helper. Each screen is one file in `src/components/onboarding/steps/`, renders inside `OnboardingContainer`, and calls `savePreferences` on Continue. The Rust `complete_onboarding` keeps its signature and only writes the transcription config when none exists.

**Tech Stack:** Tauri 2 (Rust), Next.js 14, React 18, TypeScript, shadcn/ui (`Button`, `Switch`, `Input`), lucide-react, sonner toasts.

## Global Constraints

- Work on branch `enhance/onboarding-refresh` in `/Users/lainealexandre/Code/Perso/noesis`. Use `/usr/bin/git`.
- Frontend checks: `cd frontend && pnpm exec tsc --noEmit`. `pnpm lint` does not work (no ESLint config, prompts). `pnpm test` runs the Node tests.
- Rust checks: `cd frontend/src-tauri && cargo test --lib <one_filter>` (one filter per invocation), `cargo check`.
- Steps are numbered 1 to 6 on every platform: 1 Welcome, 2 You, 3 Transcription, 4 Permissions, 5 Calls, 6 Claude Code. Step 4 renders only on macOS; elsewhere `goNext` from 3 goes to 5 and `goPrevious` from 5 goes to 3.
- Settings are saved only through `get_recording_preferences` then `set_recording_preferences` with the merged struct.
- Copy is exact as in the spec. Consent note: "Get consent from everyone on the call. Recording rules differ by country."
- Enhance model choices: Whisper `small` (~470 MB), `medium` (~1.5 GB), `large-v3` (~3 GB, default).
- Do not touch summary/LLM settings, the legacy database import in the context, or `migration.rs`.
- Match surrounding code style: no new dependencies, comments sparse.

---

### Task 1: Rust `complete_onboarding` stops overwriting settings

**Files:**
- Modify: `frontend/src-tauri/src/onboarding.rs` (function `complete_onboarding`, tests module)

**Interfaces:**
- Produces: `fn should_save_default_transcript(existing: Option<&str>) -> bool` (private, tested). `complete_onboarding(app, state, model: String)` keeps its signature; `model` is ignored.

- [ ] **Step 1: Write the failing test** in the `tests` module of `onboarding.rs`:

```rust
    #[test]
    fn default_transcript_saved_only_when_none_exists() {
        assert!(should_save_default_transcript(None));
        assert!(!should_save_default_transcript(Some("whisper")));
    }
```

- [ ] **Step 2: Run it, expect a compile failure** (`should_save_default_transcript` not found):
`cd frontend/src-tauri && cargo test --lib default_transcript_saved_only_when_none_exists`

- [ ] **Step 3: Implement.** Add above `complete_onboarding`:

```rust
/// Onboarding installs Parakeet as the live model only on a fresh setup; a re-run keeps the user's choice.
fn should_save_default_transcript(existing_provider: Option<&str>) -> bool {
    existing_provider.is_none()
}
```

Replace the body of `complete_onboarding` from `// Step 1:` up to (not including) `// Step 2:` with:

```rust
    // The summary model is not touched: titles/tags and the Ask fallback read it.
    let _ = model;
    let pool = state.db_manager.pool();

    let existing = SettingsRepository::get_transcript_config(pool)
        .await
        .map_err(|e| format!("Failed to read transcription config: {}", e))?;
    if should_save_default_transcript(existing.as_ref().map(|s| s.provider.as_str())) {
        SettingsRepository::save_transcript_config(
            pool,
            "parakeet",
            crate::config::DEFAULT_PARAKEET_MODEL,
        )
        .await
        .map_err(|e| format!("Failed to save transcription model config: {}", e))?;
        info!("Saved transcription model config: provider=parakeet, model={}", crate::config::DEFAULT_PARAKEET_MODEL);
    } else {
        info!("Transcription config already set, keeping it");
    }
```

Check the field name on `TranscriptSetting` (`grep -n "pub struct TranscriptSetting" -A6 src/database/models.rs` or wherever `get_transcript_config` returns it) and use the provider field's actual name. In the status update below, drop `status.model_status.summary = "downloaded"` and `selected_summary_model = Some(model.clone())`; leave the fields as they are. Change the log line `info!("Onboarding completed successfully with model: {}", model);` to `info!("Onboarding completed");` and the opening log to `info!("Completing onboarding");`. Remove the `error` import if it becomes unused.

- [ ] **Step 4: Run the test, expect PASS**, then `cargo check` with no new warnings in `onboarding.rs`.

- [ ] **Step 5: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/onboarding.rs
/usr/bin/git commit -m "fix(onboarding): finishing setup no longer overwrites the summary or live model"
```

---

### Task 2: Context: 6 steps, platform, `savePreferences`; container progress mapping

**Files:**
- Modify: `frontend/src/contexts/OnboardingContext.tsx`
- Modify: `frontend/src/components/onboarding/OnboardingContainer.tsx`
- Modify: `frontend/src/components/onboarding/shared/ProgressIndicator.tsx` (icons only)

**Interfaces:**
- Produces on `useOnboarding()`:
  - `isMac: boolean`
  - `savePreferences: (patch: Partial<RecordingPreferences>) => Promise<void>` (throws on failure)
  - `goNext`, `goPrevious`, `goToStep(step: number)` with the step rules in Global Constraints
  - existing members unchanged (`parakeetDownloaded`, `startBackgroundDownloads`, `retryParakeetDownload`, `completeOnboarding`, permissions…)
- `OnboardingContainer` takes the real step number (1..6) in `step`; it computes the shown number and total itself; its `totalSteps` prop is ignored.
- `RecordingPreferences` type is imported from `@/components/RecordingSettings` (`import type`).

- [ ] **Step 1: Edit the context.**
  - Add `import type { RecordingPreferences } from '@/components/RecordingSettings';` and `const LAST_STEP = 6;`.
  - Add to `OnboardingContextType`: `isMac: boolean;` and `savePreferences: (patch: Partial<RecordingPreferences>) => Promise<void>;`.
  - In the provider:

```tsx
  const [isMac, setIsMac] = useState(false);

  useEffect(() => {
    import('@tauri-apps/plugin-os')
      .then(({ platform }) => setIsMac(platform() === 'macos'))
      .catch(() => setIsMac(navigator.userAgent.includes('Mac')));
  }, []);

  const savePreferences = useCallback(async (patch: Partial<RecordingPreferences>) => {
    const current = await invoke<RecordingPreferences>('get_recording_preferences');
    await invoke('set_recording_preferences', { preferences: { ...current, ...patch } });
  }, []);
```

  - Replace `goToStep`, `goNext`, `goPrevious`:

```tsx
  const goToStep = useCallback((step: number) => {
    setCurrentStep(Math.max(1, Math.min(step, LAST_STEP)));
  }, []);

  // Step 4 (Permissions) exists only on macOS.
  const goNext = useCallback(() => {
    setCurrentStep((prev: number) => {
      const next = prev + 1 === 4 && !isMac ? 5 : prev + 1;
      return Math.min(next, LAST_STEP);
    });
  }, [isMac]);

  const goPrevious = useCallback(() => {
    setCurrentStep((prev: number) => {
      const previous = prev - 1 === 4 && !isMac ? 3 : prev - 1;
      return Math.max(previous, 1);
    });
  }, [isMac]);
```

  - In `verifyModelStatus`, replace the clamp block with `if (currentStep > LAST_STEP) currentStep = LAST_STEP;` and update its comment to the 6-step flow.
  - In `saveOnboardingStatus`, `version: '2.0'`.
  - In `completeOnboarding`, keep `invoke('complete_onboarding', { model: '' })` and change the comment to `// Rust ignores the model argument.`
  - Add `isMac` and `savePreferences` to the provider value.

- [ ] **Step 2: Edit `OnboardingContainer`.** Read `isMac` from `useOnboarding()`. Compute:

```tsx
  // Real steps are 1..6; off macOS step 4 does not exist, so later steps show one lower.
  const total = isMac ? 6 : 5;
  const shown = step && !isMac && step > 4 ? step - 1 : step;
  const toReal = (s: number) => (!isMac && s >= 4 ? s + 1 : s);
```

  Use `shown` and `total` wherever `step` and `totalSteps` are used for display and for the navigation button `disabled` checks (`shown === 1`, `shown === total`). `handleStepClick` becomes `(s: number) => goToStep(toReal(s) + stepOffset)`. Remove `totalSteps = 5` from the destructured props (leave the prop in the type so callers still compile).

- [ ] **Step 3: Edit `ProgressIndicator` icons:**

```tsx
import { Check, Lock, Download, CheckCircle2, FolderOpen, Mic, Phone, Terminal } from 'lucide-react';

const stepIcons = [Lock, FolderOpen, Download, Mic, Phone, Terminal];
```

  Leave the rest of the file. (Off macOS the icons after 3 are off by one; accepted.)

- [ ] **Step 4: Type check.** `cd frontend && pnpm exec tsc --noEmit`. Expected: passes (existing steps still pass `totalSteps`, which is now ignored).

- [ ] **Step 5: Commit**

```bash
/usr/bin/git add frontend/src/contexts/OnboardingContext.tsx frontend/src/components/onboarding/OnboardingContainer.tsx frontend/src/components/onboarding/shared/ProgressIndicator.tsx
/usr/bin/git commit -m "feat(onboarding): six-step navigation, platform flag and savePreferences in the context"
```

---

### Task 3: Welcome and "You and your files" screens, flow wiring

**Files:**
- Modify: `frontend/src/components/onboarding/steps/WelcomeStep.tsx`
- Create: `frontend/src/components/onboarding/steps/YouStep.tsx`
- Delete: `frontend/src/components/onboarding/steps/SetupOverviewStep.tsx`
- Modify: `frontend/src/components/onboarding/steps/index.ts`
- Modify: `frontend/src/components/onboarding/OnboardingFlow.tsx`

**Interfaces:**
- Consumes: `useOnboarding()` → `goNext`, `goPrevious`, `savePreferences`, `startBackgroundDownloads({ includeParakeet: true })`, `isMac`.
- Produces: `export function YouStep()`. `OnboardingFlow` renders by `currentStep`: 1 `WelcomeStep`, 2 `YouStep`, 3 `DownloadProgressStep`, 4 `PermissionsStep` (only when `isMac`), 5 `CallsStep`, 6 `ClaudeCodeStep`. Tasks 5 adds the last two; until then 5 and 6 render nothing.

- [ ] **Step 1: WelcomeStep.** Change the description to `"Records your calls and writes transcripts into project folders that Claude can read."`. Keep the two feature lines. The Get Started handler:

```tsx
  const { goNext, startBackgroundDownloads } = useOnboarding();

  const handleStart = () => {
    startBackgroundDownloads({ includeParakeet: true }).catch((e) =>
      console.error('[WelcomeStep] Failed to start Parakeet download:', e)
    );
    goNext();
  };
```

  Replace `<p className="text-xs ...">Takes less than 3 minutes</p>` with that line plus, below it:

```tsx
          <div className="text-center">
            <a
              href="https://github.com/astings/noesis/issues"
              target="_blank"
              rel="noopener noreferrer"
              className="text-xs text-muted-foreground hover:underline"
            >
              Report issues on GitHub
            </a>
          </div>
```

- [ ] **Step 2: Create `YouStep.tsx`:**

```tsx
import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { LANGUAGES } from '@/lib/languages';
import type { RecordingPreferences } from '@/components/RecordingSettings';

export function YouStep() {
  const { goNext, savePreferences } = useOnboarding();
  const [folder, setFolder] = useState('');
  const [name, setName] = useState('');
  const [languages, setLanguages] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    invoke<RecordingPreferences>('get_recording_preferences')
      .then((p) => {
        setFolder(p.save_folder);
        setName(p.owner_name);
        setLanguages(p.transcription_languages.length ? p.transcription_languages : ['en']);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const changeFolder = async () => {
    const picked = await invoke<string | null>('select_directory').catch(() => null);
    if (picked) setFolder(picked);
  };

  const toggleLanguage = (code: string) =>
    setLanguages((prev) =>
      prev.includes(code) ? (prev.length > 1 ? prev.filter((c) => c !== code) : prev) : [...prev, code]
    );

  const handleContinue = async () => {
    setSaving(true);
    setError('');
    try {
      await savePreferences({
        save_folder: folder,
        owner_name: name.trim() || 'Me',
        transcription_languages: languages,
      });
      goNext();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <OnboardingContainer
      title="You and your files"
      description="Where meetings are saved and how they are labeled."
      step={2}
      showNavigation
      canGoNext={false}
    >
      <div className="max-w-md mx-auto space-y-6">
        <div className="space-y-2">
          <p className="text-sm font-medium text-foreground">Recordings folder</p>
          <div className="flex items-center gap-2">
            <p className="flex-1 font-mono text-xs break-all text-muted-foreground">{folder || 'Default folder'}</p>
            <Button variant="outline" size="sm" onClick={changeFolder} disabled={saving}>Change…</Button>
          </div>
          <p className="text-xs text-muted-foreground">Each project gets a subfolder with one folder per meeting.</p>
        </div>

        <div className="space-y-2">
          <p className="text-sm font-medium text-foreground">Your name</p>
          <Input value={name} onChange={(e) => setName(e.target.value)} aria-label="Your name" disabled={saving} />
          <p className="text-xs text-muted-foreground">Used for your side of the transcript.</p>
        </div>

        <div className="space-y-2">
          <p className="text-sm font-medium text-foreground">Meeting languages</p>
          <div className="flex flex-wrap gap-1.5">
            {LANGUAGES.map((l) => {
              const on = languages.includes(l.code);
              return (
                <button key={l.code} type="button" aria-pressed={on} disabled={saving}
                  className={`rounded border px-2 py-0.5 text-xs ${on ? 'border-foreground bg-foreground text-background' : 'border-border text-muted-foreground hover:text-foreground'}`}
                  onClick={() => toggleLanguage(l.code)}>
                  {l.label}
                </button>
              );
            })}
          </div>
        </div>

        <div className="space-y-2 pt-2">
          <Button onClick={handleContinue} disabled={saving || !folder} className="w-full h-11">Continue</Button>
          {error && <p className="text-xs text-center text-destructive">{error}</p>}
        </div>
      </div>
    </OnboardingContainer>
  );
}
```

  Check `select_directory`'s return type in Rust (`grep -rn "fn select_directory" frontend/src-tauri/src`) and adapt the `invoke` generic if it returns `Result<String, String>` with an empty string for cancel.

- [ ] **Step 3: Delete `SetupOverviewStep.tsx`; update `steps/index.ts`** to export `WelcomeStep`, `YouStep`, `DownloadProgressStep`, `PermissionsStep`.

- [ ] **Step 4: Rewrite `OnboardingFlow.tsx`:**

```tsx
import React from 'react';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { WelcomeStep, YouStep, DownloadProgressStep, PermissionsStep } from './steps';

// 1 Welcome, 2 You, 3 Transcription, 4 Permissions (macOS only), 5 Calls, 6 Claude Code.
export function OnboardingFlow() {
  const { currentStep, isMac } = useOnboarding();

  return (
    <div className="onboarding-flow">
      {currentStep === 1 && <WelcomeStep />}
      {currentStep === 2 && <YouStep />}
      {currentStep === 3 && <DownloadProgressStep />}
      {currentStep === 4 && isMac && <PermissionsStep />}
    </div>
  );
}
```

- [ ] **Step 5: Type check** `cd frontend && pnpm exec tsc --noEmit`. Expected: passes. Fix any remaining import of `SetupOverviewStep` (`grep -rn SetupOverviewStep frontend/src`).

- [ ] **Step 6: Commit**

```bash
/usr/bin/git add -A frontend/src/components/onboarding
/usr/bin/git commit -m "feat(onboarding): welcome copy, folder/name/languages screen, drop setup overview"
```

---

### Task 4: Transcription screen (live Parakeet + enhance model choice)

**Files:**
- Modify: `frontend/src/components/onboarding/steps/DownloadProgressStep.tsx`

**Interfaces:**
- Consumes: `useOnboarding()` → `goNext`, `parakeetDownloaded`, `setParakeetDownloaded`, `startBackgroundDownloads`, `savePreferences`; commands `get_recording_preferences`, `whisper_init`, `whisper_get_available_models` (returns `{ name: string; status: unknown }[]`; a downloaded model has `status === 'Available'`), `whisper_download_model({ modelName })`.
- Produces: the step-3 screen. Continue does not wait for Parakeet any more; Task 5's Finish does.

- [ ] **Step 1: Remove the non-macOS completion branch.** `handleContinue` keeps the Parakeet verification block, then saves enhance settings, starts the Whisper download, and calls `goNext()` on every platform. Remove `completeOnboarding`, `isCompleting`, the local `isMac` state and its effect. Keep the Parakeet `useEffect` that calls `startBackgroundDownloads` (it is a no-op when Welcome already started it, because `parakeetDownloadStartedRef` guards the component and Rust ignores a duplicate start — verify with `grep -n "already" frontend/src-tauri/src/parakeet_engine/commands.rs`; if a second call would restart the download, remove that effect since Welcome now starts it).

- [ ] **Step 2: Add enhance state and choices** at the top of the component:

```tsx
const ENHANCE_MODELS = [
  { name: 'small', label: 'Small', size: '~470 MB', hint: 'Fastest' },
  { name: 'medium', label: 'Medium', size: '~1.5 GB', hint: 'Balanced' },
  { name: 'large-v3', label: 'Large', size: '~3 GB', hint: 'Most accurate' },
] as const;
```

(outside the component), and inside:

```tsx
  const { savePreferences } = useOnboarding();
  const [autoEnhance, setAutoEnhance] = useState(true);
  const [enhanceModel, setEnhanceModel] = useState<string>('large-v3');
  const [saveError, setSaveError] = useState('');

  useEffect(() => {
    invoke<RecordingPreferences>('get_recording_preferences')
      .then((p) => {
        setAutoEnhance(p.auto_enhance);
        if (ENHANCE_MODELS.some((m) => m.name === p.enhance_model)) setEnhanceModel(p.enhance_model);
      })
      .catch(() => {});
  }, []);

  const startEnhanceDownload = async (modelName: string) => {
    await invoke('whisper_init');
    const models = await invoke<{ name: string; status: unknown }[]>('whisper_get_available_models');
    const have = models.some((m) => m.name === modelName && m.status === 'Available');
    if (!have) {
      invoke('whisper_download_model', { modelName }).catch((e) =>
        console.error('[TranscriptionStep] Enhance model download failed:', e)
      );
    }
  };
```

Add `import type { RecordingPreferences } from '@/components/RecordingSettings';` and `import { Switch } from '@/components/ui/switch';`.

- [ ] **Step 3: New `handleContinue` tail** (after the Parakeet verification block, replacing the toast and the `if (isMac)` branch):

```tsx
    setSaveError('');
    try {
      await savePreferences(
        autoEnhance
          ? { auto_enhance: true, enhance_provider: 'whisper', enhance_model: enhanceModel }
          : { auto_enhance: false }
      );
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e));
      return;
    }
    if (autoEnhance) {
      startEnhanceDownload(enhanceModel).catch((e) => console.error('[TranscriptionStep] ', e));
    }
    goNext();
```

  Keep the early `return` with the "Transcription engine required" toast when Parakeet failed.

- [ ] **Step 4: Render.** Container: `title="Transcription"`, `description="A live transcript while you talk, and a more accurate one after the call."`, `step={3}`, `showNavigation`, `canGoNext={false}`. Body, in order:
  1. `renderDownloadCard('Transcription Engine', <Mic …/>, parakeetState, '~670 MB')` with a line above it: `<p className="text-sm font-medium">Live transcript</p>`.
  2. An enhance block:

```tsx
          <div className="bg-background rounded-lg border border-border p-5 space-y-4">
            <div className="flex items-center justify-between gap-4">
              <div>
                <h3 className="font-medium text-foreground">Enhance after each call</h3>
                <p className="text-sm text-muted-foreground">Re-transcribes the full recording with Whisper.</p>
              </div>
              <Switch checked={autoEnhance} onCheckedChange={setAutoEnhance} aria-label="Enhance after each call" />
            </div>
            {autoEnhance && (
              <div className="grid grid-cols-3 gap-2" role="radiogroup" aria-label="Enhance model">
                {ENHANCE_MODELS.map((m) => {
                  const on = enhanceModel === m.name;
                  return (
                    <button key={m.name} type="button" role="radio" aria-checked={on}
                      onClick={() => setEnhanceModel(m.name)}
                      className={`rounded-md border p-3 text-left ${on ? 'border-foreground' : 'border-border hover:border-foreground/50'}`}>
                      <p className="text-sm font-medium text-foreground">{m.label}</p>
                      <p className="text-xs text-muted-foreground">{m.size} · {m.hint}</p>
                    </button>
                  );
                })}
              </div>
            )}
            {autoEnhance && <p className="text-xs text-muted-foreground">It keeps downloading after setup.</p>}
          </div>
```

  3. The Continue button: no `disabled` prop, label `Continue`, no spinner. Below it, when `!parakeetDownloaded`, `<p className="text-xs text-center text-muted-foreground">The live engine keeps downloading. You can continue.</p>`, and `{saveError && <p className="text-xs text-center text-destructive">{saveError}</p>}`.

- [ ] **Step 5: Type check** `cd frontend && pnpm exec tsc --noEmit`. Expected: passes.

- [ ] **Step 6: Commit**

```bash
/usr/bin/git add frontend/src/components/onboarding/steps/DownloadProgressStep.tsx
/usr/bin/git commit -m "feat(onboarding): transcription screen with enhance on/off and model choice"
```

---

### Task 5: Permissions, Calls and Claude Code screens; Finish

**Files:**
- Modify: `frontend/src/components/onboarding/steps/PermissionsStep.tsx`
- Create: `frontend/src/components/onboarding/steps/CallsStep.tsx`
- Create: `frontend/src/components/onboarding/steps/ClaudeCodeStep.tsx`
- Modify: `frontend/src/components/onboarding/steps/index.ts`, `frontend/src/components/onboarding/OnboardingFlow.tsx`

**Interfaces:**
- Consumes: `useOnboarding()` → `goNext`, `savePreferences`, `completeOnboarding`, `parakeetDownloaded`, `parakeetProgress`, `setPermissionsSkipped`; `McpServerSettings` from `@/components/McpServerSettings`; command `calendar_request_access` (returns `boolean`).
- Produces: `CallsStep`, `ClaudeCodeStep`. `OnboardingFlow` renders them at steps 5 and 6.

- [ ] **Step 1: PermissionsStep.** Remove `handleFinish` and `completeOnboarding` from it. Replace the action buttons block with:

```tsx
        <div className="flex flex-col gap-3 pt-4">
          <Button onClick={goNext} disabled={!allPermissionsGranted} className="w-full h-11">
            Continue
          </Button>
          {!allPermissionsGranted && (
            <button
              onClick={() => { setPermissionsSkipped(true); goNext(); }}
              className="text-sm text-muted-foreground hover:text-foreground transition-colors"
            >
              Skip for now
            </button>
          )}
          {!allPermissionsGranted && (
            <p className="text-xs text-center text-muted-foreground">
              Recording won't work without permissions. You can grant them later in settings.
            </p>
          )}
        </div>
```

  Get `goNext` from `useOnboarding()`. Container: `step={4}`, remove `hideProgress`, `showNavigation`, `canGoNext={false}`.

- [ ] **Step 2: Create `CallsStep.tsx`:**

```tsx
import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import type { RecordingPreferences } from '@/components/RecordingSettings';

type CallKey = 'auto_start_on_call' | 'auto_stop_on_call_end' | 'show_call_overlay' | 'show_tray_icon' | 'calendar_meeting_name';

const ROWS: { key: CallKey; label: string; hint?: string }[] = [
  { key: 'auto_start_on_call', label: 'Start recording when a call begins', hint: 'Get consent from everyone on the call. Recording rules differ by country.' },
  { key: 'auto_stop_on_call_end', label: 'Stop recording when the call ends' },
  { key: 'show_call_overlay', label: 'Floating call overlay', hint: 'Timer, pause, stop and project during a recording.' },
  { key: 'show_tray_icon', label: 'Menu bar icon' },
  { key: 'calendar_meeting_name', label: 'Name meetings from your calendar', hint: 'Asks for Calendar access.' },
];

export function CallsStep() {
  const { goNext, savePreferences } = useOnboarding();
  const [values, setValues] = useState<Record<CallKey, boolean>>({
    auto_start_on_call: false,
    auto_stop_on_call_end: true,
    show_call_overlay: true,
    show_tray_icon: true,
    calendar_meeting_name: false,
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    invoke<RecordingPreferences>('get_recording_preferences')
      .then((p) => setValues({
        auto_start_on_call: p.auto_start_on_call,
        auto_stop_on_call_end: p.auto_stop_on_call_end,
        show_call_overlay: p.show_call_overlay,
        show_tray_icon: p.show_tray_icon,
        calendar_meeting_name: p.calendar_meeting_name,
      }))
      .catch(() => {});
  }, []);

  const toggle = async (key: CallKey, on: boolean) => {
    setError('');
    if (key === 'calendar_meeting_name' && on) {
      const granted = await invoke<boolean>('calendar_request_access').catch(() => false);
      if (!granted) {
        setError('Calendar access was refused. Allow Noesis in System Settings > Privacy & Security > Calendars.');
        return;
      }
    }
    setValues((v) => ({ ...v, [key]: on }));
  };

  const handleContinue = async () => {
    setSaving(true);
    setError('');
    try {
      await savePreferences(values);
      goNext();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <OnboardingContainer title="Calls" description="How Noesis behaves when a call starts." step={5} showNavigation canGoNext={false}>
      <div className="max-w-md mx-auto space-y-6">
        <div className="bg-background rounded-lg border border-border divide-y divide-border">
          {ROWS.map((r) => (
            <div key={r.key} className="flex items-center justify-between gap-4 p-4">
              <div>
                <p className="text-sm font-medium text-foreground">{r.label}</p>
                {r.hint && <p className="text-xs text-muted-foreground">{r.hint}</p>}
              </div>
              <Switch checked={values[r.key]} onCheckedChange={(on) => toggle(r.key, on)} aria-label={r.label} disabled={saving} />
            </div>
          ))}
        </div>
        <div className="space-y-2">
          <Button onClick={handleContinue} disabled={saving} className="w-full h-11">Continue</Button>
          {error && <p className="text-xs text-center text-destructive">{error}</p>}
        </div>
      </div>
    </OnboardingContainer>
  );
}
```

- [ ] **Step 3: Create `ClaudeCodeStep.tsx`:**

```tsx
import React, { useState } from 'react';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { McpServerSettings } from '@/components/McpServerSettings';

export function ClaudeCodeStep() {
  const { completeOnboarding, parakeetDownloaded, parakeetProgress } = useOnboarding();
  const [finishing, setFinishing] = useState(false);
  const [error, setError] = useState('');

  const handleFinish = async () => {
    setFinishing(true);
    setError('');
    try {
      await completeOnboarding();
      window.location.reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setFinishing(false);
    }
  };

  return (
    <OnboardingContainer
      title="Claude Code"
      description="Optional: let Claude Code read your meetings."
      step={6}
      showNavigation
      canGoNext={false}
    >
      <div className="max-w-md mx-auto space-y-6">
        <McpServerSettings />
        <p className="text-xs text-muted-foreground">
          No Claude Code? Install it from https://claude.com/claude-code, then use Settings &gt; Recordings &gt; Claude Code.
        </p>
        <div className="space-y-2">
          <Button onClick={handleFinish} disabled={!parakeetDownloaded || finishing} className="w-full h-11">
            {finishing ? <Loader2 className="w-4 h-4 animate-spin" /> : 'Finish'}
          </Button>
          {!parakeetDownloaded && (
            <p className="text-xs text-center text-muted-foreground">
              Downloading the live transcription engine… {Math.round(parakeetProgress)}%
            </p>
          )}
          {error && <p className="text-xs text-center text-destructive">{error}</p>}
        </div>
      </div>
    </OnboardingContainer>
  );
}
```

- [ ] **Step 4: Wire.** `steps/index.ts` adds `export { CallsStep } from './CallsStep';` and `export { ClaudeCodeStep } from './ClaudeCodeStep';`. In `OnboardingFlow.tsx` import both and add:

```tsx
      {currentStep === 5 && <CallsStep />}
      {currentStep === 6 && <ClaudeCodeStep />}
```

- [ ] **Step 5: Type check** `cd frontend && pnpm exec tsc --noEmit`. Expected: passes. `grep -rn "completeOnboarding" frontend/src/components/onboarding` shows only `ClaudeCodeStep`.

- [ ] **Step 6: Commit**

```bash
/usr/bin/git add -A frontend/src/components/onboarding
/usr/bin/git commit -m "feat(onboarding): calls and Claude Code screens, finish after the live engine is ready"
```

---

### Task 6: "Run setup again" in Settings, docs

**Files:**
- Modify: `frontend/src/components/RecordingSettings.tsx` (after `<McpServerSettings disabled={saving} />`)
- Modify: `CLAUDE.md` (fork section, one bullet)

**Interfaces:**
- Consumes: command `reset_onboarding_status_cmd`.

- [ ] **Step 1: Add the group** right after `<McpServerSettings disabled={saving} />`:

```tsx
      <SettingsGroup title="Setup">
        <SettingRow
          label="Run setup again"
          hint="Walks through folder, transcription, permissions, calls and Claude Code. Your recordings stay."
          control={
            <Button variant="outline" size="sm" disabled={saving || isRecording} onClick={async () => {
              try {
                await invoke('reset_onboarding_status_cmd');
                window.location.reload();
              } catch (error) {
                toast.error('Could not restart setup', { description: String(error) });
              }
            }}>
              Run setup
            </Button>
          }
        />
      </SettingsGroup>
```

- [ ] **Step 2: CLAUDE.md.** In the "This fork" section, after the "Menu bar toggle and call overlay" bullet, add:

```markdown
- Onboarding ([spec](docs/superpowers/specs/2026-10-30-onboarding-refresh-design.md)): six screens in `components/onboarding/steps/` (Welcome, You: folder/name/languages, Transcription: Parakeet plus enhance on/off and Whisper small/medium/large-v3, Permissions on macOS only, Calls, Claude Code). Each screen saves through `savePreferences` in `OnboardingContext`. `complete_onboarding` no longer writes the summary model and writes the Parakeet live config only when none exists. Settings > Recordings > Setup has "Run setup again" (`reset_onboarding_status_cmd`).
```

- [ ] **Step 3: Type check** `cd frontend && pnpm exec tsc --noEmit` and `pnpm test`. Expected: both pass.

- [ ] **Step 4: Commit**

```bash
/usr/bin/git add frontend/src/components/RecordingSettings.tsx CLAUDE.md
/usr/bin/git commit -m "feat(settings): run setup again; docs: onboarding in CLAUDE.md"
```
