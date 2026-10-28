'use client';

import React, { createContext, useContext, useState, useEffect } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { INBOX } from '@/lib/projects';



interface SidebarItem {
  id: string;
  title: string;
  type: 'folder' | 'file';
  children?: SidebarItem[];
  enhanceStatus?: string;
}

export const PROJECT_FOLDER_PREFIX = 'project:';

export interface CurrentMeeting {
  id: string;
  title: string;
  // spec 4.1: "queued" | "running" | "done" | "failed", absent for legacy meetings
  enhanceStatus?: string;
  // spec 4.2: project folder name, `_inbox` for legacy meetings
  project?: string;
}

// Search result type for transcript search
interface TranscriptSearchResult {
  id: string;
  title: string;
  matchContext: string;
  timestamp: string;
}

interface SidebarContextType {
  currentMeeting: CurrentMeeting | null;
  setCurrentMeeting: (meeting: CurrentMeeting | null) => void;
  sidebarItems: SidebarItem[];
  isCollapsed: boolean;
  toggleCollapse: () => void;
  meetings: CurrentMeeting[];
  setMeetings: (meetings: CurrentMeeting[]) => void;
  isMeetingActive: boolean;
  setIsMeetingActive: (active: boolean) => void;
  handleRecordingToggle: () => void;
  searchTranscripts: (query: string) => Promise<void>;
  searchResults: TranscriptSearchResult[];
  isSearching: boolean;
  setServerAddress: (address: string) => void;
  serverAddress: string;
  transcriptServerAddress: string;
  setTranscriptServerAddress: (address: string) => void;
  // Refetch meetings from backend
  refetchMeetings: () => Promise<void>;

}

const SidebarContext = createContext<SidebarContextType | null>(null);

export const useSidebar = () => {
  const context = useContext(SidebarContext);
  if (!context) {
    throw new Error('useSidebar must be used within a SidebarProvider');
  }
  return context;
};

