'use client';

/**
 * The call detector asks for a recording through the `auto-start-requested` event instead of
 * reloading the page. A reload killed the previous meeting's pending save when two calls ran
 * back to back: no title, no tags, no enhance pass, and the meeting came back as "interrupted".
 * The request waits until the stop flow has settled, then takes the sidebar button's path.
 */
import { useEffect, useState } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { listen } from '@tauri-apps/api/event';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { canAutoStartNow } from '@/lib/autoStart';

interface AutoStartRequest {
  title: string | null;
}

export function AutoStartListener() {
  const { status } = useRecordingState();
  const router = useRouter();
  const pathname = usePathname();
  const [pending, setPending] = useState<AutoStartRequest | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<AutoStartRequest>('auto-start-requested', (event) => {
      console.log('[AutoStart] requested by the call detector', event.payload);
      setPending(event.payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!pending || !canAutoStartNow(status)) return;
    setPending(null);
    if (pending.title) sessionStorage.setItem('autoStartTitle', pending.title);
    sessionStorage.setItem('autoStartRecording', 'true');
    if (pathname === '/') {
      window.dispatchEvent(new Event('auto-start-recording'));
    } else {
      router.push('/');
    }
  }, [pending, status, pathname, router]);

  return null;
}
