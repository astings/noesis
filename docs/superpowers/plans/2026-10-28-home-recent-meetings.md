# Home Recent Meetings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the idle "No transcript yet" home screen with a list of the last 5 meetings, and show meeting duration in the meeting view header.

**Architecture:** `api_get_meetings` gains an optional `duration_seconds` read from each meeting's `metadata.json`. `SidebarProvider` carries it on `CurrentMeeting` as `durationSeconds`. The home `TranscriptPanel` swaps in a new `RecentMeetings` component when idle, and the meeting header appends the formatted duration. No new Tauri command.

**Tech Stack:** Rust (Tauri 2, serde_json), Next.js 14 + React 18, Node's built-in test runner (`pnpm test`).

Spec: `docs/superpowers/specs/2026-10-28-home-recent-meetings-design.md`.

## Global Constraints

- `metadata.json` v2 and `transcript.md` are read only, never written.
- `api_get_meetings` change is additive: one new optional field, nothing removed or renamed.
- A missing or unreadable duration is never an error. The meeting list must still load and the row shows no duration.
- `VirtualizedTranscriptView.tsx` is not modified; the meeting view uses it too.
- Duration format: under 1 hour `42m`, otherwise `1h05`. Missing → empty string.
- Date format: reuse `bucketOf` + `rowLabel` from `frontend/src/lib/dateBuckets.ts`, prefixed with the bucket label for day buckets ("Today 14:00", "Yesterday 09:30"); week buckets already give "Mon 14:00"; month buckets give "Oct 12".
- Recent list shows at most 5 meetings, newest `startedAt` first.

## File Structure

- Modify `frontend/src-tauri/src/audio/library.rs`: add `duration_of(folder: &Path) -> Option<f64>` + tests.
- Modify `frontend/src-tauri/src/api/api.rs`: `Meeting.duration_seconds`, filled via `duration_of`.
- Create `frontend/src/lib/duration.ts` + `frontend/src/lib/duration.test.ts`: `formatDuration`.
- Modify `frontend/src/components/Sidebar/SidebarProvider.tsx`: `durationSeconds` on `CurrentMeeting`.
- Create `frontend/src/components/RecentMeetings.tsx`: the home list.
- Modify `frontend/src/app/_components/TranscriptPanel.tsx`: render `RecentMeetings` when idle.
- Modify `frontend/src/components/MeetingDetails/TranscriptPanel.tsx`: duration in header.

---

### Task 1: Backend duration field

**Files:**
- Modify: `frontend/src-tauri/src/audio/library.rs` (add fn near `started_at_of`, line ~55; tests in `mod tests`, line ~505)
- Modify: `frontend/src-tauri/src/api/api.rs:30-37` (struct) and `:358-370` (mapping)

**Interfaces:**
- Produces: `pub fn duration_of(folder: &Path) -> Option<f64>` in `crate::audio::library`; JSON field `duration_seconds: number | null` on each item returned by the `api_get_meetings` command.

- [ ] **Step 1: Write the failing test**

Add inside `mod tests` in `library.rs` (the `write_meta` helper already exists there):

```rust
    #[test]
    fn duration_of_reads_metadata_and_tolerates_bad_input() {
        let root = tempfile::tempdir().unwrap();
        let ok = root.path().join("ok");
        write_meta(&ok, r#"{"version":"2.0","duration_seconds":2555.2}"#);
        assert_eq!(duration_of(&ok), Some(2555.2));

        let no_key = root.path().join("no-key");
        write_meta(&no_key, r#"{"version":"2.0"}"#);
        assert_eq!(duration_of(&no_key), None);

        let bad = root.path().join("bad");
        write_meta(&bad, "not json");
        assert_eq!(duration_of(&bad), None);

        assert_eq!(duration_of(&root.path().join("missing")), None);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend/src-tauri && cargo test --lib audio::library::tests::duration_of`
Expected: compile error, `cannot find function duration_of`.

