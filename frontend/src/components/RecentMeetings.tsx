'use client';

import { useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { useSidebar, type CurrentMeeting } from '@/components/Sidebar/SidebarProvider';
import { bucketOf, parseDate, rowLabel } from '@/lib/dateBuckets';
import { formatDuration } from '@/lib/duration';
import { layoutDay } from '@/lib/agenda';
import { INBOX } from '@/lib/projects';

type View = 'list' | 'week';
const VIEW_KEY = 'noesis.homeView';
const projectLabel = (m: CurrentMeeting) => (!m.project || m.project === INBOX ? 'Inbox' : m.project);

// Idle home: every meeting as a week agenda (default) or a newest-first list
export function RecentMeetings() {
  const { meetings } = useSidebar();
  const [view, setView] = useState<View>('week');
  const [showWeekends, setShowWeekends] = useState(false);
  useEffect(() => {
    invoke<{ agenda_show_weekends?: boolean }>('get_recording_preferences')
      .then((p) => setShowWeekends(!!p.agenda_show_weekends))
      .catch(() => {});
  }, []);
  useEffect(() => {
    try { if (localStorage.getItem(VIEW_KEY) === 'list') setView('list'); } catch {}
  }, []);
  const pick = (v: View) => {
    setView(v);
    try { localStorage.setItem(VIEW_KEY, v); } catch {}
  };

  const dated = useMemo(
    () => meetings
      .map((m) => ({ m, date: parseDate(m.startedAt) }))
      .sort((a, b) => (b.date?.getTime() ?? 0) - (a.date?.getTime() ?? 0)),
    [meetings],
  );

  const tab = (v: View, label: string) => (
    <button
      type="button"
      onClick={() => pick(v)}
      className={`rounded px-2 py-0.5 ${view === v ? 'bg-accent text-foreground' : 'hover:text-foreground'}`}
    >
      {label}
    </button>
  );

  return (
    <div className="px-6 py-5">
      <div className="mb-2 flex items-center gap-2">
        <h2 className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">Recent</h2>
        <div className="ml-auto flex text-xs text-muted-foreground">
          {tab('list', 'List')}
          {tab('week', 'Week')}
        </div>
      </div>
      {view === 'list' ? <MeetingList items={dated} /> : <WeekAgenda items={dated} showWeekends={showWeekends} />}
    </div>
  );
}

function MeetingList({ items }: { items: { m: CurrentMeeting; date: Date | null }[] }) {
  const router = useRouter();
  const now = new Date();
  return (
    <ul>
      {items.map(({ m, date }) => {
        const bucket = bucketOf(date, now);
        const when = bucket.kind === 'day' ? `${bucket.label} ${rowLabel(date, bucket)}` : rowLabel(date, bucket);
        const length = formatDuration(m.durationSeconds);
        return (
          <li key={m.id}>
            <button
              type="button"
              onClick={() => router.push(`/meeting-details?id=${m.id}`)}
              className="flex w-full items-baseline gap-2 rounded px-2 py-1.5 -mx-2 text-left text-sm hover:bg-accent/60"
            >
              <span className="truncate font-medium">{m.title}</span>
              <span className="flex-none rounded bg-muted px-1.5 text-[10px] text-muted-foreground">{projectLabel(m)}</span>
              <span className="ml-auto flex-none text-xs text-muted-foreground">
                {[when, length].filter(Boolean).join(' · ')}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

const HOUR_PX = 48;
const dayFmt = new Intl.DateTimeFormat('en-US', { weekday: 'short', day: 'numeric' });
const rangeFmt = new Intl.DateTimeFormat('en-US', { month: 'short', day: 'numeric' });
const timeFmt = new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' });
// "9 AM" or "09" depending on the system clock, short enough for the label column
const hourFmt = new Intl.DateTimeFormat(undefined, { hour: 'numeric' });
// Below this height a block shows the title only
const TWO_LINE_PX = 34;
const CARD_W = 256;

interface Hover { m: CurrentMeeting; startMin: number; endMin: number; rect: DOMRect }

const clock = (min: number) => timeFmt.format(new Date(2000, 0, 1, Math.floor(min / 60), min % 60));

function mondayOf(d: Date) {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() - ((d.getDay() + 6) % 7));
}

function WeekAgenda({ items, showWeekends }: { items: { m: CurrentMeeting; date: Date | null }[]; showWeekends: boolean }) {
  const router = useRouter();
  const [weekStart, setWeekStart] = useState(() => mondayOf(new Date()));
  const dayCount = showWeekends ? 7 : 5;
  const days = Array.from({ length: dayCount }, (_, i) => new Date(weekStart.getFullYear(), weekStart.getMonth(), weekStart.getDate() + i));
  const weekEnd = days[dayCount - 1];
  const shift = (weeks: number) => setWeekStart(new Date(weekStart.getFullYear(), weekStart.getMonth(), weekStart.getDate() + weeks * 7));
  const todayKey = new Date().toDateString();
  const [hover, setHover] = useState<Hover | null>(null);

  const perDay = days.map((day) => layoutDay(
    items
      .filter(({ date }) => date && date.toDateString() === day.toDateString())
      .map(({ m, date }) => ({ item: m, start: date!, seconds: m.durationSeconds })),
  ));

  // Working hours by default, stretched to fit this week's meetings
  const all = perDay.flat();
  const firstHour = Math.min(8, ...all.map((p) => Math.floor(p.startMin / 60)));
  const lastHour = Math.max(19, ...all.map((p) => Math.ceil(p.endMin / 60)));
  const hours = Array.from({ length: lastHour - firstHour }, (_, i) => firstHour + i);
  const top = (min: number) => ((min - firstHour * 60) / 60) * HOUR_PX;

  return (
    <div>
      <div className="mb-2 flex items-center gap-1 text-xs text-muted-foreground">
        <button type="button" aria-label="Previous week" onClick={() => shift(-1)} className="rounded p-1 hover:bg-accent/60">
          <ChevronLeft className="h-4 w-4" />
        </button>
        <button type="button" aria-label="Next week" onClick={() => shift(1)} className="rounded p-1 hover:bg-accent/60">
          <ChevronRight className="h-4 w-4" />
        </button>
        <span className="ml-1 text-foreground">{rangeFmt.format(weekStart)} – {rangeFmt.format(weekEnd)}</span>
        <button type="button" onClick={() => setWeekStart(mondayOf(new Date()))} className="ml-2 rounded px-2 py-0.5 hover:bg-accent/60">
          This week
        </button>
        <span className="ml-auto">{all.length} meeting{all.length === 1 ? '' : 's'}</span>
      </div>

      <div className="grid text-xs"
        style={{ gridTemplateColumns: `3.5rem repeat(${dayCount}, minmax(0, 1fr))` }}>
        <div />
        {days.map((d) => (
          <div
            key={d.toDateString()}
            className={`border-b border-border pb-1 text-center ${d.toDateString() === todayKey ? 'font-semibold text-foreground' : 'text-muted-foreground'}`}
          >
            {dayFmt.format(d)}
          </div>
        ))}

        <div className="relative" style={{ height: hours.length * HOUR_PX }}>
          {hours.map((h) => (
            <div key={h} className="absolute right-2 -translate-y-1/2 whitespace-nowrap text-[10px] text-muted-foreground" style={{ top: top(h * 60) }}>
              {h > firstHour ? hourFmt.format(new Date(2000, 0, 1, h)) : ''}
            </div>
          ))}
        </div>

        {perDay.map((placed, i) => (
          <div
            key={i}
            className={`relative border-l border-border ${days[i].toDateString() === todayKey ? 'bg-accent/20' : ''}`}
            style={{ height: hours.length * HOUR_PX }}
          >
            {hours.map((h) => (
              <div key={h} className="absolute inset-x-0 border-t border-border/50" style={{ top: top(h * 60) }} />
            ))}
            {placed.map((p) => {
              const height = Math.max(18, top(p.endMin) - top(p.startMin) - 2);
              return (
              <button
                key={p.item.id}
                type="button"
                onClick={() => router.push(`/meeting-details?id=${p.item.id}`)}
                onMouseEnter={(e) => setHover({ m: p.item, startMin: p.startMin, endMin: p.endMin, rect: e.currentTarget.getBoundingClientRect() })}
                onMouseLeave={() => setHover(null)}
                onFocus={(e) => setHover({ m: p.item, startMin: p.startMin, endMin: p.endMin, rect: e.currentTarget.getBoundingClientRect() })}
                onBlur={() => setHover(null)}
                className="absolute overflow-hidden rounded border-l-2 border-primary bg-primary/10 px-1 py-0.5 text-left leading-tight hover:bg-primary/20"
                style={{
                  top: top(p.startMin) + 1,
                  height,
                  left: `calc(${(p.lane / p.lanes) * 100}% + 2px)`,
                  width: `calc(${100 / p.lanes}% - 4px)`,
                }}
              >
                <div className="truncate font-medium text-foreground">{p.item.title}</div>
                {height >= TWO_LINE_PX && <div className="truncate text-[10px] text-muted-foreground">{projectLabel(p.item)}</div>}
              </button>
              );
            })}
          </div>
        ))}
      </div>
      {hover && <HoverCard {...hover} />}
    </div>
  );
}

// Fixed so the scroll container never clips it; sits right of the block, left when that would overflow
function HoverCard({ m, startMin, endMin, rect }: Hover) {
  const right = rect.right + 8 + CARD_W <= window.innerWidth;
  const length = formatDuration(m.durationSeconds);
  return (
    <div
      role="tooltip"
      className="pointer-events-none fixed z-50 rounded-md border border-border bg-popover p-3 text-xs text-popover-foreground shadow-lg"
      style={{ width: CARD_W, top: Math.min(rect.top, window.innerHeight - 160), left: right ? rect.right + 8 : rect.left - 8 - CARD_W }}
    >
      <div className="font-medium leading-snug">{m.title}</div>
      <div className="mt-1 text-muted-foreground">
        {[`${clock(startMin)} – ${m.durationSeconds ? clock(Math.min(endMin, 24 * 60 - 1)) : '…'}`, length, projectLabel(m)].filter(Boolean).join(' · ')}
      </div>
      {!!m.tags?.length && (
        <div className="mt-2 flex flex-wrap gap-1">
          {m.tags.map((t) => (
            <span key={t} className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">{t}</span>
          ))}
        </div>
      )}
    </div>
  );
}
