'use client';

import React, { useState, useMemo, useEffect, useCallback } from 'react';
import { ChevronDown, ChevronRight, Settings, Trash2, Search, Pencil, Check, FolderPlus, NotebookPen, SearchIcon, X, PanelLeftClose, PanelLeftOpen, Home } from 'lucide-react';
import { useRouter, usePathname } from 'next/navigation';
import { useSidebar } from './SidebarProvider';
import type { CurrentMeeting } from '@/components/Sidebar/SidebarProvider';
import { ConfirmationModal } from '../ConfirmationModel/confirmation-modal';
import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';

import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog"
import { VisuallyHidden } from "@/components/ui/visually-hidden"

import {
  ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator,
  ContextMenuSub, ContextMenuSubContent, ContextMenuSubTrigger, ContextMenuTrigger,
} from '@/components/ui/context-menu';
import { INBOX, normalizeProjectName } from '@/lib/projects';
import { PROJECT_FOLDER_PREFIX, runMeetingHook } from '@/components/Sidebar/SidebarProvider';
import { MeetingStatusTags } from '@/components/MeetingStatusTags';
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '../ui/input-group';

interface SidebarItem {
  id: string;
  title: string;
  type: 'folder' | 'file';
  children?: SidebarItem[];
  enhanceStatus?: string;
  hookStatus?: string;
}

