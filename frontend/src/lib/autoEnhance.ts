import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import type { RecordingPreferences } from '@/components/RecordingSettings';
import { languageSuffix, providerName } from '@/lib/languages';

/** One-off choices for an enhance run; null or absent means "resolve" (project.json, then Settings). */
export interface EnhanceOverride { language?: string | null; provider?: string | null; model?: string | null }

interface Started { meeting_id: string; message: string; provider: string; model: string; language: string | null }

/**
 * spec 4.1: start the enhance (full-file re-transcription) job right after a
 * recording is saved, when the `auto_enhance` preference is on. Fire and
 * forget: progress and completion arrive as `retranscription-progress`,
 * `retranscription-complete` and `enhance-status-changed` events.
 */
export async function startAutoEnhanceIfEnabled(
  meetingId: string,
  folderPath: string | null,
): Promise<boolean> {
  if (!folderPath) {
    console.warn('[autoEnhance] no folder path for meeting, skipping', meetingId);
    return false;
  }

  let prefs: RecordingPreferences;
  try {
    prefs = await invoke<RecordingPreferences>('get_recording_preferences');
  } catch (error) {
    console.warn('[autoEnhance] could not read preferences, skipping:', error);
    return false;
  }
  // spec 4.1: no audio file is written when auto_save is off, so there is nothing to enhance
  if (!prefs.auto_enhance || !prefs.auto_save) return false;
  return startEnhance(meetingId, folderPath);
}

/**
 * Start the enhance job. Fields left null are resolved by Rust: the project's project.json,
 * then Settings (spec 2026-10-30 section 3.4). The toast names what actually runs.
 */
export async function startEnhance(
  meetingId: string,
  folderPath: string,
  override: EnhanceOverride = {},
): Promise<boolean> {
  try {
    const started = await invoke<Started>('start_retranscription_command', {
      meetingId,
      meetingFolderPath: folderPath,
      language: override.language ?? null,
      model: override.model ?? null,
      provider: override.provider ?? null,
    });
    const what = [`${providerName(started.provider)} ${started.model}`, languageSuffix(started.language)].filter(Boolean).join(' · ');
    toast.info('Enhancing transcript in the background', {
      description: `${what}. The transcript updates when it finishes.`,
    });
    return true;
  } catch (error) {
    console.error('[autoEnhance] failed to start:', error);
    toast.error('Enhance did not start', {
      description: error instanceof Error ? error.message : String(error),
    });
    return false;
  }
}
