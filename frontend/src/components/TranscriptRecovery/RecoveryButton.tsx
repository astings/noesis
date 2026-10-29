'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { TriangleAlert } from 'lucide-react';
import { toast } from 'sonner';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { RecordingStatus, useRecordingState } from '@/contexts/RecordingStateContext';
import { useTranscriptRecovery } from '@/hooks/useTranscriptRecovery';
import { indexedDBService } from '@/services/indexedDBService';
import { TranscriptRecovery } from './TranscriptRecovery';

const SHOWN_KEY = 'recovery_dialog_shown';
const STOP_FLOW = [RecordingStatus.STOPPING, RecordingStatus.PROCESSING_TRANSCRIPTS, RecordingStatus.SAVING];

/**
 * Title-bar warning for interrupted recordings; opens the recovery dialog.
 * Mounted once in the layout, so the check runs at startup and after every stop,
 * and the button stays visible on every page while something is recoverable.
 */
export function RecoveryButton() {
  const { isRecording, status } = useRecordingState();
  const { refetchMeetings } = useSidebar();
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const { recoverableMeetings, checkForRecoverableTranscripts, recoverMeeting, loadMeetingTranscripts, deleteRecoverableMeeting } =
    useTranscriptRecovery();

  // Idle only: a running or stopping recording looks recoverable until it is saved
  useEffect(() => {
    if (isRecording || STOP_FLOW.includes(status)) return;
    (async () => {
      await indexedDBService.deleteOldMeetings(7).catch((e) => console.warn('Failed to clean up old meetings:', e));
      await indexedDBService.deleteSavedMeetings(24).catch((e) => console.warn('Failed to clean up saved meetings:', e));
      await checkForRecoverableTranscripts();
    })();
  }, [checkForRecoverableTranscripts, isRecording, status]);

  // Pop the dialog once per session; the button stays as long as something is recoverable
  useEffect(() => {
    if (recoverableMeetings.length > 0 && !sessionStorage.getItem(SHOWN_KEY)) {
      sessionStorage.setItem(SHOWN_KEY, 'true');
      setOpen(true);
    }
  }, [recoverableMeetings]);

  const recover = async (meetingId: string) => {
    try {
      const result = await recoverMeeting(meetingId);
      if (!result.success) return;
      const view = () => { if (result.meetingId) router.push(`/meeting-details?id=${result.meetingId}`); };
      toast.success('Meeting recovered', {
        description: result.audioRecoveryStatus?.status === 'success' ? 'Transcripts and audio recovered' : 'Transcripts recovered (no audio available)',
        action: result.meetingId ? { label: 'View meeting', onClick: view } : undefined,
        duration: 10000,
      });
      await refetchMeetings();
      setTimeout(view, 2000);
    } catch (error) {
      toast.error('Failed to recover meeting', { description: error instanceof Error ? error.message : 'Unknown error occurred' });
      throw error;
    }
  };

  const close = () => {
    setOpen(false);
    if (recoverableMeetings.length === 0) sessionStorage.removeItem(SHOWN_KEY);
  };

  const n = recoverableMeetings.length;
  return (
    <>
      {n > 0 && (
        <button
          type="button"
          onClick={() => setOpen(true)}
          title="Interrupted recordings to recover"
          aria-label={`${n} interrupted recording${n > 1 ? 's' : ''} to recover`}
          className="relative flex h-7 w-7 items-center justify-center rounded-md text-warning hover:bg-accent/60"
        >
          <TriangleAlert className="h-4 w-4" />
          <span className="absolute -right-0.5 -top-0.5 min-w-[14px] rounded-full bg-warning px-1 text-center text-[10px] leading-[14px] text-background">{n}</span>
        </button>
      )}
      <TranscriptRecovery
        isOpen={open}
        onClose={close}
        recoverableMeetings={recoverableMeetings}
        onRecover={recover}
        onDelete={deleteRecoverableMeeting}
        onLoadPreview={loadMeetingTranscripts}
      />
    </>
  );
}
