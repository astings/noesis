# Sidebar Meeting Dates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show when each meeting happened in the sidebar: date buckets inside each project, a short date/time per row, and a newest/oldest toggle.

**Architecture:** Rust adds `started_at` (folder stamp first, `created_at` fallback) to the `api_get_meetings` payload. A pure TS module buckets and sorts meetings. The sidebar renders bucket headers inside each project and a sort toggle in the "Projects" header.

**Tech Stack:** Rust (chrono), Next.js 14 / React 18, `Intl.DateTimeFormat`, Node 26 built-in test runner.

Spec: `docs/superpowers/specs/2026-10-28-sidebar-dates-design.md`.

## Global Constraints

- Additive only: MCP tool schemas, `metadata.json`, `transcript.md` untouched.
- No new dependency (runtime or test).
- `localStorage` key `noesis.sidebar.sortOrder`, values `newest` | `oldest`, default `newest`, every access in try/catch.
- Bucket labels in English: `Today`, `Yesterday`, `This week`, `Last week`, `<Month YYYY>`, `Undated`. Week starts Monday.
- The spec says the toggle sits in the "Meeting Notes" header; in the current UI that header reads "Projects". The toggle goes there.

---

### Task 1: `started_at` on the meeting list (Rust)

**Files:**
- Modify: `frontend/src-tauri/src/audio/library.rs` (new `started_at_of` next to `stamp_of_folder`, tests in `mod tests`)
- Modify: `frontend/src-tauri/src/api/api.rs` (`Meeting` struct, `api_get_meetings` map)

**Interfaces:**
- Produces: `pub fn started_at_of(folder_path: Option<&str>, created_at: chrono::DateTime<chrono::Utc>) -> String` (RFC 3339, local offset); JSON field `started_at` on each meeting from `api_get_meetings`.

- [ ] **Step 1: Failing tests** in `library.rs` `mod tests`:

```rust
#[test]
fn started_at_prefers_folder_stamp_then_created_at() {
    use chrono::TimeZone;
    let created = chrono::Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 0).unwrap();
    let from_stamp = started_at_of(Some("/r/acme/2026-10-28_09-30 kickoff"), created);
    let parsed = chrono::DateTime::parse_from_rfc3339(&from_stamp).unwrap().with_timezone(&chrono::Local);
    assert_eq!(parsed.format("%Y-%m-%d %H:%M").to_string(), "2026-10-28 09:30");
    let legacy = started_at_of(Some("/r/Meeting_2026"), created);
    assert_eq!(chrono::DateTime::parse_from_rfc3339(&legacy).unwrap(), created);
    assert_eq!(chrono::DateTime::parse_from_rfc3339(&started_at_of(None, created)).unwrap(), created);
}
```

- [ ] **Step 2:** `cargo test --lib started_at_prefers` fails (function missing).
- [ ] **Step 3: Implement** in `library.rs`:

```rust
/// spec sidebar-dates 2: the folder stamp is the meeting start; `created_at` is the save time.
pub fn started_at_of(folder_path: Option<&str>, created_at: chrono::DateTime<chrono::Utc>) -> String {
    use chrono::TimeZone;
    folder_path
        .and_then(|f| Path::new(f).file_name())
        .and_then(|n| stamp_of_folder(&n.to_string_lossy()).map(str::to_string))
        .and_then(|s| chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d_%H-%M").ok())
        .and_then(|n| chrono::Local.from_local_datetime(&n).earliest())
        .map(|d| d.to_rfc3339())
        .unwrap_or_else(|| created_at.with_timezone(&chrono::Local).to_rfc3339())
}
```