const Sidebar: React.FC = () => {
  const router = useRouter();
  const pathname = usePathname();
  const {
    currentMeeting,
    setCurrentMeeting,
    sidebarItems,
    isCollapsed,
    toggleCollapse,
    handleRecordingToggle,
    searchTranscripts,
    searchResults,
    isSearching,
    meetings,
    setMeetings,
    refetchMeetings
  } = useSidebar();

  // Get recording state from RecordingStateContext (single source of truth)
  const { isRecording } = useRecordingState();
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(new Set(['meetings']));
  // spec 4.2: project sections are open unless the user collapsed them; the choice is kept per device
  const COLLAPSED_PROJECTS_KEY = 'meetily.collapsedProjects';
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(() => {
    try {
      return new Set<string>(JSON.parse(localStorage.getItem(COLLAPSED_PROJECTS_KEY) || '[]'));
    } catch {
      return new Set<string>();
    }
  });
  const [searchQuery, setSearchQuery] = useState<string>('');

  // State for edit modal
  const [editModalState, setEditModalState] = useState<{ isOpen: boolean; meetingId: string | null; currentTitle: string }>({
    isOpen: false,
    meetingId: null,
    currentTitle: ''
  });
  const [editingTitle, setEditingTitle] = useState<string>('');

  // Ensure 'meetings' folder is always expanded
  useEffect(() => {
    if (!expandedFolders.has('meetings')) {
      const newExpanded = new Set(expandedFolders);
      newExpanded.add('meetings');
      setExpandedFolders(newExpanded);
    }
  }, [expandedFolders]);

  const [deleteModalState, setDeleteModalState] = useState<{ isOpen: boolean; itemId: string | null }>({ isOpen: false, itemId: null });

  // Handle search input changes
  const handleSearchChange = useCallback(async (value: string) => {
    setSearchQuery(value);

    // If search query is empty, just return to normal view
    if (!value.trim()) return;

    // Search through transcripts
    await searchTranscripts(value);

    // Make sure the meetings folder is expanded when searching
    if (!expandedFolders.has('meetings')) {
      const newExpanded = new Set(expandedFolders);
      newExpanded.add('meetings');
      setExpandedFolders(newExpanded);
    }
  }, [expandedFolders, searchTranscripts]);

  // spec 4.2: filter meetings inside project sections; drop empty sections, keep the top folder
  const filterTree = (items: SidebarItem[], keep: (item: SidebarItem) => boolean): SidebarItem[] => {
    const out: SidebarItem[] = [];
    for (const item of items) {
      if (item.type === 'folder') {
        const children = filterTree(item.children ?? [], keep);
        if (children.length > 0 || item.id === 'meetings') out.push({ ...item, children });
      } else if (keep(item)) {
        out.push(item);
      }
    }
    return out;
  };

  // Combine search results with sidebar items
  const filteredSidebarItems = useMemo(() => {
    if (!searchQuery.trim()) return sidebarItems;
    const q = searchQuery.toLowerCase();
    if (searchResults.length > 0) {
      const matchedMeetingIds = new Set(searchResults.map(result => result.id));
      return filterTree(sidebarItems, item => matchedMeetingIds.has(item.id) || item.title.toLowerCase().includes(q));
    }
    return filterTree(sidebarItems, item => item.title.toLowerCase().includes(q));
  }, [sidebarItems, searchQuery, searchResults, expandedFolders]);

  const handleDelete = async (itemId: string) => {
    console.log('Deleting item:', itemId);
    const payload = {
      meetingId: itemId
    };

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('api_delete_meeting', {
        meetingId: itemId,
      });
      console.log('Meeting deleted successfully');
      const updatedMeetings = meetings.filter((m: CurrentMeeting) => m.id !== itemId);
      setMeetings(updatedMeetings);

      // Show success toast
      toast.success("Meeting deleted successfully", {
        description: "The folder was moved to the Trash"
      });

      // If deleting the active meeting, navigate to home
      if (currentMeeting?.id === itemId) {
        setCurrentMeeting({ id: 'intro-call', title: '+ New Call' });
        router.push('/');
      }
    } catch (error) {
      console.error('Failed to delete meeting:', error);
      // The meeting is kept when the Trash move fails; refresh so the list matches the backend.
      await refetchMeetings();
      toast.error("Failed to delete meeting", {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  // Right-click "Move to": project list is fetched when a menu opens. moveMeeting is the single move path (reused by drag-and-drop).
  const [projects, setProjects] = useState<string[]>([INBOX]);
  const loadProjects = (open: boolean) => {
    if (!open) return;
    invoke<string[]>('list_projects_command')
      .then((list) => setProjects(Array.from(new Set([INBOX, ...list])).sort((a, b) => (a === INBOX ? -1 : b === INBOX ? 1 : a.localeCompare(b)))))
      .catch((error) => console.warn('list_projects_command failed:', error));
  };

  // "New project...": the dialog normalizes the name like ProjectSelect; assign_meeting creates the folder.
  const [newProject, setNewProject] = useState<{ meetingId: string; draft: string } | null>(null);
  const confirmNewProject = () => {
    if (!newProject) return;
    const name = normalizeProjectName(newProject.draft);
    if (!name) return;
    setNewProject(null);
    moveMeeting(newProject.meetingId, name);
  };

  const moveMeeting = async (meetingId: string, project: string) => {
    try {
      await invoke<string>('assign_meeting', { meetingId, project });
      await refetchMeetings();
      toast.success(`Moved to ${project === INBOX ? 'Inbox' : project}`);
    } catch (error) {
      toast.error('Could not move the meeting', { description: error instanceof Error ? error.message : String(error) });
      return;
    }
    // The open details page caches folder_path; it refetches on this event. A failing emit must not read as a failed move.
    try {
      await emit('meeting-moved', { meeting_id: meetingId });
    } catch (error) {
      console.warn('meeting-moved emit failed:', error);
    }
  };

  // Drag a meeting row onto a project section (header or its list). dragSource is the dragged meeting's current project.
  const DRAG_TYPE = 'application/x-noesis-meeting';
  const [dragSource, setDragSource] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const canDrop = (e: React.DragEvent, project: string) => dragSource !== null && dragSource !== project && e.dataTransfer.types.includes(DRAG_TYPE);

  const handleDeleteConfirm = () => {
    if (deleteModalState.itemId) {
      handleDelete(deleteModalState.itemId);
    }
    setDeleteModalState({ isOpen: false, itemId: null });
  };

  // Handle modal editing of meeting names
  const handleEditStart = (meetingId: string, currentTitle: string) => {
    setEditModalState({
      isOpen: true,
      meetingId: meetingId,
      currentTitle: currentTitle
    });
    setEditingTitle(currentTitle);
  };

  const handleEditConfirm = async () => {
    const newTitle = editingTitle.trim();
    const meetingId = editModalState.meetingId;

    if (!meetingId) return;

    // Prevent empty titles
    if (!newTitle) {
      toast.error("Meeting title cannot be empty");
      return;
    }

    try {
      await invoke('api_save_meeting_title', {
        meetingId: meetingId,
        title: newTitle,
      });

      // Update local state
      const updatedMeetings = meetings.map((m: CurrentMeeting) =>
        m.id === meetingId ? { ...m, title: newTitle } : m
      );
      setMeetings(updatedMeetings);

      // Update current meeting if it's the one being edited
      if (currentMeeting?.id === meetingId) {
        setCurrentMeeting({ id: meetingId, title: newTitle });
      }

      toast.success("Meeting title updated successfully");

      // Close modal and reset state
      setEditModalState({ isOpen: false, meetingId: null, currentTitle: '' });
      setEditingTitle('');
    } catch (error) {
      console.error('Failed to update meeting title:', error);
      toast.error("Failed to update meeting title", {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleEditCancel = () => {
    setEditModalState({ isOpen: false, meetingId: null, currentTitle: '' });
    setEditingTitle('');
  };

  const toggleFolder = (folderId: string) => {
    if (folderId.startsWith(PROJECT_FOLDER_PREFIX)) {
      const next = new Set(collapsedProjects);
      if (next.has(folderId)) next.delete(folderId); else next.add(folderId);
      setCollapsedProjects(next);
      try { localStorage.setItem(COLLAPSED_PROJECTS_KEY, JSON.stringify(Array.from(next))); } catch { /* per-device convenience only */ }
      return;
    }
    // Normal toggle behavior for all other folders
    const newExpanded = new Set(expandedFolders);
    if (newExpanded.has(folderId)) {
      newExpanded.delete(folderId);
    } else {
      newExpanded.add(folderId);
    }
    setExpandedFolders(newExpanded);
  };

  const renderCollapsedIcons = () => {
    if (!isCollapsed) return null;

    const isMeetingPage = pathname?.includes('/meeting-details');
    const isSettingsPage = pathname === '/settings';
    const railButton = 'p-2 rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground';

    return (
      <TooltipProvider>
        <div className="flex flex-1 flex-col items-center gap-1 py-2.5">
          <button onClick={() => router.push('/')} aria-label="Home" className={`${railButton} mb-2`}>
            <Home className="w-4 h-4" />
          </button>

          <Tooltip>
            <TooltipTrigger asChild>
              <button onClick={toggleCollapse} aria-label="Search transcripts" className={railButton}>
                <Search className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Search transcripts</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={toggleCollapse}
                aria-label="Projects"
                className={`${railButton} ${isMeetingPage ? 'bg-accent text-foreground' : ''}`}
              >
                <NotebookPen className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Projects</p>
            </TooltipContent>
          </Tooltip>

          <div className="flex-1" />

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={handleRecordingToggle}
                disabled={isRecording}
                aria-label={isRecording ? 'Recording in progress' : 'New recording'}
                className="grid h-8 w-8 place-items-center rounded-md bg-foreground transition-opacity hover:opacity-90 disabled:opacity-50"
              >
                <span className={`h-2 w-2 rounded-full bg-recording ${isRecording ? 'animate-pulse' : ''}`} />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>{isRecording ? 'Recording in progress...' : 'New recording'}</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => router.push('/settings')}
                aria-label="Settings"
                className={`${railButton} ${isSettingsPage ? 'bg-accent text-foreground' : ''}`}
              >
                <Settings className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Settings</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button onClick={toggleCollapse} aria-label="Expand sidebar" className={railButton}>
                <PanelLeftOpen className="w-4 h-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Expand sidebar</p>
            </TooltipContent>
          </Tooltip>
        </div>
      </TooltipProvider>
    );
  };

  // Find matching transcript snippet for a meeting item
  const findMatchingSnippet = (itemId: string) => {
    if (!searchQuery.trim() || !searchResults.length) return null;
    return searchResults.find(result => result.id === itemId);
  };

  const renderItem = (item: SidebarItem, depth = 0) => {
    const isExpanded = item.id.startsWith(PROJECT_FOLDER_PREFIX)
      ? !collapsedProjects.has(item.id)
      : expandedFolders.has(item.id);
    const isActive = item.type === 'file' && currentMeeting?.id === item.id;
    const isMeetingItem = item.type === 'file' && item.id.includes('-') && !item.id.startsWith('intro-call');

    // Check if this item has a matching transcript snippet
    const matchingResult = isMeetingItem ? findMatchingSnippet(item.id) : null;
    const hasTranscriptMatch = !!matchingResult;

    if (isCollapsed) return null;

    const isProject = item.type === 'folder';
    const currentProject = meetings.find((m: CurrentMeeting) => m.id === item.id)?.project || INBOX;

    const row = (
        <div
          draggable={isMeetingItem && item.hookStatus !== 'running'}
          onDragStart={isMeetingItem ? (e) => {
            e.dataTransfer.setData(DRAG_TYPE, item.id);
            e.dataTransfer.effectAllowed = 'move';
            setDragSource(currentProject);
          } : undefined}
          onDragEnd={isMeetingItem ? () => { setDragSource(null); setDropTarget(null); } : undefined}
          className={`group flex items-center gap-1.5 px-2 py-1 rounded-md text-sm cursor-pointer transition-colors ${isActive
            ? 'bg-accent text-foreground'
            : hasTranscriptMatch
              ? 'bg-warning/10'
              : isProject
                ? 'text-foreground hover:bg-accent/60'
                : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground'
            }`}
          style={isProject ? undefined : { paddingLeft: '22px' }}
          onClick={() => {
            if (item.type === 'folder') {
              toggleFolder(item.id);
            } else {
              setCurrentMeeting({ id: item.id, title: item.title });
              const basePath = item.id.startsWith('intro-call') ? '/' : `/meeting-details?id=${item.id}`;
              router.push(basePath);
            }
          }}
        >
          {isProject ? (
            <>
              {isExpanded ? (
                <ChevronDown className="w-3.5 h-3.5 flex-none text-muted-foreground" />
              ) : (
                <ChevronRight className="w-3.5 h-3.5 flex-none text-muted-foreground" />
              )}
              <span className="truncate">{item.title}</span>
            </>
          ) : (
            <div className="flex flex-col w-full min-w-0">
              <div className="flex items-center w-full min-w-0">
                <span className="flex-1 truncate" title={item.title}>{item.title}</span>
                {isMeetingItem && <MeetingStatusTags enhanceStatus={item.enhanceStatus} hookStatus={item.hookStatus} />}
                {isMeetingItem && (
                  <div className="flex items-center opacity-0 group-hover:opacity-100 transition-opacity duration-150">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleEditStart(item.id, item.title);
                      }}
                      className="p-0.5 rounded hover:text-foreground flex-shrink-0"
                      aria-label="Edit meeting title"
                    >
                      <Pencil className="w-3.5 h-3.5" />
                    </button>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        setDeleteModalState({ isOpen: true, itemId: item.id });
                      }}
                      className="p-0.5 rounded hover:text-destructive flex-shrink-0"
                      aria-label="Delete meeting"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                )}
              </div>

              {/* Show transcript match snippet if available */}
              {hasTranscriptMatch && (
                <div className="mt-1 text-xs text-muted-foreground line-clamp-2">
                  <span className="font-medium text-warning">Match:</span> {matchingResult.matchContext}
                </div>
              )}
            </div>
          )}
        </div>
    );

    const projectName = isProject ? item.id.slice(PROJECT_FOLDER_PREFIX.length) : '';
    const dropProps = isProject ? {
      onDragOver: (e: React.DragEvent) => {
        if (!canDrop(e, projectName)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = 'move';
        if (dropTarget !== projectName) setDropTarget(projectName);
      },
      onDragLeave: (e: React.DragEvent) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropTarget(null);
      },
      onDrop: (e: React.DragEvent) => {
        setDropTarget(null);
        if (!canDrop(e, projectName)) return;
        e.preventDefault();
        const id = e.dataTransfer.getData(DRAG_TYPE);
        setDragSource(null);
        if (id) moveMeeting(id, projectName);
      },
    } : {};

    return (
      <div
        key={item.id}
        {...dropProps}
        className={isProject && dropTarget === projectName ? 'rounded-md bg-accent ring-1 ring-primary/40' : undefined}
      >
        {isMeetingItem ? (
          <ContextMenu onOpenChange={loadProjects}>
            <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
            <ContextMenuContent>
              <ContextMenuItem onSelect={() => handleEditStart(item.id, item.title)}>Rename</ContextMenuItem>
              <ContextMenuSub>
                <ContextMenuSubTrigger disabled={item.hookStatus === 'running'}>Move to</ContextMenuSubTrigger>
                <ContextMenuSubContent>
                  {projects.map((p) => (
                    <ContextMenuItem key={p} disabled={p === currentProject} onSelect={() => moveMeeting(item.id, p)}>
                      <Check className={`w-3.5 h-3.5 ${p === currentProject ? '' : 'invisible'}`} />
                      {p === INBOX ? 'Inbox' : p}
                    </ContextMenuItem>
                  ))}
                  <ContextMenuSeparator />
                  <ContextMenuItem onSelect={() => setNewProject({ meetingId: item.id, draft: '' })}>
                    <FolderPlus className="w-3.5 h-3.5" />
                    New project...
                  </ContextMenuItem>
                </ContextMenuSubContent>
              </ContextMenuSub>
              <ContextMenuItem disabled={item.hookStatus === 'running'} onSelect={() => runMeetingHook(item.id)}>Run hook</ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuItem destructive onSelect={() => setDeleteModalState({ isOpen: true, itemId: item.id })}>Delete</ContextMenuItem>
            </ContextMenuContent>
          </ContextMenu>
        ) : row}
        {item.type === 'folder' && isExpanded && item.children && (
          <div className="space-y-px">
            {item.children.map(child => renderItem(child, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="fixed top-0 left-0 h-screen z-40">
      <div
        className={`h-screen bg-card border-r border-border flex flex-col transition-all duration-300 ${isCollapsed ? 'w-16' : 'w-[220px]'
          }`}
      >
        {renderCollapsedIcons()}

        {!isCollapsed && (
          <div className="flex flex-1 min-h-0 flex-col px-2 py-2.5">
            {/* Wordmark routes home; the separate Home entry is gone */}
            <div className="flex items-center px-1 pb-2">
              <button onClick={() => router.push('/')} className="rounded-md px-1 text-sm font-semibold text-foreground">
                Noesis
              </button>
            </div>

            <div className="px-1 pb-1">
              <InputGroup className="h-8 bg-background shadow-none">
                <InputGroupInput
                  placeholder="Search transcripts"
                  className="text-[13px]"
                  value={searchQuery}
                  onChange={(e) => handleSearchChange(e.target.value)}
                />
                <InputGroupAddon>
                  <SearchIcon />
                </InputGroupAddon>
                {searchQuery &&
                  <InputGroupAddon align={'inline-end'}>
                    <InputGroupButton onClick={() => handleSearchChange('')} aria-label="Clear search">
                      <X />
                    </InputGroupButton>
                  </InputGroupAddon>
                }
              </InputGroup>
            </div>

            <div className="flex items-center gap-2 px-2 pt-2.5 pb-1 text-[11px] uppercase tracking-wide text-muted-foreground">
              <span>Projects</span>
              {searchQuery && isSearching && (
                <span className="normal-case tracking-normal text-primary animate-pulse">Searching...</span>
              )}
            </div>

            {/* Scrollable project tree */}
            <div className="flex-1 min-h-0 overflow-y-auto custom-scrollbar space-y-px">
              {filteredSidebarItems
                .filter(item => item.type === 'folder' && expandedFolders.has(item.id) && item.children)
                .map(item => (
                  <React.Fragment key={`${item.id}-children`}>
                    {item.children!.map(child => renderItem(child, 1))}
                  </React.Fragment>
                ))}
            </div>

            <div className="flex-shrink-0 space-y-0.5 pt-2">
              <button
                onClick={handleRecordingToggle}
                disabled={isRecording}
                className="flex h-8 w-full items-center justify-center gap-1.5 rounded-md bg-foreground px-3 text-[13px] font-medium text-background transition-opacity hover:opacity-90 disabled:opacity-50"
              >
                <span className={`h-[7px] w-[7px] rounded-full bg-recording ${isRecording ? 'animate-pulse' : ''}`} />
                {isRecording ? 'Recording in progress...' : 'New recording'}
              </button>

              <button
                onClick={() => router.push('/settings')}
                className={`flex w-full items-center gap-2 rounded-md px-2 py-1 text-sm transition-colors hover:bg-accent/60 hover:text-foreground ${pathname === '/settings' ? 'bg-accent text-foreground' : 'text-muted-foreground'}`}
              >
                <Settings className="w-3.5 h-3.5" />
                Settings
              </button>

              {/* h-8 matches the rail's p-2 + 16px icon, so the toggle keeps its vertical spot */}
              <button
                onClick={toggleCollapse}
                aria-label="Collapse sidebar"
                className="flex h-8 w-full items-center gap-2 rounded-md px-2 text-sm text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground"
              >
                <PanelLeftClose className="w-3.5 h-3.5" />
                Collapse
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Confirmation Modal for Delete */}
      <ConfirmationModal
        isOpen={deleteModalState.isOpen}
        text="Delete this meeting? Its folder will be moved to the Trash."
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteModalState({ isOpen: false, itemId: null })}
      />

      {/* Edit Meeting Title Modal */}
      <Dialog open={editModalState.isOpen} onOpenChange={(open) => {
        if (!open) handleEditCancel();
      }}>
        <DialogContent className="sm:max-w-[425px]">
          <VisuallyHidden>
            <DialogTitle>Edit Meeting Title</DialogTitle>
          </VisuallyHidden>
          <div className="py-4">
            <h3 className="text-lg font-semibold mb-4">Edit Meeting Title</h3>
            <div className="space-y-4">
              <div>
                <label htmlFor="meeting-title" className="block text-sm font-medium text-foreground mb-2">
                  Meeting Title
                </label>
                <input
                  id="meeting-title"
                  type="text"
                  value={editingTitle}
                  onChange={(e) => setEditingTitle(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      handleEditConfirm();
                    } else if (e.key === 'Escape') {
                      handleEditCancel();
                    }
                  }}
                  className="w-full px-3 py-2 border border-border rounded-md focus:outline-none focus:ring-2 focus:ring-ring focus:border-transparent"
                  placeholder="Enter meeting title"
                  autoFocus
                />
              </div>
            </div>
          </div>
          <DialogFooter>
            <button
              onClick={handleEditCancel}
              className="px-4 py-2 text-sm font-medium text-foreground bg-muted hover:bg-accent rounded-md transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleEditConfirm}
              className="px-4 py-2 text-sm font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition-colors"
            >
              Save
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={!!newProject} onOpenChange={(open) => { if (!open) setNewProject(null); }}>
        <DialogContent className="sm:max-w-[425px]">
          <VisuallyHidden>
            <DialogTitle>New project</DialogTitle>
          </VisuallyHidden>
          <div className="py-4">
            <h3 className="text-lg font-semibold mb-4">New project</h3>
            <label htmlFor="new-project-name" className="block text-sm font-medium text-foreground mb-2">
              Project name
            </label>
            <input
              id="new-project-name"
              type="text"
              value={newProject?.draft ?? ''}
              onChange={(e) => setNewProject((n) => (n ? { ...n, draft: e.target.value } : n))}
              onKeyDown={(e) => { if (e.key === 'Enter') confirmNewProject(); }}
              className="w-full px-3 py-2 border border-border rounded-md focus:outline-none focus:ring-2 focus:ring-ring focus:border-transparent"
              placeholder="project name"
              autoFocus
            />
          </div>
          <DialogFooter>
            <button
              onClick={() => setNewProject(null)}
              className="px-4 py-2 text-sm font-medium text-foreground bg-muted hover:bg-accent rounded-md transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={confirmNewProject}
              disabled={!normalizeProjectName(newProject?.draft ?? '')}
              className="px-4 py-2 text-sm font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition-colors disabled:opacity-50"
            >
              Move
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default Sidebar;
