"use client";

import { useEffect, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { ProjectSelect } from '@/components/ProjectSelect';
import { INBOX, projectOfFolder } from '@/lib/projects';
import { useSidebar, runMeetingHook, runMeetingTagging, deleteMeetingAudio } from '@/components/Sidebar/SidebarProvider';
import { Button } from '@/components/ui/button';
import { Copy, FolderOpen, RefreshCw, Tags, Trash2, Webhook } from 'lucide-react';
import { ConfirmationModal } from '@/components/ConfirmationModel/confirmation-modal';
import { RetranscribeDialog } from './RetranscribeDialog';

interface TranscriptButtonGroupProps {
  transcriptCount: number;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
  audioDeleted?: boolean;
}

export function TranscriptButtonGroup({
  transcriptCount,
  onCopyTranscript,
  onOpenMeetingFolder,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
  audioDeleted,
}: TranscriptButtonGroupProps) {
  const [showRetranscribeDialog, setShowRetranscribeDialog] = useState(false);
  const [showDeleteAudio, setShowDeleteAudio] = useState(false);
  const audioGone = audioDeleted ? 'Audio removed after enhance' : undefined;

  // spec 4.2: move the meeting to another project folder
  const { refetchMeetings, meetings } = useSidebar();
  const hookRunning = meetings.find((m) => m.id === meetingId)?.hookStatus === 'running';
  const taggingRunning = meetings.find((m) => m.id === meetingId)?.taggingStatus === 'running';
  const [root, setRoot] = useState<string>('');
  const [moving, setMoving] = useState(false);
  useEffect(() => {
    invoke<string>('get_default_recordings_folder_path').then(setRoot).catch(() => setRoot(''));
  }, []);
  const currentProject = meetingFolderPath && root ? projectOfFolder(root, meetingFolderPath) : INBOX;

  const handleMove = async (project: string) => {
    if (!meetingId || project === currentProject) return;
    setMoving(true);
    try {
      await invoke<string>('assign_meeting', { meetingId, project });
      toast.success(`Moved to ${project === INBOX ? 'Inbox' : project}`);
      await refetchMeetings();
      await onRefetchTranscripts?.();
    } catch (error) {
      toast.error('Could not move the meeting', { description: error instanceof Error ? error.message : String(error) });
    } finally {
      setMoving(false);
    }
  };

  const handleRetranscribeComplete = useCallback(async () => {
    // Refetch transcripts to show the updated data
    if (onRefetchTranscripts) {
      await onRefetchTranscripts();
    }
  }, [onRefetchTranscripts]);

  return (
    <>
      <Button
        variant="outline"
        size="sm"
        className="px-2.5 text-[13px] shadow-none"
        onClick={onCopyTranscript}
        disabled={transcriptCount === 0}
        title={transcriptCount === 0 ? 'No transcript available' : 'Copy transcript'}
      >
        <Copy />
        Copy
      </Button>

      <Button
        size="sm"
        variant="outline"
        className="px-2.5 text-[13px] shadow-none"
        onClick={onOpenMeetingFolder}
        title="Open the meeting folder"
      >
        <FolderOpen />
        Open folder
      </Button>

      {meetingId && meetingFolderPath && (
        <>
          <ProjectSelect
            value={currentProject}
            onChange={handleMove}
            disabled={moving}
            compact
            className="w-auto min-w-28 gap-1.5 px-2.5 text-[13px] shadow-none"
          />
          <Button
            size="sm"
            className="px-2.5 text-[13px] shadow-none"
            onClick={() => setShowRetranscribeDialog(true)}
            disabled={audioDeleted}
            title={audioGone ?? 'Retranscribe to enhance your recorded audio'}
          >
            <RefreshCw />
            Enhance
          </Button>
          <Button
            size="sm"
            className="px-2.5 text-[13px] shadow-none"
            onClick={() => runMeetingHook(meetingId)}
            disabled={hookRunning}
            title="Run the post-enhance hook for this meeting"
          >
            <Webhook />
            Run hook
          </Button>
          <Button
            size="sm"
            className="px-2.5 text-[13px] shadow-none"
            onClick={() => runMeetingTagging(meetingId)}
            disabled={taggingRunning}
            title="Name and tag this meeting with the summary model"
          >
            <Tags />
            Title &amp; tags
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="px-2.5 text-[13px] text-destructive shadow-none"
            onClick={() => setShowDeleteAudio(true)}
            disabled={audioDeleted}
            title={audioGone ?? 'Delete the audio of this meeting'}
          >
            <Trash2 />
            Delete audio
          </Button>
          <ConfirmationModal
            isOpen={showDeleteAudio}
            text="Delete the audio of this meeting? The transcript stays. Enhance and playback will no longer be possible."
            onConfirm={() => { setShowDeleteAudio(false); deleteMeetingAudio(meetingId); }}
            onCancel={() => setShowDeleteAudio(false)}
          />
          <RetranscribeDialog
            open={showRetranscribeDialog}
            onOpenChange={setShowRetranscribeDialog}
            meetingId={meetingId}
            meetingFolderPath={meetingFolderPath}
            onComplete={handleRetranscribeComplete}
          />
        </>
      )}
    </>
  );
}
