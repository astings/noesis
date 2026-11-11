'use client';

import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { checkForUpdates } from '@/lib/updater';

const EVERY_MS = 6 * 60 * 60 * 1000;

// At launch and every 6 hours while the `auto_check_updates` preference is on. Dev builds skip it.
export function UpdateChecker() {
  useEffect(() => {
    if (process.env.NODE_ENV !== 'production') return;
    const run = async () => {
      const prefs = await invoke<{ auto_check_updates?: boolean }>('get_recording_preferences').catch(() => null);
      if (prefs?.auto_check_updates !== false) await checkForUpdates();
    };
    const first = setTimeout(run, 10_000); // after startup work settles
    const timer = setInterval(run, EVERY_MS);
    return () => { clearTimeout(first); clearInterval(timer); };
  }, []);
  return null;
}
