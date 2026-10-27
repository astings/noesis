# Phase 5: Speaker Labels (Me and Them) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every transcript segment carries a speaker label, `me` (the microphone, the owner) or `them` (system audio, the other participants) or `both`, live and after enhance. `transcript.md` prints `[mm:ss] Alexandre: ...` and `[mm:ss] Others: ...`, and the enhance pass splits speech segments at speaker turns so Whisper segments stop straddling two speakers.

**Architecture:** The audio pipeline is the only place where microphone and system audio are still separate (`audio/pipeline.rs`, right before `mix_window`). There, every 600 ms mixing window is cut into 50 ms sub-windows and the RMS of each stream is appended to an energy timeline. Live VAD segments are labelled from that timeline (energies normalized per stream by their running peak, 70/30 rule). The timeline is streamed to the recording saver, which writes it as `speakers.json` at stop. The enhance pass loads `speakers.json`, splits each VAD segment where the dominant stream changes for at least one second, transcribes each piece, and labels it. A small pure module, `audio/speaker_energy.rs`, holds all the math and is unit tested. Names come from a new `owner_name` preference ("Me" by default) and `Others`.

**Tech Stack:** Rust (Tauri 2, tokio mpsc, serde_json), TypeScript (types only plus one settings input).

Spec: `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md`, sections 4.4, 5.2 (`speakers`, `speakers_file`), 5.3 (speaker line and labelled lines).

## Global Constraints

