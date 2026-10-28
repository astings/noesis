"use client";

import { useEffect, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { ProjectSelect } from '@/components/ProjectSelect';
import { INBOX, projectOfFolder } from '@/lib/projects';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { Button } from '@/components/ui/button';
import { Copy, FolderOpen, RefreshCw } from 'lucide-react';
import { RetranscribeDialog } from './RetranscribeDialog';

interface TranscriptButtonGroupProps {
  transcriptCount: number;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
}

export function TranscriptButtonGroup({
  transcriptCount,
  onCopyTranscript,
  onOpenMeetingFolder,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
}: TranscriptButtonGroupProps) {
  const [showRetranscribeDialog, setShowRetranscribeDialog] = useState(false);

  // spec 4.2: move the meeting to another project folder
  const { refetchMeetings } = useSidebar();
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
            title="Retranscribe to enhance your recorded audio"
          >
            <RefreshCw />
            Enhance
          </Button>
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