- [ ] **Step 3: Implement `duration_of`**

Add after `started_at_of` in `library.rs`:

```rust
/// `duration_seconds` from `<folder>/metadata.json`; None when absent or unreadable.
// ponytail: one file read per meeting per list refresh; cache in SQLite if the library gets large
pub fn duration_of(folder: &Path) -> Option<f64> {
    let raw = std::fs::read_to_string(folder.join("metadata.json")).ok()?;
    serde_json::from_str::<serde_json::Value>(&raw).ok()?.get("duration_seconds")?.as_f64()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend/src-tauri && cargo test --lib audio::library::tests::duration_of`
Expected: PASS.

- [ ] **Step 5: Add the field to `Meeting`**

In `api.rs`, extend the struct:

```rust
pub struct Meeting {
    pub id: String,
    pub title: String,
    /// spec 4.2: project folder derived from folder_path, `_inbox` when unknown or legacy
    pub project: String,
    /// Sidebar dates: meeting start, RFC 3339 (folder stamp, else created_at)
    pub started_at: String,
    /// From metadata.json; None for legacy meetings or unreadable files
    pub duration_seconds: Option<f64>,
}
```

and in the `api_get_meetings` mapping add, after `started_at: ...,`:

```rust
                    duration_seconds: m
                        .folder_path
                        .as_deref()
                        .and_then(|f| crate::audio::library::duration_of(std::path::Path::new(f))),
```


- [ ] **Step 6: Check everything that constructs `Meeting` compiles**

Run: `cd frontend/src-tauri && cargo check 2>&1 | tail -20`
Expected: no errors. If another place builds `Meeting { .. }`, add `duration_seconds: None` there.

- [ ] **Step 7: Commit**

```bash
git add frontend/src-tauri/src/audio/library.rs frontend/src-tauri/src/api/api.rs
git commit -m "feat(meetings): duration_seconds on api_get_meetings, read from metadata.json"
```

---

### Task 2: Duration formatter + sidebar field

**Files:**
- Create: `frontend/src/lib/duration.ts`
- Test: `frontend/src/lib/duration.test.ts`
- Modify: `frontend/src/components/Sidebar/SidebarProvider.tsx` (`CurrentMeeting` ~line 68, mapping ~line 165)

**Interfaces:**
- Consumes: `duration_seconds` from Task 1.
- Produces: `export function formatDuration(seconds?: number | null): string`; `CurrentMeeting.durationSeconds?: number`.

- [ ] **Step 1: Write the failing test**

`frontend/src/lib/duration.test.ts`:

```ts
// Run with `pnpm test` (Node's built-in runner, types stripped natively).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { formatDuration } from './duration.ts';

test('minutes under an hour', () => {
  assert.equal(formatDuration(2555.2), '43m');
  assert.equal(formatDuration(59), '1m');
  assert.equal(formatDuration(10), '0m');
});

test('hours with zero-padded minutes', () => {
  assert.equal(formatDuration(3600), '1h00');
  assert.equal(formatDuration(3900), '1h05');
});

test('missing or invalid gives empty string', () => {
  assert.equal(formatDuration(undefined), '');
  assert.equal(formatDuration(null), '');
  assert.equal(formatDuration(-5), '');
  assert.equal(formatDuration(NaN), '');
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test`
Expected: FAIL, cannot find module `./duration.ts`.

- [ ] **Step 3: Implement**

`frontend/src/lib/duration.ts`:

```ts
// Meeting length for list rows and the meeting header: "43m", "1h05"; "" when unknown.
export function formatDuration(seconds?: number | null): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return '';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h${String(minutes % 60).padStart(2, '0')}`;
}
```

Note: `59s` rounds to `1m`, `10s` to `0m`; `2555.2s` is 42.6 min → `43m`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm test`
Expected: all tests PASS (dateBuckets tests too).

- [ ] **Step 5: Carry the field in `SidebarProvider`**

