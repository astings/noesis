'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { ArrowLeft, ArrowRight, SearchIcon, X } from 'lucide-react';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group';

const formatTime = (s: number) => {
  const t = Math.floor(s);
  const mm = String(Math.floor((t % 3600) / 60)).padStart(2, '0');
  const ss = String(t % 60).padStart(2, '0');
  return t >= 3600 ? `${Math.floor(t / 3600)}:${mm}:${ss}` : `${mm}:${ss}`;
};

// the FTS snippet wraps matched terms in ** (audio/search_index.rs)
const Snippet = ({ text }: { text: string }) => (
  <>{text.split('**').map((part, i) => (i % 2 ? <mark key={i} className="bg-transparent font-semibold text-foreground">{part}</mark> : part))}</>
);

/** Full-width macOS title strip (overlay style): traffic lights, back and forward, and transcript search in the middle. Drags the window. */
export function TitleBar() {
  const { searchQuery, setSearchQuery, searchTranscripts, searchResults, isSearching } = useSidebar();
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  // one row per hit, grouped under its meeting; the arrow keys walk this flat list
  const rows = searchResults.flatMap((r) => r.hits.map((h) => ({ id: r.id, ...h })));
  const navButton = 'flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent/60 hover:text-foreground';

  useEffect(() => {
    if (searchQuery.trim()) searchTranscripts(searchQuery);
    setActive(0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [searchQuery]);

  const onSearch = (value: string) => {
    setSearchQuery(value);
    setOpen(true);
  };

  const go = (i: number) => {
    const r = rows[i];
    if (!r) return;
    setOpen(false);
    // a hit is a block of segments (t..e); q lets the transcript find the segment holding the words.
    // n makes a repeat click on the same hit a new URL, so the transcript scrolls and flashes again
    const q = encodeURIComponent(searchQuery.trim());
    router.push(`/meeting-details?id=${r.id}&t=${r.start_seconds}&e=${r.end_seconds}&q=${q}&n=${Date.now()}`);
  };

  const showResults = open && searchQuery.trim() !== '';

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

        <div className="relative ml-1">
          <InputGroup className="h-7 w-[min(440px,40vw)] bg-background shadow-none">
            <InputGroupAddon>
              <SearchIcon className={isSearching ? 'animate-pulse' : ''} />
            </InputGroupAddon>
            <InputGroupInput
              placeholder="Search transcripts"
              className="text-[13px]"
              value={searchQuery}
              role="combobox"
              aria-expanded={showResults}
              aria-controls="search-results"
              onChange={(e) => onSearch(e.target.value)}
              onFocus={() => setOpen(true)}
              onBlur={() => setOpen(false)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') { onSearch(''); setOpen(false); }
                else if (e.key === 'ArrowDown') { e.preventDefault(); setOpen(true); setActive((a) => Math.min(a + 1, rows.length - 1)); }
                else if (e.key === 'ArrowUp') { e.preventDefault(); setActive((a) => Math.max(a - 1, 0)); }
                else if (e.key === 'Enter') go(active);
              }}
            />
            {searchQuery && (
              <InputGroupAddon align="inline-end">
                <InputGroupButton onClick={() => onSearch('')} aria-label="Clear search">
                  <X />
                </InputGroupButton>
              </InputGroupAddon>
            )}
          </InputGroup>

          {showResults && (
            <div
              id="search-results"
              role="listbox"
              className="absolute left-0 right-0 top-full mt-1 max-h-[60vh] overflow-y-auto rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-lg"
              // keep focus in the input so blur does not close the list before the click lands
              onMouseDown={(e) => e.preventDefault()}
            >
              {rows.length === 0 ? (
                <div className="px-2 py-1.5 text-xs text-muted-foreground">{isSearching ? 'Searching…' : 'No results'}</div>
              ) : (
                searchResults.map((r) => (
                  <div key={r.id} className="py-0.5">
                    <div className="truncate px-2 pt-1 text-[13px] font-medium">{r.title}</div>
                    {r.hits.map((h) => {
                      const i = rows.findIndex((row) => row.id === r.id && row.start_seconds === h.start_seconds);
                      return (
                        <button
                          key={h.start_seconds}
                          type="button"
                          role="option"
                          aria-selected={i === active}
                          className={`flex w-full items-baseline gap-2 rounded px-2 py-1 text-left text-xs ${i === active ? 'bg-accent' : 'hover:bg-accent/60'}`}
                          onMouseEnter={() => setActive(i)}
                          onClick={() => go(i)}
                        >
                          <span className="flex-none font-mono text-[11px] tabular-nums text-muted-foreground">{formatTime(h.start_seconds)}</span>
                          <span className="line-clamp-2 text-muted-foreground"><Snippet text={h.snippet} /></span>
                        </button>
                      );
                    })}
                  </div>
                ))
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
