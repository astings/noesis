"use client";

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { ProjectSelect } from '@/components/ProjectSelect';
import { INBOX, projectOfFolder } from '@/lib/projects';
import { useSidebar, runMeetingTagging, deleteMeetingAudio } from '@/components/Sidebar/SidebarProvider';
import { Button } from '@/components/ui/button';
import { ChevronDown, RefreshCw, SlidersHorizontal, Tags, Trash2 } from 'lucide-react';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { ConfirmationModal } from '@/components/ConfirmationModel/confirmation-modal';
import { startEnhance } from '@/lib/autoEnhance';
import { EnhanceWithDialog } from './EnhanceWithDialog';

interface TranscriptButtonGroupProps {
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
  audioDeleted?: boolean;
}

export function TranscriptButtonGroup({
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
  audioDeleted,
}: TranscriptButtonGroupProps) {
  const [showDeleteAudio, setShowDeleteAudio] = useState(false);
  const [showEnhanceWith, setShowEnhanceWith] = useState(false);
  const audioGone = audioDeleted ? 'Audio removed after enhance' : undefined;

  // spec 4.2: move the meeting to another project folder
  const { refetchMeetings, meetings } = useSidebar();
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

  return (
    <>
      {meetingId && meetingFolderPath && (
        <>
          <ProjectSelect
            value={currentProject}
            onChange={handleMove}
            disabled={moving}
            compact
            className="w-auto min-w-28 gap-1.5 px-2.5 text-[13px] shadow-none"
          />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="sm" className="px-2.5 text-[13px] shadow-none">
                Actions
                <ChevronDown />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem disabled={audioDeleted} title={audioGone} onSelect={() => startEnhance(meetingId, meetingFolderPath)}>
                <RefreshCw />
                Enhance
              </DropdownMenuItem>
              <DropdownMenuItem
                disabled={audioDeleted}
                title={audioGone}
                // deferred: the menu must finish closing before a Radix dialog takes focus
                onSelect={() => setTimeout(() => setShowEnhanceWith(true), 0)}
              >
                <SlidersHorizontal />
                Enhance with...
              </DropdownMenuItem>
              <DropdownMenuItem disabled={taggingRunning} onSelect={() => runMeetingTagging(meetingId)}>
                <Tags />
                Title &amp; tags
              </DropdownMenuItem>
              <DropdownMenuItem
                className="text-destructive focus:text-destructive"
                disabled={audioDeleted}
                title={audioGone}
                onSelect={() => setShowDeleteAudio(true)}
              >
                <Trash2 />
                Delete audio
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <ConfirmationModal
            isOpen={showDeleteAudio}
            text="Delete the audio of this meeting? The transcript stays. Enhance and playback will no longer be possible."
            onConfirm={() => { setShowDeleteAudio(false); deleteMeetingAudio(meetingId); }}
            onCancel={() => setShowDeleteAudio(false)}
          />
          <EnhanceWithDialog open={showEnhanceWith} onOpenChange={setShowEnhanceWith} meetingId={meetingId} folderPath={meetingFolderPath} />
        </>
      )}
    </>
  );
}
