'use client';

import { ArrowLeft, ArrowRight, SearchIcon, X } from 'lucide-react';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group';

/** Full-width macOS title strip (overlay style): traffic lights, back and forward, and transcript search in the middle. Drags the window. */
export function TitleBar() {
  const { searchQuery, setSearchQuery, isSearching, isCollapsed, toggleCollapse } = useSidebar();
  const navButton = 'flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent/60 hover:text-foreground';

  const onSearch = (value: string) => {
    setSearchQuery(value);
    // results show in the sidebar tree, so open it
    if (value.trim() && isCollapsed) toggleCollapse();
  };

  return (
    <div
      data-tauri-drag-region
      className="fixed inset-x-0 top-0 z-50 h-10 bg-card"
      // a click on the drag region starts a window drag and never moves focus, so drop it from the search here
      onMouseDown={(e) => { if (e.target === e.currentTarget) (document.activeElement as HTMLElement | null)?.blur(); }}
    >
      <div className="absolute left-1/2 top-1/2 flex -translate-x-1/2 -translate-y-1/2 items-center gap-1">
        <button type="button" aria-label="Back" title="Back" className={navButton} onClick={() => window.history.back()}>
          <ArrowLeft className="h-4 w-4" />
        </button>
        <button type="button" aria-label="Forward" title="Forward" className={navButton} onClick={() => window.history.forward()}>
          <ArrowRight className="h-4 w-4" />
        </button>
  
        <InputGroup className="ml-1 h-7 w-[min(440px,40vw)] bg-background shadow-none">
          <InputGroupAddon>
            <SearchIcon className={isSearching ? 'animate-pulse' : ''} />
          </InputGroupAddon>
          <InputGroupInput
            placeholder="Search transcripts"
            className="text-[13px]"
            value={searchQuery}
            onChange={(e) => onSearch(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Escape') onSearch(''); }}
          />
          {searchQuery && (
            <InputGroupAddon align="inline-end">
            <InputGroupButton onClick={() => onSearch('')} aria-label="Clear search">
              <X />
            </InputGroupButton>
          </InputGroupAddon>
        )}
      </InputGroup>
      </div>
    </div>
  );
}
