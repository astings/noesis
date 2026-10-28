'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { Activity, Check, Circle, Loader2, Minus, RotateCw, X } from 'lucide-react';
import { toast } from 'sonner';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { isTerminal, useJobs, type Job, type JobState } from '@/hooks/useJobs';

const KIND = { enhance: 'Enhance', summary: 'Summary', download: 'Download' } as const;
const ordinal = (n: number) => `${n}${['th', 'st', 'nd', 'rd'][n % 100 > 10 && n % 100 < 14 ? 0 : n % 10] ?? 'th'}`;
const eta = (s: number) => (s < 60 ? '<1 min left' : `~${Math.round(s / 60)} min left`);
const clock = (ms: number) => {
  const t = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
};
const minutes = (j: Job) =>
  j.started_at && j.finished_at ? `${Math.max(1, Math.round((Date.parse(j.finished_at) - Date.parse(j.started_at)) / 60000))} min` : '';

function StepIcon({ state }: { state: JobState }) {
  if (state === 'done') return <Check className="h-3 w-3 text-primary" aria-hidden />;
  if (state === 'running') return <Loader2 className="h-3 w-3 animate-spin" aria-hidden />;
  if (state === 'failed') return <X className="h-3 w-3 text-destructive" aria-hidden />;
  if (state === 'cancelled') return <Minus className="h-3 w-3" aria-hidden />;
  return <Circle className="h-3 w-3" aria-hidden />;
}

