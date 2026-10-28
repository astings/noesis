"use client";
import { motion } from 'framer-motion';
import { TranscriptPanel } from '@/components/MeetingDetails/TranscriptPanel';
import { MeetingDetailsSplitView } from '@/components/MeetingDetails/MeetingDetailsSplitView';

// Custom hooks
import { useMeetingData } from '@/hooks/meeting-details/useMeetingData';
import { useMeetingOperations } from '@/hooks/meeting-details/useMeetingOperations';

export default function PageContent({
  meeting,
  onRefetchTranscripts,
  // Pagination props for efficient transcript loading
  segments,
  hasMore,
  isLoadingMore,
  totalCount,
  loadedCount,
  onLoadMore,
}: {
  meeting: any;
  onRefetchTranscripts?: () => Promise<void>;
  // Pagination props
  segments?: any[];
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;
}) {
  const isRecording = false;

  // Custom hooks
  const meetingData = useMeetingData({ meeting });

  const meetingOperations = useMeetingOperations({
    meeting,
  });

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
      className="flex flex-col h-screen min-w-0 bg-background"
    >
      <div className="flex flex-1 min-w-0 overflow-hidden">
        <MeetingDetailsSplitView
          transcript={
            <TranscriptPanel
              title={meetingData.meetingTitle}
              createdAt={meeting.created_at}
              transcripts={meetingData.transcripts}
              onOpenMeetingFolder={meetingOperations.handleOpenMeetingFolder}
              isRecording={isRecording}
              disableAutoScroll={true}
              usePagination={true}
              segments={segments}
              hasMore={hasMore}
              isLoadingMore={isLoadingMore}
              totalCount={totalCount}
              loadedCount={loadedCount}
              onLoadMore={onLoadMore}
              meetingId={meeting.id}
              meetingFolderPath={meeting.folder_path}
              onRefetchTranscripts={onRefetchTranscripts}
            />
          }
        />
      </div>
    </motion.div>
  );
}