- Additive changes marked `// spec 4.4`. New logic lives in `audio/speaker_energy.rs`; edits to existing files are small and never change mixing, VAD or recording behaviour. `audio.mp4` stays mono.
- Timeline: 50 ms windows, `(mic_rms, sys_rms)` pairs, computed from the un-mixed windows in the pipeline. Persisted as `speakers.json`: `{"version": 1, "window_ms": 50, "streams": ["me", "them"], "windows": [[m, s], ...]}` with values rounded to 5 decimals.
- Labelling rule (spec 4.4): normalize each stream's RMS by a per-stream reference (live: running peak so far; enhance: 95th percentile of non-zero windows). Over a span, ignore windows whose larger normalized value is below 0.1 (noise). Sum the normalized values; `me` when the mic share is at least 0.7, `them` when at most 0.3, otherwise `both`; no label when no window counted.
- Turn splitting (enhance only): a per-window dominant stream (mic when its normalized value is at least the system's; noise windows inherit the previous dominant), runs shorter than 1000 ms merged into their neighbours, a boundary at every remaining change; segments split at those boundaries.
- Labels travel as `Option<String>` with values `"me"`, `"them"`, `"both"` on `AudioChunk`, `TranscriptUpdate`, the saver's `TranscriptSegment`, the API `TranscriptSegment`, and the `transcripts.speaker` column (which already exists and is unused today). `transcripts.json` gains `speaker` per segment. Missing or unknown is `null`.
- `transcript.md` (spec 5.3): frontmatter gains `speakers: me=<owner_name>, them=Others` when the meeting has any label; a labelled line reads `[mm:ss] <Name>: text`; `both` and unlabelled lines stay bare. Phase 2's byte-exact test for unlabelled transcripts must keep passing.
- `metadata.json` gains `speakers: {"me": "<owner_name>", "them": "Others"}` and `speakers_file: "speakers.json"` (spec 5.2), written by the saver at stop. Phase 1 and 2 keys are untouched.
- Preference `owner_name: String` (default `"Me"`) in `recording_preferences.json`, with a text input in Recording settings.
- The transcript UI does not change in this phase (labels appear in `transcript.md`, `transcripts.json` and the database); a visible prefix in the app is a follow-up.
- No em-dashes in any text, code comment or UI string.
- Rust tests: `cd frontend/src-tauri && cargo test --lib <module>`; full suite on `main` is 274 passed, 3 ignored, upstream warnings exist, add none. Type check: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json`. Never run pnpm.
- Branch `phase-5/speaker-labels`, one commit per task, no push.

## File Structure

| File | Responsibility |
| --- | --- |
| `frontend/src-tauri/src/audio/speaker_energy.rs` (create) | `Speaker`, `EnergyTimeline` (push, references, label_span, turn_boundaries, save, load), `rms` |
| `frontend/src-tauri/src/audio/mod.rs` (modify) | Declare the module |
| `frontend/src-tauri/src/audio/recording_state.rs` (modify) | `AudioChunk.speaker` |
| `frontend/src-tauri/src/audio/pipeline.rs` (modify) | Timeline in the mixing loop, live labels on transcription chunks, energy sender, `speaker: None` on other constructors |
| `frontend/src-tauri/src/audio/recording_manager.rs` (modify) | Energy channel wiring, `set_owner_name` pass-through |
| `frontend/src-tauri/src/audio/recording_saver.rs` (modify) | `TranscriptSegment.speaker`, energy accumulation, `speakers.json`, metadata `speakers` keys, labelled transcript.md |
| `frontend/src-tauri/src/audio/transcription/worker.rs` (modify) | `TranscriptUpdate.speaker` |
| `frontend/src-tauri/src/audio/recording_commands.rs` (modify) | Copy `speaker` into the saved segment; read `owner_name` at start |
| `frontend/src-tauri/src/audio/recording_preferences.rs` (modify) | `owner_name` |
| `frontend/src-tauri/src/api/api.rs`, `database/repositories/transcript.rs` (modify) | `speaker` on the API segment and in the INSERT |
| `frontend/src-tauri/src/audio/common.rs` (modify) | Labelled segment helper, `speaker` in `transcripts.json` |
| `frontend/src-tauri/src/audio/transcript_md.rs` (modify) | `MdSegment`, speaker names, labelled lines |
| `frontend/src-tauri/src/audio/retranscription.rs` (modify) | Turn splitting and labelling in the enhance pass |
| `frontend/src/types/index.ts`, `frontend/src/contexts/TranscriptContext.tsx`, `frontend/src/components/RecordingSettings.tsx` (modify) | `speaker` on the TS types and live transcripts; `owner_name` input |

---

### Task 1: `speaker_energy.rs`

**Files:**
- Create: `frontend/src-tauri/src/audio/speaker_energy.rs`
- Modify: `frontend/src-tauri/src/audio/mod.rs` (after `pub mod transcript_md;`)

**Interfaces:**
- Produces (all `pub`):
  - `pub const WINDOW_MS: u32 = 50;`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Speaker { Me, Them, Both }` with `pub fn as_str(self) -> &'static str` (`"me"`, `"them"`, `"both"`) and `pub fn parse(s: &str) -> Option<Speaker>`
  - `#[derive(Debug, Clone, Default, Serialize, Deserialize)] pub struct EnergyTimeline { pub window_ms: u32, pub windows: Vec<(f32, f32)>, #[serde(skip)] max_mic: f32, #[serde(skip)] max_sys: f32 }`
  - `impl EnergyTimeline { pub fn new() -> Self; pub fn push(&mut self, mic_rms: f32, sys_rms: f32); pub fn push_window_pair(&mut self, mic: &[f32], sys: &[f32], sample_rate: u32) -> Vec<(f32, f32)>; pub fn extend(&mut self, pairs: &[(f32, f32)]); pub fn running_references(&self) -> (f32, f32); pub fn references(&self) -> (f32, f32); pub fn label_span(&self, start_ms: f64, end_ms: f64, refs: (f32, f32)) -> Option<Speaker>; pub fn turn_boundaries(&self, start_ms: f64, end_ms: f64, refs: (f32, f32), min_run_ms: u32) -> Vec<f64>; pub fn save(&self, folder: &Path) -> anyhow::Result<()>; pub fn load(folder: &Path) -> Option<EnergyTimeline>; pub fn has_speech(&self) -> bool }`
  - `pub fn rms(samples: &[f32]) -> f32`

- [ ] **Step 1: Failing tests**

Create the file with header and tests:

```rust
//! spec 4.4: who is speaking, from energy alone. The microphone is the owner ("me"),
//! system audio is everyone else ("them"). The pipeline records the RMS of both
//! streams per 50 ms window while they are still separate; this module turns that
//! timeline into labels and into split points at speaker turns.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a timeline from a pattern string: 'm' = mic loud, 't' = system loud, 'b' = both, '.' = silence.
    fn timeline(pattern: &str) -> EnergyTimeline {
        let mut t = EnergyTimeline::new();
        for c in pattern.chars() {
            match c {
                'm' => t.push(0.20, 0.01),
                't' => t.push(0.01, 0.08),
                'b' => t.push(0.20, 0.08),
                _ => t.push(0.001, 0.0005),
            }
        }
        t
    }

    #[test]
    fn rms_of_known_signal() {
        assert!((rms(&[0.5, -0.5, 0.5, -0.5]) - 0.5).abs() < 1e-6);
        assert_eq!(rms(&[]), 0.0);
    }

    #[test]
    fn window_pairs_are_cut_into_50ms_subwindows() {
        let mut t = EnergyTimeline::new();
        let mic = vec![0.5f32; 48_000 * 600 / 1000]; // one 600 ms window at 48 kHz
        let sys = vec![0.0f32; 48_000 * 600 / 1000];
        let pairs = t.push_window_pair(&mic, &sys, 48_000);
        assert_eq!(pairs.len(), 12);
        assert_eq!(t.windows.len(), 12);
        assert!((pairs[0].0 - 0.5).abs() < 1e-6 && pairs[0].1 == 0.0);
    }

    #[test]
    fn references_use_the_95th_percentile_of_nonzero_windows() {
        let t = timeline("mmmmmmmmmmtttttttttt....");
        let (rm, rs) = t.references();
        assert!((rm - 0.20).abs() < 1e-6, "{}", rm);
        assert!((rs - 0.08).abs() < 1e-6, "{}", rs);
        assert_eq!(timeline("....").references(), (1e-6, 1e-6), "silence falls back to a floor");
    }

    #[test]
    fn spans_are_labelled_by_dominant_normalized_energy() {
        let t = timeline("mmmmmmmmtttttttt");
        let refs = t.references();
        assert_eq!(t.label_span(0.0, 400.0, refs), Some(Speaker::Me));
        assert_eq!(t.label_span(400.0, 800.0, refs), Some(Speaker::Them));
        assert_eq!(t.label_span(0.0, 800.0, refs), Some(Speaker::Both));
        assert_eq!(timeline("........").label_span(0.0, 400.0, (0.2, 0.08)), None, "noise only");
    }

    #[test]
    fn running_references_track_the_peak_so_far() {
        let mut t = EnergyTimeline::new();
        t.push(0.1, 0.02);
        t.push(0.3, 0.01);
        assert_eq!(t.running_references(), (0.3, 0.02));
    }

    #[test]
    fn turn_boundaries_split_at_sustained_changes_only() {
        // 40 windows of me (2 s), a 10-window blip of them (0.5 s), 40 windows of me, then 40 of them
        let t = timeline(&format!("{}{}{}{}", "m".repeat(40), "t".repeat(10), "m".repeat(40), "t".repeat(40)));
        let refs = t.references();
        let b = t.turn_boundaries(0.0, 130.0 * 50.0, refs, 1000);
        assert_eq!(b, vec![90.0 * 50.0], "the 0.5 s blip is absorbed; the real turn at 4.5 s remains");
        assert!(t.turn_boundaries(0.0, 40.0 * 50.0, refs, 1000).is_empty(), "single speaker, no split");
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let t = timeline("mmtt");
        t.save(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join("speakers.json")).unwrap();
        assert!(text.contains("\"window_ms\": 50") && text.contains("\"streams\""));
        let back = EnergyTimeline::load(dir.path()).unwrap();
        assert_eq!(back.windows.len(), 4);
        assert_eq!(back.references(), t.references());
        assert!(EnergyTimeline::load(Path::new("/nonexistent/folder")).is_none());
    }
}
```

Add to `audio/mod.rs` after `pub mod transcript_md;`:

```rust
// spec 4.4: speaker labels from per-stream energy
pub mod speaker_energy;
```

Run: `cd frontend/src-tauri && cargo test --lib speaker_energy`
Expected: compile errors for the missing items.

- [ ] **Step 2: Implement**

Insert above the tests:

```rust
pub const WINDOW_MS: u32 = 50;
const NOISE_FLOOR: f32 = 0.1;
const ME_SHARE: f32 = 0.7;
const THEM_SHARE: f32 = 0.3;
const REFERENCE_FLOOR: f32 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    Me,
    Them,
    Both,
}

impl Speaker {
    pub fn as_str(self) -> &'static str {
        match self {
            Speaker::Me => "me",
            Speaker::Them => "them",
            Speaker::Both => "both",
        }
    }

    pub fn parse(s: &str) -> Option<Speaker> {
        match s {
            "me" => Some(Speaker::Me),
            "them" => Some(Speaker::Them),
            "both" => Some(Speaker::Both),
            _ => None,
        }
    }
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyTimeline {
    pub window_ms: u32,
    pub windows: Vec<(f32, f32)>,
    #[serde(skip)]
    max_mic: f32,
    #[serde(skip)]
    max_sys: f32,
}

impl Default for EnergyTimeline {
    fn default() -> Self {
        Self::new()
    }
}

impl EnergyTimeline {
    pub fn new() -> Self {
        Self { window_ms: WINDOW_MS, windows: Vec::new(), max_mic: 0.0, max_sys: 0.0 }
    }

    pub fn push(&mut self, mic_rms: f32, sys_rms: f32) {
        self.max_mic = self.max_mic.max(mic_rms);
        self.max_sys = self.max_sys.max(sys_rms);
        self.windows.push((mic_rms, sys_rms));
    }

    pub fn extend(&mut self, pairs: &[(f32, f32)]) {
        for &(m, s) in pairs {
            self.push(m, s);
        }
    }

    /// Cut one mixing window (both streams, same length) into 50 ms sub-windows and record each pair.
    /// Returns the pairs so the caller can forward them.
    pub fn push_window_pair(&mut self, mic: &[f32], sys: &[f32], sample_rate: u32) -> Vec<(f32, f32)> {
        let sub = (sample_rate as usize * self.window_ms as usize / 1000).max(1);
        let len = mic.len().max(sys.len());
        let mut out = Vec::with_capacity(len / sub + 1);
        let mut i = 0;
        while i < len {
            let end = (i + sub).min(len);
            let m = rms(&mic[i.min(mic.len())..end.min(mic.len())]);
            let s = rms(&sys[i.min(sys.len())..end.min(sys.len())]);
            self.push(m, s);
            out.push((m, s));
            i = end;
        }
        out
    }

    /// Peak so far per stream, for live labelling before the whole recording is known.
    pub fn running_references(&self) -> (f32, f32) {
        (self.max_mic.max(REFERENCE_FLOOR), self.max_sys.max(REFERENCE_FLOOR))
    }

    /// 95th percentile of the non-zero windows per stream, for the enhance pass.
    pub fn references(&self) -> (f32, f32) {
        fn p95(mut v: Vec<f32>) -> f32 {
            if v.is_empty() {
                return REFERENCE_FLOOR;
            }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let idx = ((v.len() as f32 - 1.0) * 0.95).round() as usize;
            v[idx.min(v.len() - 1)].max(REFERENCE_FLOOR)
        }
        let mic: Vec<f32> = self.windows.iter().map(|w| w.0).filter(|m| *m > 0.01).collect();
        let sys: Vec<f32> = self.windows.iter().map(|w| w.1).filter(|s| *s > 0.01).collect();
        (p95(mic), p95(sys))
    }

    pub fn has_speech(&self) -> bool {
        self.windows.iter().any(|(m, s)| *m > 0.01 || *s > 0.01)
    }

    fn index_range(&self, start_ms: f64, end_ms: f64) -> std::ops::Range<usize> {
        let w = self.window_ms as f64;
        let a = (start_ms / w).floor().max(0.0) as usize;
        let b = ((end_ms / w).ceil() as usize).min(self.windows.len());
        a.min(b)..b
    }

    /// Who spoke in [start_ms, end_ms): me, them or both; None when every window is noise.
    pub fn label_span(&self, start_ms: f64, end_ms: f64, refs: (f32, f32)) -> Option<Speaker> {
        let (mut me, mut them, mut counted) = (0.0f32, 0.0f32, 0usize);
        for &(m, s) in &self.windows[self.index_range(start_ms, end_ms)] {
            let (nm, ns) = (m / refs.0, s / refs.1);
            if nm.max(ns) < NOISE_FLOOR {
                continue;
            }
            me += nm;
            them += ns;
            counted += 1;
        }
        if counted == 0 || me + them <= 0.0 {
            return None;
        }
        let share = me / (me + them);
        Some(if share >= ME_SHARE { Speaker::Me } else if share <= THEM_SHARE { Speaker::Them } else { Speaker::Both })
    }

    /// Positions (ms, absolute) inside (start_ms, end_ms) where the dominant stream changes
    /// and the new run lasts at least `min_run_ms`. Noise windows inherit the previous dominant.
    pub fn turn_boundaries(&self, start_ms: f64, end_ms: f64, refs: (f32, f32), min_run_ms: u32) -> Vec<f64> {
        let range = self.index_range(start_ms, end_ms);
        if range.is_empty() {
            return Vec::new();
        }
        // Per-window dominant stream: true = me
        let mut dominant: Vec<Option<bool>> = Vec::with_capacity(range.len());
        let mut last: Option<bool> = None;
        for &(m, s) in &self.windows[range.clone()] {
            let (nm, ns) = (m / refs.0, s / refs.1);
            if nm.max(ns) >= NOISE_FLOOR {
                last = Some(nm >= ns);
            }
            dominant.push(last);
        }
        // Run-length encode, skipping leading unknowns
        let mut runs: Vec<(bool, usize)> = Vec::new();
        for d in dominant.into_iter().flatten() {
            match runs.last_mut() {
                Some((v, n)) if *v == d => *n += 1,
                _ => runs.push((d, 1)),
            }
        }
        // Absorb short runs into the previous run (or the next one when first)
        let min_run = ((min_run_ms / self.window_ms).max(1)) as usize;
        let mut merged: Vec<(bool, usize)> = Vec::new();
        for (v, n) in runs {
            if n < min_run {
                match merged.last_mut() {
                    Some((_, pn)) => *pn += n,
                    None => merged.push((v, n)), // leading short run: keep, may merge forward below
                }
            } else {
                match merged.last_mut() {
                    Some((pv, pn)) if *pv == v => *pn += n,
                    Some((_, pn)) if *pn < min_run => {
                        // previous was a short leading run: fold it into this one
                        let total = *pn + n;
                        merged.pop();
                        merged.push((v, total));
                    }
                    _ => merged.push((v, n)),
                }
            }
        }
        let w = self.window_ms as f64;
        let mut boundaries = Vec::new();
        let mut offset = range.start;
        for (i, (_, n)) in merged.iter().enumerate() {
            offset += n;
            if i + 1 < merged.len() {
                let ms = offset as f64 * w;
                if ms > start_ms && ms < end_ms {
                    boundaries.push(ms);
                }
            }
        }
        boundaries
    }

    /// Write speakers.json atomically.
    pub fn save(&self, folder: &Path) -> Result<()> {
        let json = serde_json::json!({
            "version": 1,
            "window_ms": self.window_ms,
            "streams": ["me", "them"],
            "windows": self.windows.iter().map(|(m, s)| {
                serde_json::json!([(m * 100000.0).round() / 100000.0, (s * 100000.0).round() / 100000.0])
            }).collect::<Vec<_>>(),
        });
        let tmp = folder.join(".speakers.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&json)?)?;
        std::fs::rename(&tmp, folder.join("speakers.json"))?;
        Ok(())
    }

    /// Read speakers.json; None when missing or unreadable.
    pub fn load(folder: &Path) -> Option<EnergyTimeline> {
        let text = std::fs::read_to_string(folder.join("speakers.json")).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let window_ms = v.get("window_ms")?.as_u64()? as u32;
        let mut t = EnergyTimeline { window_ms, ..EnergyTimeline::new() };
        for w in v.get("windows")?.as_array()? {
            let a = w.as_array()?;
            t.push(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32);
        }
        Some(t)
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib speaker_energy`
Expected: 7 passed. The `turn_boundaries` test expects exactly `[4500.0]`: 40 me, 10 them (absorbed into the previous me run, 50 windows), 40 me (merged with the previous me run because same value: 90 windows), then 40 them; the single boundary is at window 90. If your merge logic yields `[2000.0, 2500.0, 4500.0]`, the short-run absorption is wrong; fix the implementation, not the test.

- [ ] **Step 4: Commit**

```bash
git add frontend/src-tauri/src/audio/speaker_energy.rs frontend/src-tauri/src/audio/mod.rs
git commit -m "feat(speakers): energy timeline, labelling and turn boundaries"
```

---

### Task 2: Preference `owner_name`

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs`
- Modify: `frontend/src/components/RecordingSettings.tsx` (interface, initial state, one input)

**Interfaces:**
- Produces: `RecordingPreferences.owner_name: String` (default `"Me"`); TS `owner_name: string`.

- [ ] **Step 1: Rust**

Add to the struct after `auto_start_on_call`:

```rust
    // spec 4.4: how the owner is named in transcripts ("me" segments)
    #[serde(default = "default_owner_name")]
    pub owner_name: String,
```

with `fn default_owner_name() -> String { "Me".to_string() }` next to the other default helpers and `owner_name: default_owner_name(),` in `impl Default`. Add a test:

```rust
    #[test]
    fn owner_name_defaults_to_me() {
        let old = r#"{"save_folder":"/tmp/x","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(old).unwrap();
        assert_eq!(p.owner_name, "Me");
    }
```

Run: `cd frontend/src-tauri && cargo test --lib recording_preferences` (4 passed).

- [ ] **Step 2: TypeScript**

Interface: `owner_name: string; // spec 4.4`. Initial state: `owner_name: 'Me',`. Add `import { Input } from '@/components/ui/input';`. Add a handler next to `handleAutoStopToggle`:

```ts
  // spec 4.4
  const [ownerDraft, setOwnerDraft] = useState<string | null>(null);
  const commitOwnerName = async () => {
    const name = (ownerDraft ?? preferences.owner_name).trim() || 'Me';
    setOwnerDraft(null);
    if (name === preferences.owner_name) return;
    const next = { ...preferences, owner_name: name };
    setPreferences(next);
    await savePreferences(next);
  };
```

JSX, directly after the `{/* spec 4.5: Zoom call detection */}` block:

```tsx
      {/* spec 4.4: speaker labels */}
      <div className="p-4 border rounded-lg flex items-center justify-between gap-4">
        <div className="flex-1">
          <div className="font-medium">Your name in transcripts</div>
          <div className="text-sm text-gray-600">
            Lines from your microphone are labelled with this name; everyone else is "Others"
          </div>
        </div>
        <Input
          className="w-44"
          value={ownerDraft ?? preferences.owner_name}
          onChange={(e) => setOwnerDraft(e.target.value)}
          onBlur={commitOwnerName}
          onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur(); }}
          disabled={saving}
        />
      </div>
```

Run: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json` (silent).

- [ ] **Step 3: Commit**

```bash
git add frontend/src-tauri/src/audio/recording_preferences.rs frontend/src/components/RecordingSettings.tsx
git commit -m "feat(speakers): owner_name preference and settings input"
```

---

### Task 3: Labels on the live path

**Files:**
- Modify: `frontend/src-tauri/src/audio/recording_state.rs` (`AudioChunk`)
- Modify: `frontend/src-tauri/src/audio/pipeline.rs` (struct, constructor, mixing loop, `AudioPipelineManager::start`, every `AudioChunk {` literal)
- Modify: `frontend/src-tauri/src/audio/transcription/worker.rs` (`TranscriptUpdate`, its construction)
- Modify: `frontend/src-tauri/src/audio/recording_commands.rs` (the `transcript-update` listener's `TranscriptSegment` literal, both start functions read `owner_name`)
- Modify: `frontend/src-tauri/src/audio/recording_saver.rs` (`TranscriptSegment.speaker`, `speaker_windows`, `start_energy_accumulation`, `set_owner_name`)
- Modify: `frontend/src-tauri/src/audio/recording_manager.rs` (energy channel, pass-throughs)
- Modify: `frontend/src-tauri/src/audio/incremental_saver.rs`, `frontend/src-tauri/src/whisper_engine/parallel_commands.rs` (`speaker: None` in their `AudioChunk` literals)

**Interfaces:**
- Consumes: `speaker_energy::{EnergyTimeline, Speaker, WINDOW_MS}`.
- Produces: `AudioChunk.speaker: Option<String>`; `TranscriptUpdate.speaker: Option<String>`; saver `TranscriptSegment.speaker: Option<String>` (serde default); `RecordingSaver::start_energy_accumulation(&mut self, receiver: mpsc::UnboundedReceiver<Vec<(f32, f32)>>)`, `RecordingSaver::set_owner_name(&mut self, name: String)`, `RecordingSaver::take_timeline(&self) -> EnergyTimeline` (used by Task 4); `AudioPipelineManager::start(..., energy_sender: Option<mpsc::UnboundedSender<Vec<(f32, f32)>>>, ...)` (new parameter right after `recording_sender`).

- [ ] **Step 1: `AudioChunk.speaker`**

In `recording_state.rs`:

```rust
pub struct AudioChunk {
    pub data: Vec<f32>,
    pub sample_rate: u32,
    pub timestamp: f64,
    pub chunk_id: u64,
    pub device_type: DeviceType,
    /// spec 4.4: "me", "them" or "both" for VAD segments sent to transcription; None elsewhere
    pub speaker: Option<String>,
}
```

Add `speaker: None,` to every `AudioChunk {` literal the compiler reports (pipeline.rs has several including the recording chunk and the capture chunk, `incremental_saver.rs`, `whisper_engine/parallel_commands.rs`; `lib_old_complex.rs` is not compiled and stays untouched).

- [ ] **Step 2: Pipeline timeline and live labels**

In `pipeline.rs`, add fields to `AudioPipeline`:

```rust
    // spec 4.4: per-stream energy before mixing, for speaker labels
    timeline: super::speaker_energy::EnergyTimeline,
    energy_sender: Option<mpsc::UnboundedSender<Vec<(f32, f32)>>>,
```

initialize them in `new` (`timeline: super::speaker_energy::EnergyTimeline::new(), energy_sender: None,`). In the mixing loop, right after `if let Some((mic_window, sys_window)) = self.ring_buffer.extract_window() {` and before `mix_window`, add:

```rust
                            // spec 4.4: record who is loud while the streams are still separate
                            let pairs = self.timeline.push_window_pair(&mic_window, &sys_window, self.sample_rate);
                            if let Some(sender) = &self.energy_sender {
                                let _ = sender.send(pairs);
                            }
```

In the VAD segment branch, compute the label before building the transcription chunk and set it:

```rust
                                            let speaker = self
                                                .timeline
                                                .label_span(segment.start_timestamp_ms, segment.end_timestamp_ms, self.timeline.running_references())
                                                .map(|s| s.as_str().to_string());
```

and `speaker,` in that `AudioChunk { ... }` literal (the recording chunk keeps `speaker: None`).

In `AudioPipelineManager::start`, add the parameter `energy_sender: Option<mpsc::UnboundedSender<Vec<(f32, f32)>>>,` directly after `recording_sender`, and after `pipeline.recording_sender_for_mixed = recording_sender;` add `pipeline.energy_sender = energy_sender; // spec 4.4`. Update every caller (`recording_manager.rs` is the one; pass the sender created in Step 4).

- [ ] **Step 3: Worker**

In `worker.rs`, add `pub speaker: Option<String>,` to `TranscriptUpdate` (with a `// spec 4.4` comment) and capture the chunk's label where `chunk_timestamp` is captured from the chunk (`let chunk_speaker = chunk.speaker.clone();` near the other `let chunk_... = chunk.…` lines) and set `speaker: chunk_speaker.clone(),` in the `TranscriptUpdate { ... }` literal. If the chunk is moved before that point, clone the field first.

- [ ] **Step 4: Saver, manager, commands**

`recording_saver.rs`:
- `TranscriptSegment` gains `#[serde(default)] pub speaker: Option<String>,` (spec 4.4).
- `RecordingSaver` gains `speaker_windows: Arc<Mutex<Vec<(f32, f32)>>>` (init `Arc::new(Mutex::new(Vec::new()))`) and `owner_name: String` (init `"Me".to_string()`).
- Add:

```rust
    /// spec 4.4: collect the per-window energies streamed by the pipeline
    pub fn start_energy_accumulation(&mut self, mut receiver: mpsc::UnboundedReceiver<Vec<(f32, f32)>>) {
        let windows = self.speaker_windows.clone();
        tokio::spawn(async move {
            while let Some(pairs) = receiver.recv().await {
                if let Ok(mut w) = windows.lock() {
                    w.extend(pairs);
                }
            }
        });
    }

    pub fn set_owner_name(&mut self, name: String) {
        self.owner_name = if name.trim().is_empty() { "Me".to_string() } else { name };
    }

    /// The timeline collected so far (a copy).
    pub fn take_timeline(&self) -> super::speaker_energy::EnergyTimeline {
        let mut t = super::speaker_energy::EnergyTimeline::new();
        if let Ok(w) = self.speaker_windows.lock() {
            t.extend(&w);
        }
        t
    }
```

`recording_manager.rs`, in `start_recording`: create `let (energy_sender, energy_receiver) = mpsc::unbounded_channel::<Vec<(f32, f32)>>();` next to the other channels, pass `Some(energy_sender)` as the new argument to `self.pipeline_manager.start(...)`, and call `self.recording_saver.start_energy_accumulation(energy_receiver);` right after `start_accumulation`. Add pass-throughs `set_owner_name(&mut self, name: String)` and `take_timeline(&self)`.

`recording_commands.rs`: in both start functions, where the transcript config is read (the `// spec 4.2` block), also `manager.set_owner_name(prefs_owner_name)`; the preferences are already loaded earlier in each function as `prefs`/the destructured tuple; read `owner_name` from that same `load_recording_preferences` result (extend the destructuring or keep the whole struct). In the `transcript-update` listener, add `speaker: update.speaker.clone(),` to the `TranscriptSegment { ... }` literal.

- [ ] **Step 5: Verify and commit**

Run: `cd frontend/src-tauri && cargo check 2>&1 | grep -E "^error|^warning: unused" ; cargo test --lib speaker_energy && cargo test --lib retranscription`
Expected: no errors, no new warnings, suites green (the existing `create_transcript_segments` tests are untouched until Task 5).

```bash
git add frontend/src-tauri/src/audio/recording_state.rs frontend/src-tauri/src/audio/pipeline.rs frontend/src-tauri/src/audio/transcription/worker.rs frontend/src-tauri/src/audio/recording_commands.rs frontend/src-tauri/src/audio/recording_saver.rs frontend/src-tauri/src/audio/recording_manager.rs frontend/src-tauri/src/audio/incremental_saver.rs frontend/src-tauri/src/whisper_engine/parallel_commands.rs
git commit -m "feat(speakers): label live segments from pre-mix energy and stream the timeline to the saver"
```

---

### Task 4: `speakers.json`, metadata keys, labelled `transcript.md`

**Files:**
- Modify: `frontend/src-tauri/src/audio/transcript_md.rs` (`MdSegment`, `TranscriptMdMeta.speaker_names`, renderer, tests)
- Modify: `frontend/src-tauri/src/audio/recording_saver.rs` (`MeetingMetadata.speakers`, `speakers_file`; `stop_and_save`)

**Interfaces:**
- Produces: `pub struct MdSegment { pub start: f64, pub speaker: Option<String>, pub text: String }`; `TranscriptMdMeta` gains `pub speaker_names: Option<(&'a str, &'a str)>` (me, them); `render_transcript_md(meta, segments: &[MdSegment])`, `write_transcript_md(folder, meta, segments: &[MdSegment])`. Task 5 uses the same.

- [ ] **Step 1: Failing test**

Add to `transcript_md.rs` tests:

```rust
    #[test]
    fn labelled_lines_carry_names_and_frontmatter_lists_speakers() {
        let meta = TranscriptMdMeta {
            title: "t", project: "dex", meeting_id: "m", started_at: "2026-10-27T12:03:09-04:00",
            duration_seconds: None, quality: "enhanced", model: "whisper large-v3",
            speaker_names: Some(("Alexandre", "Others")),
        };
        let segments = vec![
            MdSegment { start: 0.0, speaker: Some("me".into()), text: "Hello.".into() },
            MdSegment { start: 4.0, speaker: Some("them".into()), text: "Hi there.".into() },
            MdSegment { start: 9.0, speaker: Some("both".into()), text: "Yes yes.".into() },
            MdSegment { start: 12.0, speaker: None, text: "Unknown.".into() },
        ];
        let md = render_transcript_md(&meta, &segments);
        assert!(md.contains("model: whisper large-v3\nspeakers: me=Alexandre, them=Others\n---\n"), "{}", md);
        assert!(md.ends_with("[00:00] Alexandre: Hello.\n[00:04] Others: Hi there.\n[00:09] Yes yes.\n[00:12] Unknown.\n"), "{}", md);
    }
```

Update the three existing tests to the new types: build `MdSegment { start, speaker: None, text }` and add `speaker_names: None` to their metas; their expected strings do not change (no `speakers` line when `speaker_names` is `None`).

Run: `cd frontend/src-tauri && cargo test --lib transcript_md` and expect compile errors.

- [ ] **Step 2: Implement**

```rust
pub struct MdSegment {
    pub start: f64,
    /// "me", "them", "both" or None
    pub speaker: Option<String>,
    pub text: String,
}
```

Add to `TranscriptMdMeta`: `/// spec 4.4: (name for "me", name for "them"); None when the meeting has no labels` `pub speaker_names: Option<(&'a str, &'a str)>,`. In `render_transcript_md`, after the `model:` line: `if let Some((me, them)) = meta.speaker_names { out.push_str(&format!("speakers: me={}, them={}\n", me, them)); }`. Change the loop to:

```rust
    for seg in segments {
        let one_line = seg.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if one_line.is_empty() {
            continue;
        }
        let name = match (seg.speaker.as_deref(), meta.speaker_names) {
            (Some("me"), Some((me, _))) => Some(me),
            (Some("them"), Some((_, them))) => Some(them),
            _ => None,
        };
        match name {
            Some(n) => out.push_str(&format!("[{}] {}: {}\n", format_mmss(seg.start), n, one_line)),
            None => out.push_str(&format!("[{}] {}\n", format_mmss(seg.start), one_line)),
        }
    }
```

`write_transcript_md` takes `&[MdSegment]`. Update the Phase 2 callers now so the crate compiles: in `recording_saver.rs` `stop_and_save`, build `Vec<MdSegment>` from the saver's segments (`start: t.audio_start_time, speaker: t.speaker.clone(), text: t.text.clone()`), and pass `speaker_names: Some((owner_name.as_str(), "Others"))` when any segment has a label, else `None` (`let owner_name = self.owner_name.clone();` before the block). In `retranscription.rs`, map the segments the same way for now (`speaker: None`; Task 5 fills it) with `speaker_names: None`.

- [ ] **Step 3: `speakers.json` and metadata**

`MeetingMetadata` gains:

```rust
    // spec 4.4
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speakers: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speakers_file: Option<String>,
```

(`None` in the initial literal.) In `stop_and_save`, inside the completed-metadata block before `write_metadata`:

```rust
            // spec 4.4: persist the energy timeline and name the speakers
            let timeline = self.take_timeline();
            if timeline.has_speech() {
                match timeline.save(folder) {
                    Ok(()) => metadata.speakers_file = Some("speakers.json".to_string()),
                    Err(e) => warn!("Failed to write speakers.json: {}", e),
                }
            }
            metadata.speakers = Some(serde_json::json!({ "me": self.owner_name, "them": "Others" }));
```

Then the transcript.md block (already in that scope) uses the labelled segments and names from Step 2. Make sure `self.metadata = Some(metadata.clone())` is not required (the block uses its local `metadata`).

- [ ] **Step 4: Verify and commit**

Run: `cd frontend/src-tauri && cargo test --lib transcript_md && cargo check 2>&1 | grep -E "^error|^warning: unused"`
Expected: 6 transcript_md tests pass; no errors or new warnings.

```bash
git add frontend/src-tauri/src/audio/transcript_md.rs frontend/src-tauri/src/audio/recording_saver.rs frontend/src-tauri/src/audio/retranscription.rs
git commit -m "feat(speakers): speakers.json, metadata speaker names, labelled transcript.md"
```

---

### Task 5: Enhance pass splits at speaker turns and labels every segment

**Files:**
- Modify: `frontend/src-tauri/src/audio/common.rs` (labelled helper, `speaker` in `transcripts.json`)
- Modify: `frontend/src-tauri/src/api/api.rs` (`TranscriptSegment.speaker`), `frontend/src-tauri/src/database/repositories/transcript.rs` (INSERT)
- Modify: `frontend/src-tauri/src/audio/retranscription.rs` (splitting, labelling, INSERT, transcript.md names)

**Interfaces:**
- Consumes: `EnergyTimeline::{load, references, turn_boundaries, label_span}`, `MdSegment`, `Speaker`.
- Produces: `common::create_labelled_transcript_segments(&[(String, f64, f64, Option<String>)]) -> Vec<TranscriptSegment>` (the existing `create_transcript_segments` delegates to it with `None` labels); API `TranscriptSegment.speaker: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`); `transcripts.json` segments include `"speaker"`; the `transcripts` INSERTs (live save and enhance) write the `speaker` column.

- [ ] **Step 1: Failing tests**

In `common.rs` (or the retranscription tests module, which already tests `create_transcript_segments`), add:

```rust
    #[test]
    fn labelled_segments_keep_their_speaker() {
        let segs = create_labelled_transcript_segments(&[
            ("Hello".to_string(), 0.0, 1000.0, Some("me".to_string())),
            ("Hi".to_string(), 1000.0, 2000.0, None),
        ]);
        assert_eq!(segs[0].speaker.as_deref(), Some("me"));
        assert_eq!(segs[1].speaker, None);
    }
```

And a pure splitting test in `retranscription.rs` tests, for the helper you will add:

```rust
    #[test]
    fn split_at_boundaries_cuts_samples_and_timestamps() {
        let seg = crate::audio::vad::SpeechSegment {
            samples: vec![0.1; 16000 * 3], // 3 s at 16 kHz
            start_timestamp_ms: 10_000.0,
            end_timestamp_ms: 13_000.0,
            confidence: 0.9,
        };
        let parts = split_at_boundaries(&seg, &[11_000.0, 12_000.0]);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].samples.len(), 16000);
        assert_eq!(parts[1].start_timestamp_ms, 11_000.0);
        assert_eq!(parts[2].end_timestamp_ms, 13_000.0);
        assert_eq!(split_at_boundaries(&seg, &[]).len(), 1);
        assert_eq!(split_at_boundaries(&seg, &[10_000.0, 13_000.0, 500.0]).len(), 1, "boundaries at the edges or outside are ignored");
    }
```

Run both module suites; expect compile errors.

- [ ] **Step 2: `common.rs`**

```rust
/// spec 4.4: segments with an optional speaker label ("me", "them", "both")
pub(crate) fn create_labelled_transcript_segments(transcripts: &[(String, f64, f64, Option<String>)]) -> Vec<TranscriptSegment> {
    transcripts
        .iter()
        .map(|(text, start_ms, end_ms, speaker)| {
            let start_seconds = start_ms / 1000.0;
            let end_seconds = end_ms / 1000.0;
            TranscriptSegment {
                id: format!("transcript-{}", Uuid::new_v4()),
                text: text.trim().to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                audio_start_time: Some(start_seconds),
                audio_end_time: Some(end_seconds),
                duration: Some(end_seconds - start_seconds),
                speaker: speaker.clone(),
            }
        })
        .collect()
}
```

Make `create_transcript_segments` delegate: map each `(text, s, e)` to `(text.clone(), *s, *e, None)` and call the new function. In `write_transcripts_json`, add `"speaker": s.speaker,` to the per-segment `json!`.

In `api/api.rs`, add to `TranscriptSegment`:

```rust
    /// spec 4.4: "me", "them", "both" or absent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
```

(`MeetingTranscript` is unchanged in this phase.) In `database/repositories/transcript.rs`, the INSERT becomes `INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker) VALUES (?, ?, ?, ?, ?, ?, ?, ?)` with `.bind(&segment.speaker)` last. Grep for other constructions of `api::TranscriptSegment` (import.rs builds them through `create_transcript_segments`, so it needs nothing; any literal gets `speaker: None`).

- [ ] **Step 3: Enhance pass**

In `retranscription.rs`:

1. Add the pure helper (above the tests, near `find_audio_file`):

```rust
/// spec 4.4: cut a VAD segment at absolute millisecond positions (16 kHz samples).
/// Boundaries at or outside the segment edges are ignored.
fn split_at_boundaries(segment: &crate::audio::vad::SpeechSegment, boundaries_ms: &[f64]) -> Vec<crate::audio::vad::SpeechSegment> {
    const SR: f64 = 16000.0;
    let mut cuts: Vec<f64> = boundaries_ms
        .iter()
        .copied()
        .filter(|b| *b > segment.start_timestamp_ms && *b < segment.end_timestamp_ms)
        .collect();
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    cuts.dedup();
    if cuts.is_empty() {
        return vec![segment.clone()];
    }
    let mut parts = Vec::with_capacity(cuts.len() + 1);
    let mut prev_ms = segment.start_timestamp_ms;
    let mut prev_idx = 0usize;
    for cut in cuts.into_iter().chain(std::iter::once(segment.end_timestamp_ms)) {
        let idx = (((cut - segment.start_timestamp_ms) / 1000.0) * SR).round() as usize;
        let idx = idx.min(segment.samples.len()).max(prev_idx);
        if idx > prev_idx {
            parts.push(crate::audio::vad::SpeechSegment {
                samples: segment.samples[prev_idx..idx].to_vec(),
                start_timestamp_ms: prev_ms,
                end_timestamp_ms: cut,
                confidence: segment.confidence,
            });
        }
        prev_ms = cut;
        prev_idx = idx;
    }
    parts
}
```

2. After `processable_segments` is built (the silence split loop) and before the transcription loop, add:

```rust
    // spec 4.4: split at speaker turns and remember who spoke, when the recording has an energy timeline
    let timeline = super::speaker_energy::EnergyTimeline::load(&folder_path);
    let refs = timeline.as_ref().map(|t| t.references());
    let mut labelled_segments: Vec<(crate::audio::vad::SpeechSegment, Option<String>)> = Vec::new();
    for segment in processable_segments {
        match (&timeline, refs) {
            (Some(t), Some(r)) => {
                let cuts = t.turn_boundaries(segment.start_timestamp_ms, segment.end_timestamp_ms, r, 1000);
                for part in split_at_boundaries(&segment, &cuts) {
                    let label = t.label_span(part.start_timestamp_ms, part.end_timestamp_ms, r).map(|s| s.as_str().to_string());
                    labelled_segments.push((part, label));
                }
            }
            _ => labelled_segments.push((segment, None)),
        }
    }
    let processable_count = labelled_segments.len();
    info!("Processing {} segments (after silence and speaker-turn splitting)", processable_count);
```

Remove the earlier `let processable_count = processable_segments.len();` and its log line. Change the transcription loop to iterate `labelled_segments.iter().enumerate()` with `(segment, speaker)` and push `(text, start_ms, end_ms, speaker.clone())` into `all_transcripts: Vec<(String, f64, f64, Option<String>)>`. Keep the short-segment skip and the confidence accounting as they are.

3. Replace `create_transcript_segments(&all_transcripts)` with `create_labelled_transcript_segments(&all_transcripts)` (import it), extend the INSERT to the eight-column form with `.bind(&segment.speaker)`, and in the transcript.md block build `MdSegment { start, speaker: s.speaker.clone(), text }` with `speaker_names` from metadata: `meta_json["speakers"]["me"]` and `["them"]` as `&str` (fallback `("Me", "Others")`) when any segment is labelled, else `None`.

- [ ] **Step 4: Verify and commit**

Run: `cd frontend/src-tauri && cargo test --lib retranscription && cargo test --lib common && cargo test --lib import && cargo check 2>&1 | grep -E "^error|^warning: unused"`
Expected: all green (retranscription 18 or more, import unchanged), no errors or new warnings.

```bash
git add frontend/src-tauri/src/audio/common.rs frontend/src-tauri/src/api/api.rs frontend/src-tauri/src/database/repositories/transcript.rs frontend/src-tauri/src/audio/retranscription.rs
git commit -m "feat(speakers): enhance pass splits at speaker turns and labels segments"
```

---

### Task 6: Frontend types carry the label

**Files:**
- Modify: `frontend/src/types/index.ts` (`Transcript`, `TranscriptUpdate`)
- Modify: `frontend/src/contexts/TranscriptContext.tsx` (copy `speaker` when building the live `Transcript`)

- [ ] **Step 1: Types**

Add `speaker?: string | null; // spec 4.4: "me" | "them" | "both"` to both `Transcript` and `TranscriptUpdate`. In `TranscriptContext.tsx`, in the `newTranscript: Transcript = { ... }` literal built from the update, add `speaker: update.speaker ?? null,`.

The live transcripts are saved through `api_save_transcript` as JSON values, so the new field reaches the Rust `TranscriptSegment` and the `speaker` column with no further change.

- [ ] **Step 2: Verify and commit**

Run: `cd frontend && node_modules/.bin/tsc --noEmit -p tsconfig.json` (silent).

```bash
git add frontend/src/types/index.ts frontend/src/contexts/TranscriptContext.tsx
git commit -m "feat(speakers): speaker label on live transcript objects"
```

---

### Task 7: Manual acceptance (owner)

- [ ] **Step 1:** Set "Your name in transcripts" to your first name in Settings. Restart the dev app if it did not rebuild on its own.
- [ ] **Step 2:** Start a Zoom test meeting with a second participant (phone, or a colleague), or play a spoken YouTube video as the "other side". Record about one minute where you speak, then they speak, then you again. Stop (or let the call end).
- [ ] **Step 3:** Expected in the meeting folder: `speakers.json` exists; `metadata.json` has `speakers` and `speakers_file`; `transcripts.json` segments carry `speaker` values; after the enhance job, `transcript.md` has `speakers: me=<name>, them=Others` in the frontmatter and lines like `[00:03] <name>: ...` and `[00:20] Others: ...`, with a line break where the speaker changed.
- [ ] **Step 4:** Read it from a wiki with `get_transcript`: the labels are present as plain text.
- [ ] **Step 5:** Record 20 seconds alone with no system audio playing. Expected: every labelled line is yours; nothing is labelled `Others`.
- [ ] **Step 6:** Finish with `superpowers:finishing-a-development-branch` (squash merge to `main`).

---

## Self-review against the spec

- 4.4 live labels from the pre-mix windows with a running reference and the 70/30 rule: Tasks 1 and 3. Timeline persisted as `speakers.json`: Task 4. Enhance splits at turns of at least one second and labels pieces: Task 5. Names from a preference, `Others` for the rest: Tasks 2 and 4.
- 5.2 `speakers` and `speakers_file` keys: Task 4.
- 5.3 `speakers:` frontmatter line and labelled lines, bare for `both` and unknown: Task 4, with a byte-exact test; Phase 2's unlabelled test keeps passing.
- `transcripts.speaker` column finally written: Task 5 (both INSERTs).
- Deferred by design: a visible speaker prefix in the app's transcript view; full multi-speaker diarization (spec 4.4 names `sherpa-rs` as the later upgrade).
- Names used across tasks: `Speaker`, `EnergyTimeline`, `WINDOW_MS`, `rms`, `push_window_pair`, `running_references`, `references`, `label_span`, `turn_boundaries`, `save`, `load`, `has_speech`, `take_timeline`, `start_energy_accumulation`, `set_owner_name`, `owner_name`, `MdSegment`, `speaker_names`, `create_labelled_transcript_segments`, `split_at_boundaries`.

## Outcome

Accepted by the owner on 2026-10-27 on a real recording (labels live, `speakers.json`, `transcript.md` with names after enhance) and squash-merged into `main`. Review fixes folded in: reference floor raised to 0.01 so a faint never-silent stream cannot steal labels; leading-noise offset in turn boundaries; f64 rounding and compact JSON for `speakers.json`, written whenever the timeline has windows; last split piece keeps every sample; owner name collapsed to one line; recovery save and reload sync keep the label; MCP fallback renderer prints the same labelled markdown as Rust.

Deferred follow-ups (none blocks daily use):
- No visible speaker prefix in the app's transcript view yet; the database read model (`MeetingTranscript`) has no `speaker` field. Labels live in `transcript.md`, `transcripts.json` and the `transcripts.speaker` column.
- Live labels use running peak references, so the first seconds of a recording can be mislabelled until both streams have peaked; the enhance pass relabels with 95th-percentile references and corrects this.
- Remote speech that never exceeds 0.01 RMS for a whole meeting is scored against the floor and can come out as `both` when the microphone also hears it.
- `split_at_boundaries` deep-clones a segment when there is nothing to cut.
- `EnergyTimeline::save` leaves `.speakers.json.tmp` behind if the write fails and does no fsync before the rename.
- Rust counts any non-null label as "labelled" while the MCP renderer counts only me/them/both; a ranged MCP request decides the `speakers:` line from labels inside the range only; the MCP `text` format carries no names.
- The ring-buffer comment in `pipeline.rs` says 50 ms while the mixing window is 600 ms (sub-windows are 50 ms, labels unaffected).
- Full multi-speaker diarization (for example `sherpa-rs`) stays a later upgrade, as the spec notes.
