import { Sparkles, Tags, Webhook } from 'lucide-react';

const ENHANCE: Record<string, { label: string; className: string }> = {
  done: { label: 'Enhanced', className: 'text-muted-foreground' },
  queued: { label: 'Enhance queued', className: 'text-muted-foreground/50' },
  running: { label: 'Enhancing transcript', className: 'text-primary animate-pulse' },
  failed: { label: 'Enhance failed', className: 'text-destructive' },
};

const HOOK: Record<string, { label: string; className: string }> = {
  done: { label: 'Hook ran', className: 'text-muted-foreground' },
  running: { label: 'Hook running', className: 'text-primary animate-pulse' },
  failed: { label: 'Hook failed', className: 'text-destructive' },
};

const TAGGING: Record<string, { label: string; className: string }> = {
  done: { label: 'Tagged', className: 'text-muted-foreground' },
  running: { label: 'Tagging', className: 'text-primary animate-pulse' },
  failed: { label: 'Tagging failed', className: 'text-destructive' },
};

// Three small status icons: enhance (Sparkles) hook (Webhook) and tagging (Tags). A missing status renders nothing for that icon.
export function MeetingStatusTags({ enhanceStatus, hookStatus, taggingStatus }: { enhanceStatus?: string; hookStatus?: string; taggingStatus?: string }) {
  const enhance = enhanceStatus ? ENHANCE[enhanceStatus] : undefined;
  const hook = hookStatus ? HOOK[hookStatus] : undefined;
  const tagging = taggingStatus ? TAGGING[taggingStatus] : undefined;
  if (!enhance && !hook && !tagging) return null;
  return (
    <span className="ml-2 inline-flex flex-shrink-0 items-center gap-1">
      {enhance && <Sparkles role="img" aria-label={enhance.label} className={`h-3 w-3 ${enhance.className}`}><title>{enhance.label}</title></Sparkles>}
      {hook && <Webhook role="img" aria-label={hook.label} className={`h-3 w-3 ${hook.className}`}><title>{hook.label}</title></Webhook>}
      {tagging && <Tags role="img" aria-label={tagging.label} className={`h-3 w-3 ${tagging.className}`}><title>{tagging.label}</title></Tags>}
    </span>
  );
}
