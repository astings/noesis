# README media

Regenerates the screenshots and the demo GIF in `docs/readme/` from the command line. Nothing real is shown: the Next.js UI runs in headless Chromium against a fake Tauri backend (`mock-tauri.js`) that serves invented meetings.

```bash
npm --prefix scripts/readme-media install        # once: Playwright and its Chromium
npm --prefix scripts/readme-media run capture    # all shots, about 1 minute
npm --prefix scripts/readme-media run capture -- --only=ask,gif
```

Needs `pnpm install` done in `frontend/` and `ffmpeg` on the PATH. A dev server already on port 3118 (`pnpm run dev` or `tauri:dev`) is reused; otherwise one is started and stopped. `PORT=<n>` points at another port. The script refuses to start a second server while one runs for `frontend/`, because two `next dev` processes share `.next` and break each other.

| File | Scene |
|---|---|
| `home.png` | Recent list and projects sidebar |
| `meeting.png` | Transcript with me/them labels and tags |
| `search.png` | Title-bar search dropdown |
| `ask.png` | Ask panel answer |
| `demo.gif` | Search, open the hit, ask a question (video re-encoded with a two-pass ffmpeg palette) |

Changing what is shown:

- Demo meetings, transcripts and the Ask answer: the data at the top of `mock-tauri.js`.
- Steps and timing: `scenes` in `capture.mjs`.
- The clock is fixed to 2026-10-30 15:00 New York, so "Today" and "Yesterday" stay put.
- A UI change that calls a new command gets `null` back; the home scene prints the unhandled command names. Add a handler when a screen breaks.
