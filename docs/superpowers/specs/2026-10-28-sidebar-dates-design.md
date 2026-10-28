# Sidebar meeting dates: design

Status: approved 2026-10-28. Builds on `main` (e533700). Branch `enhance/sidebar-dates`.

## 1. Goal

The sidebar lists meetings per project by title only, so the owner cannot see when a meeting happened. Show the date and time of each meeting, grouped into date sections inside each project, with a newest/oldest sort toggle.

## 2. Data: `started_at` on the meeting list

The SQLite `meetings.created_at` column exists and `get_meetings` already sorts by it, but `api_get_meetings` (`src-tauri/src/api/api.rs`) drops it. For live recordings `created_at` is set when the transcript row is saved, which is close to the end of the meeting, not its start.

Design: `Meeting` gains `started_at: String`, RFC 3339 with the local offset. It is computed in `api_get_meetings`:

1. If `folder_path` has a v2 folder name, parse its stamp (`library::stamp_of_folder`, format `%Y-%m-%d_%H-%M`) as local time. This is the meeting start and the filesystem is the source of truth.
2. Otherwise (legacy flat folders, no folder, unparsable stamp) use `created_at`.

The derivation is a small pure function in `audio/library.rs` (`started_at_of(folder_path, created_at)`), so it is testable without a database. The change is additive: the MCP tool schemas, `metadata.json` and `transcript.md` are untouched.

## 3. Date buckets

A new pure module `frontend/src/lib/dateBuckets.ts`, plain `Intl.DateTimeFormat`, no dependency:

- `bucketOf(date, now)` returns a stable key and label. Buckets, in local time:
  - `Today`
  - `Yesterday`
  - `This week`: from Monday of the current week, excluding today and yesterday
  - `Last week`: Monday to Sunday of the previous week, excluding yesterday
  - one bucket per calendar month otherwise, labelled like `September 2026`
- `rowLabel(date, bucket)` returns the short label shown on the row:
  - `09:30` in Today and Yesterday
  - `Mon 11:00` in This week and Last week
  - `Sep 28` in month buckets
- A meeting with a missing or unparsable `started_at` goes to an `Undated` bucket, placed last, with no row label.

Labels are English, matching the rest of the UI. Times use the system locale's 24h or 12h setting through `Intl`.

## 4. Sidebar render

- `CurrentMeeting` gains `startedAt?: string`, filled in `fetchMeetings` in `SidebarProvider.tsx`.
- Inside each project section, meetings are sorted by `startedAt` in the chosen direction, then split into buckets in that order. Bucket order follows the sort direction (newest first: Today at the top; oldest first: the oldest month at the top). `Undated` is always last.
- A bucket header is a small uppercase dimmed label, not a folder: it does not collapse, is not a drag target and is not in the context menu. Drag and drop and "Move to" keep targeting the project section as today.
- Each meeting row shows its dimmed row label before the title, with the full date and time in the row's `title` attribute on hover.
- Project order is unchanged: Inbox first, then alphabetical.
- Collapsed sidebar mode is unchanged.

## 5. Sort toggle

An icon button in the "Meeting Notes" header flips between newest first (default) and oldest first, for all projects at once. It shows the current direction (arrow down plus "Newest", arrow up plus "Oldest") and has an `aria-label`. The choice is kept in `localStorage` under `noesis.sidebar.sortOrder`, with reads and writes wrapped in try/catch; on failure it falls back to newest first.

## 6. Tests

- Rust: `started_at_of` returns the stamp time for a v2 folder and falls back to `created_at` for a legacy flat folder and for `None`.
- TypeScript: `src/lib/dateBuckets.test.ts`, run with Node's built-in runner (`node --test`, Node strips types natively; no test dependency is added). `bucketOf` and `rowLabel` take `now` as a parameter so tests are deterministic. It covers just before and after midnight, a Sunday and the following Monday (week start), the last day of Last week versus the month bucket, and a year boundary (`December 2025` next to `January 2026`).

## 7. Out of scope

- Search results layout.
- Collapsible date buckets.
- Sorting projects by their latest meeting.
- Sorting by title.
