import { Info } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

const ENHANCE: Record<string, string> = { done: 'Done', queued: 'Queued', running: 'Running', failed: 'Failed' };
const HOOK: Record<string, string> = { done: 'Ran', running: 'Running', failed: 'Failed' };
const TAGGING: Record<string, string> = { done: 'Done', running: 'Running', failed: 'Failed' };

/** Info icon on the meeting page; hover shows tags and the enhance, title/tags and hook statuses. */
export function MeetingInfoButton({ tags, enhanceStatus, hookStatus, taggingStatus }: { tags: string[]; enhanceStatus?: string; hookStatus?: string; taggingStatus?: string }) {
  const statuses = [enhanceStatus, hookStatus, taggingStatus];
  // the icon still signals work in progress and failures without opening the hover
  const tone = statuses.includes('failed') ? 'text-destructive' : statuses.includes('running') ? 'text-primary animate-pulse' : 'text-muted-foreground';
  const rows: [string, string][] = [
    ['Enhanced', (enhanceStatus && ENHANCE[enhanceStatus]) || 'Not run'],
    ['Title and tags', (taggingStatus && TAGGING[taggingStatus]) || 'Not run'],
    ['Hook', (hookStatus && HOOK[hookStatus]) || 'Not run'],
  ];
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button type="button" aria-label="Meeting info" className={`flex h-8 w-8 items-center justify-center rounded-md hover:bg-accent/60 ${tone}`}>
          <Info className="h-4 w-4" />
        </button>
      </TooltipTrigger>
      <TooltipContent side="bottom" align="end" className="w-56 space-y-2 border border-border bg-popover p-3 text-popover-foreground">
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
          {rows.map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-muted-foreground">{k}</dt>
              <dd className={v === 'Failed' ? 'text-destructive' : ''}>{v}</dd>
            </div>
          ))}
        </dl>
        <div className="flex flex-wrap gap-1 border-t border-border pt-2">
          {tags.length ? tags.map((tag) => <span key={tag} className="rounded border border-border px-1.5">{tag}</span>) : <span className="text-muted-foreground">No tags</span>}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}
