import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Plus, Tag, X } from 'lucide-react';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { MEETING_EXTRAS_CHANGED } from '@/components/Sidebar/SidebarProvider';

const INLINE = 3;

/** One line under the meeting title: the first tags and a +N count; a click opens the editor. */
export function MeetingTags({ meetingId, tags }: { meetingId: string; tags: string[] }) {
  const rest = tags.length - INLINE;
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          title={tags.length ? `${tags.join(', ')}\nClick to edit` : undefined}
          className="mt-1 flex max-w-full items-center gap-1 overflow-hidden whitespace-nowrap rounded px-1 -mx-1 text-[11px] text-muted-foreground hover:bg-accent/60 hover:text-foreground"
        >
          <Tag className="h-3 w-3 flex-none" />
          {tags.length ? tags.slice(0, INLINE).map((t) => (
            <span key={t} className="max-w-[9rem] truncate rounded border border-border px-1.5">{t}</span>
          )) : 'Add tags'}
          {rest > 0 && <span className="flex-none">+{rest}</span>}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80 p-3">
        <TagEditor meetingId={meetingId} tags={tags} />
      </PopoverContent>
    </Popover>
  );
}

/** Click a tag to rename it, x deletes it, + adds one. Writes metadata.json. */
function TagEditor({ meetingId, tags }: { meetingId: string; tags: string[] }) {
  // index being edited; tags.length means the new-tag input
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState('');

  const save = async (next: string[]) => {
    setEditing(null);
    try {
      await invoke<string[]>('set_meeting_tags', { meetingId, tags: next });
      window.dispatchEvent(new Event(MEETING_EXTRAS_CHANGED));
    } catch (e) {
      toast.error('Could not save the tags', { description: String(e) });
    }
  };

  const commit = () => {
    if (editing === null) return;
    const value = draft.trim();
    if (editing === tags.length) {
      if (value) save([...tags, value]); else setEditing(null);
    } else if (value !== tags[editing]) {
      save(value ? tags.map((t, i) => (i === editing ? value : t)) : tags.filter((_, i) => i !== editing));
    } else {
      setEditing(null);
    }
  };

  const start = (i: number) => { setDraft(i < tags.length ? tags[i] : ''); setEditing(i); };

  const input = (
    <input
      autoFocus
      aria-label="Tag"
      className="h-6 w-28 rounded border border-input bg-transparent px-1 text-[11px] outline-none focus:ring-1 focus:ring-ring"
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
        if (e.key === 'Escape') setEditing(null);
      }}
    />
  );

  return (
    <div className="flex flex-wrap items-center gap-1 text-xs">
      {tags.map((tag, i) =>
        editing === i ? <span key={tag}>{input}</span> : (
          <span key={tag} className="group flex items-center rounded border border-border text-muted-foreground">
            <button type="button" title="Click to edit" className="px-1.5 py-0.5 hover:text-foreground" onClick={() => start(i)}>{tag}</button>
            <button type="button" aria-label={`Delete tag ${tag}`} className="pr-1 hover:text-destructive" onClick={() => save(tags.filter((_, j) => j !== i))}>
              <X className="h-3.5 w-3.5" />
            </button>
          </span>
        ),
      )}
      {editing === tags.length ? input : (
        <button type="button" onClick={() => start(tags.length)} className="flex items-center gap-0.5 rounded px-1 text-muted-foreground hover:bg-accent/60 hover:text-foreground">
          <Plus className="h-3 w-3" />
          Add tag
        </button>
      )}
    </div>
  );
}