export function SidebarProvider({ children }: { children: React.ReactNode }) {
  const [currentMeeting, setCurrentMeeting] = useState<CurrentMeeting | null>({ id: 'intro-call', title: '+ New Call' });
  const [isCollapsed, setIsCollapsed] = useState(true);
  const [meetings, setMeetings] = useState<CurrentMeeting[]>([]);
  const [sidebarItems, setSidebarItems] = useState<SidebarItem[]>([]);
  const [isMeetingActive, setIsMeetingActive] = useState(false);
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [serverAddress, setServerAddress] = useState('');
  const [transcriptServerAddress, setTranscriptServerAddress] = useState('');

  // Use recording state from RecordingStateContext (single source of truth)
  const { isRecording } = useRecordingState();

  const pathname = usePathname();
  const router = useRouter();

  // Extract fetchMeetings as a reusable function
  const fetchMeetings = React.useCallback(async () => {
    if (serverAddress) {
      try {
        const meetings = await invoke('api_get_meetings') as Array<{ id: string, title: string, project?: string }>;
        // spec 4.1: statuses are optional; a failure here must not hide the meeting list
        const statuses = await invoke<Array<{ meeting_id: string; status: string }>>('list_enhance_statuses')
          .catch((error) => { console.warn('list_enhance_statuses failed:', error); return []; });
        const statusById = new Map(statuses.map((s) => [s.meeting_id, s.status]));
        const transformedMeetings = meetings.map((meeting: any) => ({
          id: meeting.id,
          title: meeting.title,
          project: meeting.project || INBOX,
          enhanceStatus: statusById.get(meeting.id),
        }));
        setMeetings(transformedMeetings);
      } catch (error) {
        console.error('Error fetching meetings:', error);
        setMeetings([]);
      }
    }
  }, [serverAddress]);

  useEffect(() => {
    fetchMeetings();
  }, [serverAddress, fetchMeetings]);

  useEffect(() => {
    const fetchSettings = async () => {
      setServerAddress('http://localhost:5167');
      setTranscriptServerAddress('http://127.0.0.1:8178/stream');
    };
    fetchSettings();
  }, []);

  // spec 4.1: live badge updates while a job moves through queued, running, done, failed
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    let cancelled = false;
    listen<{ meeting_id: string; status: string }>('enhance-status-changed', (event) => {
      setMeetings((prev) =>
        prev.map((m) => (m.id === event.payload.meeting_id ? { ...m, enhanceStatus: event.payload.status } : m)),
      );
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    // spec 4.1: an unattended job has no open dialog, so surface its error here
    listen<{ meeting_id: string; error: string }>('retranscription-error', (event) => {
      toast.error('Enhance failed', { description: event.payload.error });
    }).then((fn) => {
      if (cancelled) fn();
      else unlistenError = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
      unlistenError?.();
    };
  }, []);

  // spec 4.2: one collapsible section per project, Inbox first
  const meetingsByProject = new Map<string, CurrentMeeting[]>();
  for (const meeting of meetings) {
    const project = meeting.project || INBOX;
    const bucket = meetingsByProject.get(project);
    if (bucket) bucket.push(meeting); else meetingsByProject.set(project, [meeting]);
  }
  const projectNames = Array.from(meetingsByProject.keys()).sort((a, b) =>
    a === INBOX ? -1 : b === INBOX ? 1 : a.localeCompare(b),
  );
  const baseItems: SidebarItem[] = [
    {
      id: 'meetings',
      title: 'Meeting Notes',
      type: 'folder' as const,
      children: projectNames.map((project) => ({
        id: PROJECT_FOLDER_PREFIX + project,
        title: project === INBOX ? 'Inbox' : project,
        type: 'folder' as const,
        children: (meetingsByProject.get(project) ?? []).map((meeting) => ({
          id: meeting.id,
          title: meeting.title,
          type: 'file' as const,
          enhanceStatus: meeting.enhanceStatus,
        })),
      })),
    },
  ];


  const toggleCollapse = () => {
    setIsCollapsed(!isCollapsed);
  };

  // Update current meeting when on home page
  useEffect(() => {
    if (pathname === '/') {
      setCurrentMeeting({ id: 'intro-call', title: '+ New Call' });
    }
    setSidebarItems(baseItems);
  }, [pathname]);

  // Update sidebar items when meetings change
  useEffect(() => {
    setSidebarItems(baseItems);
  }, [meetings]);

  // Function to handle recording toggle from sidebar
  const handleRecordingToggle = () => {
    if (!isRecording) {
      // Check if already on home page
      if (pathname === '/') {
        // Already on home - trigger recording directly via custom event
        console.log('Triggering recording from sidebar (already on home page)');
        window.dispatchEvent(new CustomEvent('start-recording-from-sidebar'));
      } else {
        // Not on home - navigate and use auto-start mechanism
        console.log('Navigating to home page with auto-start flag');
        sessionStorage.setItem('autoStartRecording', 'true');
        router.push('/');
      }

    }
    // The actual recording start/stop is handled in the Home component
  };

  // Function to search through meeting transcripts
  const searchTranscripts = async (query: string) => {
    if (!query.trim()) {
      setSearchResults([]);
      return;
    }

    try {
      setIsSearching(true);


      const results = await invoke('api_search_transcripts', { query }) as TranscriptSearchResult[];
      setSearchResults(results);
    } catch (error) {
      console.error('Error searching transcripts:', error);
      setSearchResults([]);
    } finally {
      setIsSearching(false);
    }
  };

  return (
    <SidebarContext.Provider value={{
      currentMeeting,
      setCurrentMeeting,
      sidebarItems,
      isCollapsed,
      toggleCollapse,
      meetings,
      setMeetings,
      isMeetingActive,
      setIsMeetingActive,
      handleRecordingToggle,
      searchTranscripts,
      searchResults,
      isSearching,
      setServerAddress,
      serverAddress,
      transcriptServerAddress,
      setTranscriptServerAddress,
      refetchMeetings: fetchMeetings,

    }}>
      {children}
    </SidebarContext.Provider>
  );
}
