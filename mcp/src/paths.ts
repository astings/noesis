import os from 'node:os';
import path from 'node:path';

export const INBOX = '_inbox';

const AUTO_TITLE = /^Meeting (\d{2}_\d{2}_\d{2}_\d{2}_\d{2}_\d{2}|\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2})$/;
const STAMP_PREFIX = /^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}/;

/** MEETILY_RECORDINGS or ~/Movies/meetily-recordings. */
export function recordingsRoot(): string {
  return process.env.MEETILY_RECORDINGS || path.join(os.homedir(), 'Movies', 'meetily-recordings');
}

export function isValidProjectName(name: string): boolean {
  const n = name.trim();
  return n.length > 0 && n !== '.' && n !== '..' && !n.startsWith('.') && !n.includes('/') && !n.includes('\\') && !n.includes('\0');
}

export function slug(title: string): string {
  let out = '';
  let lastDash = true;
  for (const c of title) {
    if (/[A-Za-z0-9]/.test(c)) { out += c.toLowerCase(); lastDash = false; }
    else if (!lastDash) { out += '-'; lastDash = true; }
    if (out.length >= 60) break;
  }
  return out.replace(/^-+|-+$/g, '');
}

export function isAutoTitle(title: string): boolean {
  return AUTO_TITLE.test(title.trim());
}

const pad = (n: number) => String(n).padStart(2, '0');

export function localStamp(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}_${pad(date.getHours())}-${pad(date.getMinutes())}`;
}

export function stampOfFolder(name: string): string | null {
  const m = STAMP_PREFIX.exec(name);
  return m ? m[0] : null;
}

export function meetingFolderName(stamp: string, title?: string | null): string {
  const s = title && !isAutoTitle(title) ? slug(title) : '';
  return s ? `${stamp} ${s}` : stamp;
}

export function formatMmss(seconds: number): string {
  const total = Math.floor(Math.max(0, seconds || 0));
  return `${pad(Math.floor(total / 60))}:${pad(total % 60)}`;
}

/** RFC 3339 in UTC rendered in local time with an explicit offset; garbage passes through. */
export function toLocalIso(rfc3339: string): string {
  const d = new Date(rfc3339);
  if (Number.isNaN(d.getTime())) return rfc3339;
  const offsetMin = -d.getTimezoneOffset();
  const sign = offsetMin >= 0 ? '+' : '-';
  const abs = Math.abs(offsetMin);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`;
}

const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;
const ISO_DATETIME = /^\d{4}-\d{2}-\d{2}T/;

/** A YYYY-MM-DD value means that whole local day; a datetime is taken as given. Returns NaN for anything else. */
export function parseBound(value: string, endOfDay: boolean): number {
  if (ISO_DATE.test(value)) {
    const [y, m, d] = value.split('-').map(Number);
    return endOfDay ? new Date(y, m - 1, d, 23, 59, 59, 999).getTime() : new Date(y, m - 1, d).getTime();
  }
  return ISO_DATETIME.test(value) ? Date.parse(value) : Number.NaN;
}
