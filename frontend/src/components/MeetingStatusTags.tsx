import { Info } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { enhancedLabel, liveLabel, type TranscriptionInfo } from '@/lib/languages';

const TAGGING: Record<string, string> = { done: 'Done', running: 'Running', failed: 'Failed', cancelled: 'Cancelled' };

/** Info icon on the meeting page; hover shows the live and enhance engines and the statuses. */
export function MeetingInfoButton({
  enhanceStatus,
  taggingStatus,
  transcription,
}: {
  enhanceStatus?: string;
  taggingStatus?: string;
  transcription?: TranscriptionInfo | null;
}) {
  const statuses = [enhanceStatus, taggingStatus];
  // the icon still signals work in progress and failures without opening the hover
  const tone = statuses.includes('failed') ? 'text-destructive' : statuses.includes('running') ? 'text-primary animate-pulse' : 'text-muted-foreground';
  const rows: [string, string][] = [
    ['Live', liveLabel(transcription)],
    ['Enhanced', enhancedLabel(enhanceStatus, transcription)],
    ['Title and tags', (taggingStatus && TAGGING[taggingStatus]) || 'Not run'],
  ];
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button type="button" aria-label="Meeting info" className={`flex h-8 w-8 items-center justify-center rounded-md hover:bg-accent/60 ${tone}`}>
          <Info className="h-4 w-4" />
        </button>
      </TooltipTrigger>
      <TooltipContent side="bottom" align="end" className="w-72 space-y-2 border border-border bg-popover p-3 text-popover-foreground">
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
          {rows.map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-muted-foreground">{k}</dt>
              <dd className={`break-words ${v.startsWith('Failed') ? 'text-destructive' : ''}`}>{v}</dd>
            </div>
          ))}
        </dl>
      </TooltipContent>
    </Tooltip>
  );
}
