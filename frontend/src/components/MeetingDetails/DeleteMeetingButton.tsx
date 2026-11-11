'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { ConfirmationModal } from '@/components/ConfirmationModel/confirmation-modal';

// Meeting header: delete the whole meeting (audio + transcript), same command as the sidebar's Delete
export function DeleteMeetingButton({ meetingId }: { meetingId: string }) {
  const [open, setOpen] = useState(false);
  const { refetchMeetings, setCurrentMeeting } = useSidebar();
  const router = useRouter();

  const remove = async () => {
    setOpen(false);
    try {
      await invoke('api_delete_meeting', { meetingId });
      toast.success('Meeting deleted', { description: 'The folder was moved to the Trash' });
      setCurrentMeeting({ id: 'intro-call', title: '+ New Call' });
      router.push('/');
    } catch (error) {
      toast.error('Failed to delete meeting', { description: error instanceof Error ? error.message : String(error) });
    }
    await refetchMeetings();
  };

  return (
    <>
      <button
        type="button"
        aria-label="Delete meeting"
        title="Delete meeting (audio and transcript)"
        onClick={() => setOpen(true)}
        className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground hover:bg-accent/60 hover:text-destructive"
      >
        <Trash2 className="h-4 w-4" />
      </button>
      <ConfirmationModal
        isOpen={open}
        text="Delete this meeting, audio and transcript? Its folder will be moved to the Trash."
        onConfirm={remove}
        onCancel={() => setOpen(false)}
      />
    </>
  );
}