In `api.rs`: add `pub started_at: String,` to `Meeting` and in the map `started_at: crate::audio::library::started_at_of(m.folder_path.as_deref(), m.created_at.0),` (before `project`, which moves `folder_path`'s borrow fine since both borrow).

- [ ] **Step 4:** test passes; `cargo check` clean.
- [ ] **Step 5:** commit `feat(sidebar): meeting list carries started_at from the folder stamp`.

### Task 2: `dateBuckets` module (TS)

**Files:**
- Create: `frontend/src/lib/dateBuckets.ts`, `frontend/src/lib/dateBuckets.test.ts`
- Modify: `frontend/tsconfig.json` (exclude `**/*.test.ts`: the test imports with a `.ts` extension for Node), `frontend/package.json` (`"test": "node --test src/lib/*.test.ts"`)

**Interfaces:**
- Produces:
  - `type SortOrder = 'newest' | 'oldest'`
  - `type Bucket = { key: string; label: string; kind: 'day' | 'week' | 'month' | 'undated' }`
  - `bucketOf(date: Date | null, now: Date): Bucket`
  - `rowLabel(date: Date | null, bucket: Bucket): string`
  - `groupByDate<T>(items: T[], dateOf: (t: T) => Date | null, order: SortOrder, now: Date): { bucket: Bucket; items: T[] }[]`
  - `parseDate(s?: string): Date | null`

- [ ] **Step 1: Failing tests** (`node:test` + `node:assert/strict`), with now = Wed 2026-10-28 12:00 local:
  - 00:00 same day → Today; 23:59 previous day → Yesterday
  - Mon 2026-10-26 → This week; Sun 2026-10-25 → Last week; Mon 2026-10-19 → Last week; Sun 2026-10-18 → `October 2026`
  - 2025-12-31 → `December 2025`, 2026-01-01 → `January 2026`
  - now = Mon 2026-10-26 09:00: Sun 25th → Yesterday, Sat 24th → Last week
  - `null` / invalid → Undated; `rowLabel` week → starts with `Mon `; month → `Sep 28`
  - `groupByDate` newest: Today group before October group, rows newest first, Undated last; oldest: reversed, Undated still last
- [ ] **Step 2:** `pnpm test` fails (module missing).
- [ ] **Step 3: Implement** `dateBuckets.ts` (local-time day math, Monday week start, `Intl.DateTimeFormat('en-US', …)` for names, `Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' })` for times, undated kept apart and appended last).
- [ ] **Step 4:** `pnpm test` passes.
- [ ] **Step 5:** commit `feat(sidebar): date bucket helpers`.

### Task 3: Sidebar render and sort toggle

**Files:**
- Modify: `frontend/src/components/Sidebar/SidebarProvider.tsx` (`startedAt` on `CurrentMeeting`, on `SidebarItem`, filled in `fetchMeetings` and `baseItems`)
- Modify: `frontend/src/components/Sidebar/index.tsx` (`startedAt` on `SidebarItem`, sort state, toggle button, grouped children, row label)

**Interfaces:**
- Consumes: `started_at` (Task 1), `groupByDate`, `rowLabel`, `bucketOf`, `parseDate`, `SortOrder` (Task 2).

- [ ] **Step 1:** Provider: `startedAt: meeting.started_at` in `transformedMeetings`, `startedAt: meeting.startedAt` in file children.
- [ ] **Step 2:** `index.tsx`: `sortOrder` state from `localStorage` (try/catch), toggle button (`ArrowDown`/`ArrowUp` + `Newest`/`Oldest`, `aria-label`) at the right of the "Projects" header, persisted on change.
- [ ] **Step 3:** In `renderItem`, a project's expanded children render as `groupByDate(children, c => parseDate(c.startedAt), sortOrder, now)`: a non-interactive uppercase dimmed header per bucket, then rows. Rows get a dimmed `tabular-nums` label before the title and a full date/time `title` attribute.
- [ ] **Step 4:** `pnpm exec tsc --noEmit` clean, `pnpm test` passes; check in the running app (Inbox and one project, toggle both ways, drag a meeting onto another project still works).
- [ ] **Step 5:** commit `feat(sidebar): meetings grouped by date with a newest/oldest toggle`.
