'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { RecordingControls } from '@/components/RecordingControls';
import { ProjectSelect } from '@/components/ProjectSelect'; // spec 4.2
import { INBOX, getLastProject, setLastProject } from '@/lib/projects'; // spec 4.2
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { usePermissionCheck } from '@/hooks/usePermissionCheck';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useConfig } from '@/contexts/ConfigContext';
import { StatusOverlays } from '@/app/_components/StatusOverlays';
import { SettingsModals } from './_components/SettingsModal';
import { TranscriptPanel } from './_components/TranscriptPanel';
import { AskPanel } from '@/components/AskPanel'; // live ask spec 3.2
import { useModalState } from '@/hooks/useModalState';
import { useRecordingStateSync } from '@/hooks/useRecordingStateSync';
import { useRecordingStart } from '@/hooks/useRecordingStart';
import { useRecordingStop } from '@/hooks/useRecordingStop';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Copy, GlobeIcon } from 'lucide-react';

export default function Home() {
  // Local page state (not moved to contexts)
  const [isRecording, setIsRecordingState] = useState(false);
  const [project, setProject] = useState<string>(() => (typeof window === 'undefined' ? INBOX : getLastProject())); // spec 4.2

  // The call overlay can change the project of the live recording
  useEffect(() => {
    const unlisten = listen<string>('recording-project-changed', (e) => { setProject(e.payload); setLastProject(e.payload); });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // Use contexts for state management
  const { meetingTitle, transcripts, copyTranscript } = useTranscripts();
  const { transcriptModelConfig, selectedDevices } = useConfig();
  const recordingState = useRecordingState();

  // Extract status from global state
  const { status, isStopping, isProcessing, isSaving } = recordingState;

  // Hooks
  const { hasMicrophone } = usePermissionCheck();
  const { setIsMeetingActive, isCollapsed: sidebarCollapsed } = useSidebar();
  const { modals, messages, showModal, hideModal } = useModalState(transcriptModelConfig);
  const { isRecordingDisabled, setIsRecordingDisabled } = useRecordingStateSync(isRecording, setIsRecordingState, setIsMeetingActive);
  const { handleRecordingStart } = useRecordingStart(isRecording, setIsRecordingState, showModal);

  // Get handleRecordingStop function and setIsStopping (state comes from global context)
  const { handleRecordingStop, setIsStopping } = useRecordingStop(
    setIsRecordingState,
    setIsRecordingDisabled
  );

  // Interrupted-recording recovery lives in the title bar (RecoveryButton)

  // Computed values using global status
  const isProcessingStop = status === RecordingStatus.PROCESSING_TRANSCRIPTS || isProcessing;

  return (
    <div className="flex flex-col h-full bg-background">
      {/* All Modals supported*/}
      <SettingsModals
        modals={modals}
        messages={messages}
        onClose={hideModal}
      />

      <div data-tauri-drag-region className="flex items-center gap-2 px-4 py-2.5 border-b border-border">
        <h1 className="mr-auto min-w-0 truncate text-sm font-semibold">
          {meetingTitle === '+ New Call' ? 'New recording' : meetingTitle}
        </h1>
        {transcripts.length > 0 && (
          <Button variant="ghost" size="sm" className="text-[13px]" onClick={copyTranscript} title="Copy transcript">
            <Copy />
            Copy
          </Button>
        )}
        {transcriptModelConfig.provider === 'localWhisper' && (
          <Button variant="ghost" size="sm" className="text-[13px]" onClick={() => showModal('languageSettings')} title="Language">
            <GlobeIcon />
            Language
          </Button>
        )}
        {/* spec 4.2 */}
        <ProjectSelect
          value={project}
          onChange={async (p) => {
            if (!recordingState.isRecording) { setProject(p); setLastProject(p); return; }
            // spec 2: the meeting moves to this project after stop, before auto-enhance
            try {
              const applied = await invoke<string>('set_recording_project', { project: p });
              setProject(applied);
              setLastProject(applied);
            } catch (error) {
              toast.error('Could not change the project', { description: String(error) });
            }
          }}
          disabled={isRecordingDisabled}
          compact
          className="w-auto min-w-28 gap-1.5 rounded-full text-muted-foreground shadow-none"
        />
      </div>

      <div className="flex flex-1 min-h-0 flex-col overflow-hidden">
        <TranscriptPanel
          isProcessingStop={isProcessingStop}
          isStopping={isStopping}
        />

        {/* live ask spec 3.2: while recording, or while live segments are still on screen */}
        {(recordingState.isRecording || transcripts.length > 0) && (
          <AskPanel hasSegments={transcripts.length > 0} disabled={isStopping || isProcessingStop || status === RecordingStatus.SAVING || !recordingState.isRecording} />
        )}

        {/* Recording controls - only show when permissions are granted or already recording and not showing status messages */}
        {(hasMicrophone || isRecording) &&
          status !== RecordingStatus.PROCESSING_TRANSCRIPTS &&
          status !== RecordingStatus.SAVING && (
            <div className="flex justify-center border-t border-border px-4 py-3.5">
              <RecordingControls
                isRecording={recordingState.isRecording}
                onRecordingStop={(callApi = true) => handleRecordingStop(callApi)}
                onRecordingStart={handleRecordingStart}
                onStopInitiated={() => setIsStopping(true)}
                onTranscriptionError={(message) => {
                  showModal('errorAlert', message);
                }}
                isRecordingDisabled={isRecordingDisabled}
                isParentProcessing={isProcessingStop}
                selectedDevices={selectedDevices}
                meetingName={meetingTitle}
              />
            </div>
          )}

        {/* Status Overlays - Processing and Saving */}
        <StatusOverlays
          isProcessing={status === RecordingStatus.PROCESSING_TRANSCRIPTS && !recordingState.isRecording}
          isSaving={status === RecordingStatus.SAVING}
          sidebarCollapsed={sidebarCollapsed}
        />
      </div>
    </div>
  );
}
