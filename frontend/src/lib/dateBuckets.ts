// Sidebar dates: group a project's meetings under Today / Yesterday / This week / Last week / <Month YYYY>.
// Pure and `now`-driven so it is testable; all day math is in local time, weeks start on Monday.

export type SortOrder = 'newest' | 'oldest';

export interface Bucket {
  key: string;
  label: string;
  kind: 'day' | 'week' | 'month' | 'undated';
}

const UNDATED: Bucket = { key: 'undated', label: 'Undated', kind: 'undated' };

const dayStart = (d: Date, offset = 0) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + offset);
const weekStart = (d: Date) => dayStart(d, -((d.getDay() + 6) % 7));

const monthFmt = new Intl.DateTimeFormat('en-US', { month: 'long', year: 'numeric' });
const shortDayFmt = new Intl.DateTimeFormat('en-US', { month: 'short', day: 'numeric' });
const weekdayFmt = new Intl.DateTimeFormat('en-US', { weekday: 'short' });
// Names stay English like the rest of the UI; the clock follows the system's 12h/24h setting
const timeFmt = new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' });

export function parseDate(s?: string): Date | null {
  if (!s) return null;
  const d = new Date(s);
  return isNaN(d.getTime()) ? null : d;
}

export function bucketOf(date: Date | null, now: Date): Bucket {
  if (!date || isNaN(date.getTime())) return UNDATED;
  const today = dayStart(now);
  const thisWeek = weekStart(now);
  if (date >= today) return { key: 'today', label: 'Today', kind: 'day' };
  if (date >= dayStart(now, -1)) return { key: 'yesterday', label: 'Yesterday', kind: 'day' };
  if (date >= thisWeek) return { key: 'this-week', label: 'This week', kind: 'week' };
  if (date >= dayStart(thisWeek, -7)) return { key: 'last-week', label: 'Last week', kind: 'week' };
  return { key: `${date.getFullYear()}-${date.getMonth() + 1}`, label: monthFmt.format(date), kind: 'month' };
}

export function rowLabel(date: Date | null, bucket: Bucket): string {
  if (!date) return '';
  switch (bucket.kind) {
    case 'day': return timeFmt.format(date);
    case 'week': return `${weekdayFmt.format(date)} ${timeFmt.format(date)}`;
    case 'month': return shortDayFmt.format(date);
    default: return '';
  }
}

export function groupByDate<T>(
  items: T[],
  dateOf: (t: T) => Date | null,
  order: SortOrder,
  now: Date,
): { bucket: Bucket; items: T[] }[] {
  const dated: { item: T; date: Date }[] = [];
  const undated: T[] = [];
  for (const item of items) {
    const date = dateOf(item);
    if (date) dated.push({ item, date }); else undated.push(item);
  }
  const sign = order === 'newest' ? -1 : 1;
  dated.sort((a, b) => sign * (a.date.getTime() - b.date.getTime()));

  // Sorted input keeps each bucket contiguous, so one pass groups them
  const groups: { bucket: Bucket; items: T[] }[] = [];
  for (const { item, date } of dated) {
    const bucket = bucketOf(date, now);
    const last = groups[groups.length - 1];
    if (last?.bucket.key === bucket.key) last.items.push(item); else groups.push({ bucket, items: [item] });
  }
  if (undated.length) groups.push({ bucket: UNDATED, items: undated });
  return groups;
}
