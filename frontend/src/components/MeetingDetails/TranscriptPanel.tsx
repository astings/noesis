"use client";

import { Transcript, TranscriptSegmentData } from '@/types';
import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { TranscriptButtonGroup } from './TranscriptButtonGroup';
import { useEffect, useMemo, useState } from 'react';
import { useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useSidebar, MEETING_EXTRAS_CHANGED } from '@/components/Sidebar/SidebarProvider';
import { MeetingInfoButton } from '@/components/MeetingStatusTags';
import { toast } from 'sonner';
import { format, isValid, parseISO } from 'date-fns';
import { formatDuration } from '@/lib/duration';

interface TranscriptPanelProps {
  title: string;
  createdAt?: string;
  transcripts: Transcript[];
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

  // set by a title bar search hit: /meeting-details?id=…&t=<block start>&e=<block end>&q=<query>&n=<click>
  const searchParams = useSearchParams();
  const t = searchParams.get('t');
  const e = searchParams.get('e');
  const q = searchParams.get('q') ?? '';
  const n = searchParams.get('n');
  const highlight = useMemo(
    () => (t ? { seconds: Number(t), end: Number(e ?? t), query: q, key: `${t}|${n}` } : undefined),
    [t, e, q, n],
  );

  const { meetings, refetchMeetings } = useSidebar();
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
  const length = formatDuration(status?.durationSeconds);

  // Click the title to rename in place; Enter or blur saves, Escape cancels.
  const [titleDraft, setTitleDraft] = useState<string | null>(null);
  const saveTitle = async () => {
    const next = titleDraft?.trim();
    setTitleDraft(null);
    if (!meetingId || !next || next === title) return;
    try {
      await invoke('api_save_meeting_title', { meetingId, title: next });
      // the rename also moves the folder, so reload metadata and the sidebar
      await Promise.all([onRefetchTranscripts?.(), refetchMeetings()]);
    } catch (error) {
      toast.error('Failed to rename meeting', { description: String(error) });
    }
  };

  return (
    <div className="flex h-full min-w-0 w-full bg-background flex-col relative">
      {/* Top bar: title, date, actions */}
      <div data-tauri-drag-region className="flex flex-wrap items-center gap-2 px-4 py-2.5 border-b border-border">
        <div className="mr-auto min-w-0">
          <h1 className="flex min-w-0 items-center text-sm font-semibold">
            {titleDraft !== null ? (
              <input
                autoFocus
                aria-label="Meeting title"
                className="min-w-0 flex-1 rounded border border-input bg-transparent px-1 font-semibold outline-none focus:ring-1 focus:ring-ring"
                value={titleDraft}
                onChange={(e) => setTitleDraft(e.target.value)}
                onBlur={saveTitle}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') e.currentTarget.blur();
                  if (e.key === 'Escape') setTitleDraft(null);
                }}
              />
            ) : (
              <button
                type="button"
                title={meetingId ? 'Click to rename' : undefined}
                disabled={!meetingId}
                className="truncate rounded px-1 -mx-1 text-left hover:bg-accent/60 disabled:hover:bg-transparent"
                onClick={() => setTitleDraft(title)}
              >
                {title}
              </button>
            )}
            {created && isValid(created) && (
              <span className="ml-1 flex-none font-normal text-muted-foreground">
                · {format(created, 'd MMM, HH:mm')}{length && ` · ${length}`}
              </span>
            )}
          </h1>
        </div>
        {meetingId && (
          <MeetingInfoButton
            tags={extras?.tags ?? []}
            enhanceStatus={status?.enhanceStatus}
            taggingStatus={status?.taggingStatus}
          />
        )}
        <TranscriptButtonGroup
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
          highlight={highlight}
        />
      </div>
    </div>
  );
}
