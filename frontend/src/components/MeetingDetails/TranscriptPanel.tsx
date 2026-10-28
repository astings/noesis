"use client";

import { Transcript, TranscriptSegmentData } from '@/types';
import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { TranscriptButtonGroup } from './TranscriptButtonGroup';
import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useSidebar, MEETING_EXTRAS_CHANGED } from '@/components/Sidebar/SidebarProvider';
import { MeetingStatusTags } from '@/components/MeetingStatusTags';
import { format, isValid, parseISO } from 'date-fns';

interface TranscriptPanelProps {
  title: string;
  createdAt?: string;
  transcripts: Transcript[];
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  isRecording: boolean;
  disableAutoScroll?: boolean;

  // Optional pagination props (when using virtualization)
  usePagination?: boolean;
  segments?: TranscriptSegmentData[];
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;

  // Retranscription props
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
}

export function TranscriptPanel({
  title,
  createdAt,
  transcripts,
  onCopyTranscript,
  onOpenMeetingFolder,
  isRecording,
  disableAutoScroll = false,
  usePagination = false,
  segments,
  hasMore,
  isLoadingMore,
  totalCount,
  loadedCount,
  onLoadMore,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
}: TranscriptPanelProps) {
  // Convert transcripts to segments if pagination is not used but we want virtualization
  const convertedSegments = useMemo(() => {
    if (usePagination && segments) {
      return segments;
    }
    // Convert transcripts to segments for virtualization
    return transcripts.map(t => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      text: t.text,
      confidence: t.confidence,
      speaker: t.speaker,
    }));
  }, [transcripts, usePagination, segments]);

  const { meetings } = useSidebar();
  const status = meetings.find((m) => m.id === meetingId);
  const [extras, setExtras] = useState<{ tags: string[]; audio_deleted_at: string | null; has_audio: boolean } | null>(null);
  useEffect(() => {
    if (!meetingId) { setExtras(null); return; }
    const load = () => invoke<{ tags: string[]; audio_deleted_at: string | null; has_audio: boolean }>('get_meeting_extras', { meetingId }).then(setExtras).catch(() => setExtras(null));
    load();
    window.addEventListener(MEETING_EXTRAS_CHANGED, load);
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen<{ meeting_id: string }>('tagging-status-changed', (e) => { if (e.payload.meeting_id === meetingId) load(); })
      .then((fn) => { if (cancelled) fn(); else unlisten = fn; });
    return () => { cancelled = true; unlisten?.(); window.removeEventListener(MEETING_EXTRAS_CHANGED, load); };
  }, [meetingId]);
  const created = createdAt ? parseISO(createdAt) : null;

  return (
    <div className="flex h-full min-w-0 w-full bg-background flex-col relative">
      {/* Top bar: title, date, actions */}
      <div className="flex flex-wrap items-center gap-2 px-4 py-2.5 border-b border-border">
        <div className="mr-auto min-w-0">
          <h1 className="truncate text-sm font-semibold">
            {title}
            {created && isValid(created) && (
              <span className="font-normal text-muted-foreground"> · {format(created, 'd MMM, HH:mm')}</span>
            )}
          </h1>
          {extras && extras.tags.length > 0 && (
            <div className="mt-1 flex flex-wrap gap-1">
              {extras.tags.map((tag) => (
                <span key={tag} className="text-xs text-muted-foreground border border-border rounded px-1.5">{tag}</span>
              ))}
            </div>
          )}
        </div>
        <MeetingStatusTags enhanceStatus={status?.enhanceStatus} hookStatus={status?.hookStatus} taggingStatus={status?.taggingStatus} />
        <TranscriptButtonGroup
          transcriptCount={usePagination ? (totalCount ?? convertedSegments.length) : (transcripts?.length || 0)}
          onCopyTranscript={onCopyTranscript}
          onOpenMeetingFolder={onOpenMeetingFolder}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onRefetchTranscripts={onRefetchTranscripts}
          audioDeleted={!!extras?.audio_deleted_at}
        />
      </div>

      {/* Transcript content - use virtualized view for better performance */}
      <div className="flex-1 overflow-hidden">
        <VirtualizedTranscriptView
          segments={convertedSegments}
          isRecording={isRecording}
          isPaused={false}
          isProcessing={false}
          isStopping={false}
          enableStreaming={false}
          showConfidence={true}
          disableAutoScroll={disableAutoScroll}
          hasMore={hasMore}
          isLoadingMore={isLoadingMore}
          totalCount={totalCount}
          loadedCount={loadedCount}
          onLoadMore={onLoadMore}
        />
      </div>
    </div>
  );
}
