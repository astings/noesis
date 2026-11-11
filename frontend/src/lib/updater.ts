// Auto-update (tauri-plugin-updater): GitHub releases latest.json, signed with the key in ~/.tauri.
// Checking is silent; installing always asks and never runs during a recording.
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

let offered: string | null = null; // version already shown in a toast this session

async function install(update: Update) {
  if (await invoke<boolean>('is_recording').catch(() => false)) {
    toast.error('Stop the recording first', { description: 'The update restarts Noesis.' });
    return;
  }
  const id = toast.loading(`Downloading Noesis ${update.version}…`);
  try {
    await update.downloadAndInstall();
    toast.dismiss(id);
    await relaunch();
  } catch (e) {
    toast.error('Update failed', { id, description: String(e) });
  }
}

export async function checkForUpdates({ manual = false } = {}) {
  try {
    const update = await check();
    if (!update) {
      if (manual) toast.success('Noesis is up to date');
      return;
    }
    if (!manual && offered === update.version) return;
    offered = update.version;
    toast.info(`Noesis ${update.version} is available`, {
      description: update.body?.split('\n')[0] || 'Installs and restarts the app.',
      duration: Infinity,
      action: { label: 'Install and restart', onClick: () => void install(update) },
    });
  } catch (e) {
    console.warn('Update check failed:', e);
    if (manual) toast.error('Could not check for updates', { description: String(e) });
  }
}
