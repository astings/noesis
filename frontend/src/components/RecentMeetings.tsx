'use client';

import { useRouter } from 'next/navigation';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { bucketOf, parseDate, rowLabel } from '@/lib/dateBuckets';
import { formatDuration } from '@/lib/duration';
import { INBOX } from '@/lib/projects';

// Idle home: the last few meetings instead of an empty transcript
export function RecentMeetings() {
  const { meetings } = useSidebar();
  const router = useRouter();
  const now = new Date();
  const recent = meetings
    .map((m) => ({ m, date: parseDate(m.startedAt) }))
    .sort((a, b) => (b.date?.getTime() ?? 0) - (a.date?.getTime() ?? 0))
    .slice(0, 5);

  return (
    <div className="px-6 py-5">
      <h2 className="mb-2 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">Recent</h2>
      <ul>
        {recent.map(({ m, date }) => {
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
                <span className="flex-none rounded bg-muted px-1.5 text-[10px] text-muted-foreground">
                  {!m.project || m.project === INBOX ? 'Inbox' : m.project}
                </span>
                <span className="ml-auto flex-none text-xs text-muted-foreground">
                  {[when, length].filter(Boolean).join(' · ')}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