/** Title-bar button and popover listing background jobs (spec 4). */
export function JobsButton() {
  const jobs = useJobs();
  const { meetings } = useSidebar();
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [seenAt, setSeenAt] = useState(() => Date.now());
  const [now, setNow] = useState(Date.now());

  const running = jobs.filter((j) => j.state === 'running');
  const queued = jobs.filter((j) => j.state === 'queued');
  const recent = jobs.filter((j) => isTerminal(j.state)).sort((a, b) => (b.finished_at ?? '').localeCompare(a.finished_at ?? ''));
  const active = running.length + queued.length;
  const unseenFailure = recent.some((j) => j.state === 'failed' && Date.parse(j.finished_at ?? '') > seenAt);

  // elapsed clocks tick only while the panel shows a running job
  useEffect(() => {
    if (!open || running.length === 0) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [open, running.length]);

  const failed = recent.filter((j) => j.state === 'failed').length;
  const summary = [running.length && `${running.length} running`, queued.length && `${queued.length} queued`, failed && `${failed} failed`].filter(Boolean).join(', ');

  const name = (j: Job) => (j.meeting_id ? meetings.find((m) => m.id === j.meeting_id)?.title || j.title || 'Meeting' : j.title);
  const call = (cmd: string, id?: string) => invoke(cmd, id ? { id } : {}).catch((e) => toast.error(String(e)));
  const openMeeting = (j: Job) => {
    if (!j.meeting_id) return;
    setOpen(false);
    router.push(`/meeting-details?id=${j.meeting_id}`);
  };

  const title = (j: Job) =>
    j.meeting_id ? (
      <button type="button" onClick={() => openMeeting(j)} className="min-w-0 truncate text-left font-medium hover:underline">{name(j)}</button>
    ) : (
      <span className="min-w-0 truncate font-medium">{name(j)}</span>
    );

  const cancel = (j: Job) => (
    <button type="button" aria-label={`Cancel ${KIND[j.kind]} of ${name(j)}`} title="Cancel" onClick={() => call('cancel_job', j.id)}
      className="flex h-6 w-6 flex-none items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground">
      <X className="h-3.5 w-3.5" />
    </button>
  );

  const status = (j: Job) => {
    if (j.progress === null) return `${clock(now - Date.parse(j.started_at ?? new Date().toISOString()))} elapsed`;
    return `${j.progress}% · ${j.eta_secs === null ? 'Estimating…' : eta(j.eta_secs)}`;
  };

  return (
    <Popover open={open} onOpenChange={(o) => { setOpen(o); setSeenAt(Date.now()); }}>
      <PopoverTrigger asChild>
        <button type="button" aria-label={active ? `Background jobs, ${active} active` : 'Background jobs'} title="Background jobs"
          className="relative flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent/60 hover:text-foreground">
          {running.length ? <Loader2 className="h-4 w-4 animate-spin text-foreground" /> : <Activity className="h-4 w-4" />}
          {active > 0 && (
            <span className="absolute -right-0.5 -top-0.5 min-w-[14px] rounded-full bg-primary px-1 text-center text-[10px] leading-[14px] text-primary-foreground">{active}</span>
          )}
          {unseenFailure && <span className="absolute -bottom-0.5 -right-0.5 h-2 w-2 rounded-full bg-destructive" aria-hidden />}
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[360px] p-0" role="dialog" aria-label="Background jobs">
        <div className="max-h-[70vh] overflow-y-auto p-2 text-[13px]">
          <p className="sr-only" aria-live="polite">{summary}</p>
          {active === 0 && <p className="px-2 py-1.5 text-xs text-muted-foreground">Nothing running</p>}

          {running.length > 0 && <h3 className="px-2 pb-1 pt-1 text-[11px] font-medium uppercase text-muted-foreground">Running</h3>}
          <ul>
            {running.map((j) => (
              <li key={j.id} className="mb-1 rounded-md border border-border p-2">
                <div className="flex items-center gap-1.5">
                  {title(j)}
                  <span className="flex-none text-muted-foreground">— {KIND[j.kind]}</span>
                  <span className="flex-1" />
                  {cancel(j)}
                </div>
                {j.steps.length > 0 && (
                  <div className="mt-1 flex flex-wrap gap-x-3 text-xs text-muted-foreground">
                    {j.steps.map((s) => (
                      <span key={s.name} className="flex items-center gap-1"><StepIcon state={s.state} />{s.name}<span className="sr-only"> {s.state}</span></span>
                    ))}
                  </div>
                )}
                <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-muted" role="progressbar" aria-label={`${KIND[j.kind]} progress`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={j.progress ?? undefined}>
                  {j.progress === null
                    ? <div className="h-full w-1/3 animate-pulse rounded-full bg-primary/60" />
                    : <div className="h-full rounded-full bg-primary transition-[width]" style={{ width: `${j.progress}%` }} />}
                </div>
                <div className="mt-1 flex gap-2 text-xs text-muted-foreground">
                  <span className="tabular-nums">{status(j)}</span>
                  {j.detail && <span className="min-w-0 truncate">{j.detail}</span>}
                </div>
              </li>
            ))}
          </ul>

          {queued.length > 0 && <h3 className="px-2 pb-1 pt-2 text-[11px] font-medium uppercase text-muted-foreground">Queued</h3>}
          <ul>
            {queued.map((j, i) => (
              <li key={j.id} className="flex items-center gap-1.5 rounded px-2 py-1">
                {title(j)}
                <span className="flex-none text-muted-foreground">— {KIND[j.kind]} · {ordinal(i + 1)} in queue</span>
                <span className="flex-1" />
                {cancel(j)}
              </li>
            ))}
          </ul>

          {recent.length > 0 && (
            <div className="flex items-center px-2 pb-1 pt-2">
              <h3 className="flex-1 text-[11px] font-medium uppercase text-muted-foreground">Recent</h3>
              <button type="button" onClick={() => call('clear_recent_jobs')} className="text-xs text-muted-foreground hover:text-foreground">Clear</button>
            </div>
          )}
          <ul>
            {recent.map((j) => (
              <li key={j.id} className="flex items-center gap-1.5 rounded px-2 py-1 text-xs">
                <StepIcon state={j.state} />
                {title(j)}
                <span className="min-w-0 flex-1 truncate text-muted-foreground" title={j.error ?? undefined}>
                  — {KIND[j.kind]} · {j.state === 'done' ? minutes(j) : j.state}{j.error ? `: ${j.error}` : ''}
                  {j.steps.filter((s) => s.state === 'failed').map((s) => ` · ${s.name} failed`).join('')}
                </span>
                {j.state === 'failed' && j.kind !== 'summary' && (
                  <button type="button" onClick={() => call('retry_job', j.id)} aria-label={`Retry ${KIND[j.kind]} of ${name(j)}`}
                    className="flex flex-none items-center gap-1 rounded px-1.5 py-0.5 hover:bg-accent"><RotateCw className="h-3 w-3" />Retry</button>
                )}
              </li>
            ))}
          </ul>
        </div>
      </PopoverContent>
    </Popover>
  );
}