In `CurrentMeeting`, after `startedAt?: string;`:

```ts
  // From metadata.json; absent for legacy meetings
  durationSeconds?: number;
```

In `fetchMeetings`, after `startedAt: meeting.started_at,`:

```ts
          durationSeconds: meeting.duration_seconds ?? undefined,
```

- [ ] **Step 6: Type-check**

Run: `cd frontend && pnpm exec tsc --noEmit`
Expected: no new errors.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/lib/duration.ts frontend/src/lib/duration.test.ts frontend/src/components/Sidebar/SidebarProvider.tsx
git commit -m "feat(meetings): formatDuration and durationSeconds on sidebar meetings"
```

---

### Task 3: Recent list on home + duration in meeting header

**Files:**
- Create: `frontend/src/components/RecentMeetings.tsx`
- Modify: `frontend/src/app/_components/TranscriptPanel.tsx`
- Modify: `frontend/src/components/MeetingDetails/TranscriptPanel.tsx:143-145`

**Interfaces:**
- Consumes: `formatDuration`, `CurrentMeeting.durationSeconds` (Task 2); `bucketOf`, `rowLabel`, `parseDate` from `@/lib/dateBuckets`; `INBOX` from `@/lib/projects`; `useSidebar` from `@/components/Sidebar/SidebarProvider`.
- Produces: `export function RecentMeetings()`.

- [ ] **Step 1: Create `RecentMeetings.tsx`**

```tsx
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
```

- [ ] **Step 2: Render it on the idle home**

In `frontend/src/app/_components/TranscriptPanel.tsx`, add imports:

```tsx
import { RecentMeetings } from '@/components/RecentMeetings';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
```

inside the component, after the `useIsLinux()` line:

```tsx
  const { meetings } = useSidebar();
  const showRecent = !isRecording && !isStopping && !isProcessingStop && segments.length === 0 && meetings.length > 0;
```

(`segments` is declared below with `useMemo`; place `showRecent` after the `segments` `useMemo`.) Replace the `<VirtualizedTranscriptView ... />` element with:

```tsx
      {showRecent ? (
        <RecentMeetings />
      ) : (
        <VirtualizedTranscriptView
          segments={segments}
          isRecording={isRecording}
          isPaused={isPaused}
          isProcessing={isProcessingStop}
          isStopping={isStopping}
          enableStreaming={isRecording}
          showConfidence={true}
        />
      )}
```

- [ ] **Step 3: Duration in the meeting header**

In `frontend/src/components/MeetingDetails/TranscriptPanel.tsx`, add the import:

```tsx
import { formatDuration } from '@/lib/duration';
```

after `const created = createdAt ? parseISO(createdAt) : null;` add:

```tsx
  const length = formatDuration(status?.durationSeconds);
```

and change the date span to:

```tsx
            {created && isValid(created) && (
              <span className="ml-1 flex-none font-normal text-muted-foreground">
                · {format(created, 'd MMM, HH:mm')}{length && ` · ${length}`}
              </span>
            )}
```

- [ ] **Step 4: Type-check and lint**

Run: `cd frontend && pnpm exec tsc --noEmit && pnpm lint`
Expected: no new errors.

- [ ] **Step 5: Manual check in the app**

Run: `cd frontend && pnpm run tauri:dev`
Check:
1. Idle home with meetings: "Recent" shows 5 rows, newest first, with project tag, date and duration. Clicking a row opens that meeting.
2. Start a recording: the list disappears and "Listening for speech..." shows. Stop: after save, the new meeting is at the top of the list.
3. Meeting view header reads `Title · 28 Oct, 14:00 · 43m`. A legacy meeting with no `metadata.json` shows no duration and no trailing dot.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/RecentMeetings.tsx frontend/src/app/_components/TranscriptPanel.tsx frontend/src/components/MeetingDetails/TranscriptPanel.tsx
git commit -m "feat(home): recent meetings on the idle home; duration in the meeting header"
```
