'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { ArrowLeft, ArrowRight, SearchIcon, Tag, X } from 'lucide-react';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { JobsButton } from '@/components/Jobs/JobsButton';
import { RecoveryButton } from '@/components/TranscriptRecovery';
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
  // per meeting: its header (opens the meeting), then one row per transcript hit; the arrow keys walk this flat list.
  // blocks returned only because the title or a tag matched are not content hits, so they get no row
  const groups = searchResults.map((r) => ({ ...r, contentHits: r.hits.filter((h) => h.text_match) }));
  const rows = groups.flatMap((r) => [
    { id: r.id, hit: undefined as (typeof r.hits)[number] | undefined },
    ...r.contentHits.map((hit) => ({ id: r.id, hit })),
  ]);
  const option = (i: number) => ({
    type: 'button' as const,
    role: 'option',
    'aria-selected': i === active,
    onMouseEnter: () => setActive(i),
    onClick: () => go(i),
  });
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
    if (!r.hit) { router.push(`/meeting-details?id=${r.id}`); return; }
    // a hit is a block of segments (t..e); q lets the transcript find the segment holding the words.
    // n makes a repeat click on the same hit a new URL, so the transcript scrolls and flashes again
    const q = encodeURIComponent(searchQuery.trim());
    router.push(`/meeting-details?id=${r.id}&t=${r.hit.start_seconds}&e=${r.hit.end_seconds}&q=${q}&n=${Date.now()}`);
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
                groups.map((r) => {
                  const head = rows.findIndex((row) => row.id === r.id && !row.hit);
                  return (
                    <div key={r.id} className="py-0.5">
                      <button {...option(head)} className={`flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-[13px] ${head === active ? 'bg-accent' : 'hover:bg-accent/60'}`}>
                        <span className={`min-w-0 flex-1 truncate font-medium ${r.title_match ? 'text-primary' : ''}`} title={r.title_match ? 'Title matches' : undefined}>{r.title}</span>
                        {/* tag hit: pill with a tag icon, unlike content hits (time + snippet rows) */}
                        {r.matched_tags.map((tag) => (
                          <span key={tag} title="Tag matches" className="flex flex-none items-center gap-1 rounded-full border border-warning/40 bg-warning/15 px-1.5 py-px text-[11px] text-foreground">
                            <Tag className="h-3 w-3 text-warning" />{tag}
                          </span>
                        ))}
                      </button>
                      {r.contentHits.map((h) => {
                        const i = rows.findIndex((row) => row.hit === h);
                        return (
                          <button key={h.start_seconds} {...option(i)} className={`flex w-full items-baseline gap-2 rounded py-1 pl-4 pr-2 text-left text-xs ${i === active ? 'bg-accent' : 'hover:bg-accent/60'}`}>
                            <span className="flex-none font-mono text-[11px] tabular-nums text-muted-foreground">{formatTime(h.start_seconds)}</span>
                            <span className="line-clamp-2 text-muted-foreground"><Snippet text={h.snippet} /></span>
                          </button>
                        );
                      })}
                    </div>
                  );
                })
              )}
            </div>
          )}
        </div>
      </div>

      <div className="absolute right-3 top-1/2 flex -translate-y-1/2 items-center gap-1">
        <RecoveryButton />
        <JobsButton />
      </div>
    </div>
  );
}
