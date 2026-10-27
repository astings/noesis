import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import type { RecordingPreferences } from '@/components/RecordingSettings';

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

  try {
    await invoke('start_retranscription_command', {
      meetingId,
      meetingFolderPath: folderPath,
      language: prefs.enhance_language,
      model: prefs.enhance_model,
      provider: prefs.enhance_provider,
    });
    toast.info('Enhancing transcript in the background', {
      description: `${prefs.enhance_provider} ${prefs.enhance_model}. The transcript updates when it finishes.`,
    });
    return true;
  } catch (error) {
    console.error('[autoEnhance] failed to start:', error);
    toast.error('Automatic enhance did not start', {
      description: error instanceof Error ? error.message : String(error),
    });
    return false;
  }
}
