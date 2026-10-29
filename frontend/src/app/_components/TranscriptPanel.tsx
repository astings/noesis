import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { PermissionWarning } from '@/components/PermissionWarning';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { usePermissionCheck } from '@/hooks/usePermissionCheck';
import { useIsLinux } from '@/hooks/usePlatform';
import { RecentMeetings } from '@/components/RecentMeetings';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useMemo } from 'react';

/**
 * TranscriptPanel Component
 *
 * Scrollable live transcript body of the home screen. Copy and Language live in the page's top bar.
 * Uses TranscriptContext and RecordingStateContext internally.
 */

interface TranscriptPanelProps {
  // indicates stop-processing state for transcripts; derived from backend statuses.
  isProcessingStop: boolean;
  isStopping: boolean;
}

export function TranscriptPanel({
  isProcessingStop,
  isStopping,
}: TranscriptPanelProps) {
  // Contexts
  const { transcripts, transcriptContainerRef } = useTranscripts();
  const { isRecording, isPaused } = useRecordingState();
  const { checkPermissions, isChecking, hasSystemAudio, hasMicrophone } = usePermissionCheck();
  const isLinux = useIsLinux();
  const { meetings } = useSidebar();

  // Convert transcripts to segments for virtualized view
  const segments = useMemo(() =>
    transcripts.map(t => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      text: t.text,
      confidence: t.confidence,
      speaker: t.speaker,
    })),
    [transcripts]
  );
  const showRecent = !isRecording && !isStopping && !isProcessingStop && segments.length === 0 && meetings.length > 0;

  return (
    <div ref={transcriptContainerRef} className="flex-1 min-h-0 bg-background flex flex-col overflow-y-auto">
      {/* Permission Warning - Not needed on Linux */}
      {!isRecording && !isChecking && !isLinux && (
        <div className="px-6 pt-5 empty:hidden">
          <PermissionWarning
            hasMicrophone={hasMicrophone}
            hasSystemAudio={hasSystemAudio}
            onRecheck={checkPermissions}
            isRechecking={isChecking}
          />
        </div>
      )}

      {showRecent ? (
        <RecentMeetings />
      ) : (
        <VirtualizedTranscriptView
          segments={segments}
          isRecording={isRecording}
          isPaused={isPaused}
          isProcessing={isProcessingStop}
          isStopping={isStopping}
          enableStreaming={isRecording}
          showConfidence={true}
        />
      )}
    </div>
  );
}
