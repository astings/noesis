# Home: recent meetings and meeting duration

Date: 2026-10-28. Status: approved in brainstorming.

## Problem

When idle, the home page shows an empty transcript ("No transcript yet"). It feels unfinished. We want something useful there while staying minimal.

## Decision

Option B from the brainstorm: show a "Recent" list of the last 5 meetings in place of the empty-transcript message. Also show meeting duration in the meeting view header.

Rejected: the ready-state screen (devices, model, mic meter) and the "needs you" list of pending work. Both can be revisited later.

## Behaviour

### Home page (`app/page.tsx`)

- When the app is not recording and there is no transcript, show "Recent" with the 5 newest meetings by `startedAt`. It takes the place of the "No transcript yet" empty state (see Components).
- Each row shows the title, a project tag (`Inbox` for `_inbox`), a relative date and the duration:
  - Date: reuse the sidebar format from `lib/dateBuckets.ts` with the day name in front: "Today 14:00", "Yesterday 09:30", "Mon 14:00" (this or last week), otherwise "Oct 12".
  - Duration: under 1 hour "42m", otherwise "1h05". If `durationSeconds` is missing, show no duration.
- Clicking a row goes to `/meeting-details?id=<id>`.
- With zero meetings, the existing "No transcript yet" message stays.
- While recording, or while paused or "Listening for speech", nothing changes from today.
- The record button and project select stay where they are.

### Meeting view (`components/MeetingDetails/TranscriptPanel.tsx`)

- The header today reads `Title · 28 Oct, 14:00`. Add the duration after it: `Title · 28 Oct, 14:00 · 42m`, using the same duration format as the home page.
- If the duration is missing, the header stays as it is now.

## Data

- Rust: `api_get_meetings` (`src-tauri/src/api/api.rs`) adds an optional `duration_seconds: Option<f64>` field to each `Meeting`. It's read from `<folder_path>/metadata.json` (key `duration_seconds`, written by `recording_saver.rs`).
- The value is `None` when the folder path, the file or the key is missing, or the JSON is invalid. This is never an error, and the meeting list must still load.
- The read goes in a helper `duration_of(folder: &Path) -> Option<f64>` in `audio/library.rs`, next to `started_at_of`.
- Frontend: `SidebarProvider` maps `duration_seconds` to `durationSeconds?: number` on `CurrentMeeting`. Both the home list and the meeting header read from `useSidebar().meetings`, so there is no new command and no new fetch.
- Cost: one small file read per meeting on each list refresh, which is acceptable at personal scale. If it gets slow, cache the duration in SQLite (`ponytail:` comment at the read site).

## Contracts

- `metadata.json` v2 and `transcript.md` are only read, never changed.
- `api_get_meetings` gets one new optional field and nothing is removed, so it stays additive.

## Components

- New `components/RecentMeetings.tsx` (about 50 lines). It takes no props and reads `meetings` from `useSidebar()`. Duration formatting goes in `lib/duration.ts` (pure, tested with `pnpm test`) and is shared with the meeting header.
- `app/_components/TranscriptPanel.tsx` (home only): renders `<RecentMeetings />` instead of `VirtualizedTranscriptView` when not recording, there are no segments and `meetings.length > 0`. `VirtualizedTranscriptView` is untouched because the meeting view uses it too.
- `TranscriptPanel.tsx` (meeting details): adds `· {formatDuration(status.durationSeconds)}` after the date.

## Testing

- Rust unit test for `duration_of`: a valid file returns the value; a missing file, a missing key and invalid JSON each return `None`.
- Manual: idle home with meetings shows the list; a fresh install shows the old empty state; starting a recording shows the live transcript; the meeting header shows the duration; a legacy meeting with no `metadata.json` shows no duration.
