# Speaker Diarization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After an enhance, remote speech is split into numbered speakers (`s1`, `s2`, ...) that the user can rename per meeting, in the app, in transcript.md and through the MCP server.

**Architecture:** sherpa-onnx's command-line diarization tool runs as a Tauri sidecar inside the enhance job, on the already decoded 16 kHz samples with the owner's windows silenced. Its clusters cut and label the remote pieces before Whisper runs. Names live in `metadata.json` `speakers` as a tag-to-name map; one renderer in Rust and its mirror in the MCP server read that map.

**Tech Stack:** Rust (Tauri 2, tokio, reqwest 0.11, tokio-util CancellationToken, serde_json), Next.js 14 / React 18 / TypeScript, Node test runner, sherpa-onnx v2.0.1 binaries, ONNX models from Hugging Face and GitHub releases.

Spec: `docs/superpowers/specs/2026-10-29-speaker-diarization-design.md`. Read it once before Task 1.

## Global Constraints

- Sidecar: `sherpa-onnx-offline-speaker-diarization` from sherpa-onnx release `v2.0.1`, archive `sherpa-onnx-v2.0.1-osx-arm64-static.tar.bz2`, path `bin/sherpa-onnx-offline-speaker-diarization` inside it, installed as `frontend/src-tauri/binaries/sherpa-onnx-offline-speaker-diarization-aarch64-apple-darwin` (gitignored, like `llama-helper`).
- Sidecar flags: `--segmentation.pyannote-model=`, `--embedding.model=`, `--clustering.num-clusters=-1`, `--clustering.cluster-threshold=0.5`, `--segmentation.num-threads=<N>`, `--embedding.num-threads=<N>`, then the WAV path. `N = available_parallelism() - 1`, clamped to `1..=8`.
- Sidecar output: stdout lines `0.031 -- 5.120 speaker_00`; stderr lines `progress 42.00%`. Non-zero exit is a failure; its last non-progress stderr line is the error text.
- Models directory: `<app data dir>/models/speakers/` holding `segmentation.onnx` (5,992,913 bytes, from `https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx`) and `embedding.onnx` (29,596,978 bytes, from `https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx`). The tag `speaker-recongition-models` is spelled that way upstream.
- Audio handed to the sidecar: 16 kHz mono 16-bit PCM WAV at `<meeting folder>/.speakers.wav`, deleted on every exit path. Windows that `EnergyTimeline::label_span` marks `Me` are written as silence; `Them`, `Both` and noise windows are kept.
- Clusters with under 5.0 s of total speech are dropped. Remaining clusters are renumbered `s1`, `s2`, ... by the start of their earliest segment. Remote pieces are cut at speaker changes; runs under 1000 ms are absorbed into the neighbouring run. A piece with no overlapping cluster stays `them`. `me` and `both` pieces are untouched.
- Segment tag set: `me`, `them`, `both`, `s<N>` (N >= 1). Readers treat any other value like `null`.
- `metadata.json` `speakers`: a map from tag to name. Defaults when missing: `Me`, `Others`, `Speaker N`. An enhance keeps `me` and `them` and resets every `s*` entry to its default. `both` never has a name.
- transcript.md front matter: `speakers: me=Alexandre, them=Others, s1=Speaker 1, s2=Marie` in the order `me`, `them`, `s1`, `s2`, ...; only when at least one segment carries a tag. Lines read `[mm:ss] Marie: text`; `both` and unknown tags stay bare.
- `rename_speaker(meeting_id, tag, name)`: tag matches `^(me|them|s[1-9][0-9]*)$`; name trimmed, 1 to 60 characters, no line breaks. Updates the map, rewrites transcript.md from the stored segments, emits `meeting-speakers-changed { meeting_id }`.
- Jobs panel: step `Speakers` before `Transcribe`; the job marks `Transcribe` running only after the Speakers step ends. Cancel during Speakers kills the sidecar and ends the job Cancelled. A sidecar failure marks the step Failed with the error's first line and the enhance continues with `them` labels.
- No new preference. Models present and sidecar present means the step runs.
- Verification commands: `cd frontend/src-tauri && cargo test --lib <filter>` (one filter per run), `cd frontend && ./node_modules/.bin/tsc --noEmit -p tsconfig.json`, `cd frontend && pnpm test`, `cd mcp && npm test`. Use `/usr/bin/git` for git.
- Never run `npm publish`, `claude mcp add`, or push to a remote. The owner's dev app (`tauri:dev`) hot-rebuilds from this working tree; do not stop it.

---

## File structure

| File | Responsibility |
|---|---|
| `scripts/fetch-diarizer.sh` (new) | Download the pinned sherpa-onnx archive, extract the one binary into `binaries/`. |
| `frontend/src-tauri/tauri.conf.json` | `externalBin` entry for the sidecar. |
| `.github/workflows/build-macos.yml` | CI step that runs the fetch script. |
| `frontend/src-tauri/src/audio/diarization.rs` (new) | Owner mask, masked WAV writer, sidecar spawn/parse/cancel, cluster numbering, speaker cuts, labelling. |
| `frontend/src-tauri/src/audio/speaker_models.rs` (new) | Model file specs, installed check, download with resume and cancel, delete, status; Tauri commands. |
| `frontend/src-tauri/src/audio/speaker_names.rs` (new) | Tag-to-name map helpers, `rename_speaker` command, transcript.md rewrite from the database. |
| `frontend/src-tauri/src/audio/transcript_md.rs` | `speaker_names` becomes an ordered list of (tag, name). |
| `frontend/src-tauri/src/audio/recording_saver.rs` | Live writer passes the list form. |
| `frontend/src-tauri/src/audio/retranscription.rs` | Speakers step, remote cuts and labels, names map reset, `job_id` plumbing. |
| `frontend/src-tauri/src/jobs.rs` | `STEP_SPEAKERS`, `Control::SpeakersDownload`, cancel and retry branches. |
| `frontend/src-tauri/src/audio/audio_cleanup.rs` | `get_meeting_extras` returns the `speakers` map. |
| `frontend/src-tauri/src/audio/mod.rs`, `frontend/src-tauri/src/lib.rs` | Module exports and command registration. |
| `mcp/src/library.ts`, `mcp/test/library.test.ts` | Renderer reads the map with defaults. |
| `frontend/src/lib/speakers.ts` (new) + `speakers.test.ts` | Tag to label, color class, rename eligibility. |
| `frontend/src/components/VirtualizedTranscriptView.tsx` | Labels from the map, click to rename. |
| `frontend/src/components/MeetingDetails/TranscriptPanel.tsx` | Loads names with the extras, calls `rename_speaker`. |
| `frontend/src/components/ModelsSettings.tsx` | "Speakers" group with the download row. |
| `CLAUDE.md`, `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md` | Pointers to the new contract. |

---

### Task 1: Sidecar fetch script, bundle entry, CI step

**Files:**
- Create: `scripts/fetch-diarizer.sh`
- Modify: `frontend/src-tauri/tauri.conf.json:102-105`
- Modify: `.github/workflows/build-macos.yml:138-148` (add a step after "Build llama-helper sidecar")

**Interfaces:**
- Produces: the file `frontend/src-tauri/binaries/sherpa-onnx-offline-speaker-diarization-aarch64-apple-darwin` on this Mac; Tauri copies it next to the dev binary and into the bundle.

- [ ] **Step 1: Write the script**

```bash
#!/usr/bin/env bash
# Installs sherpa-onnx's speaker diarization tool as the Noesis sidecar
# (docs/superpowers/specs/2026-10-29-speaker-diarization-design.md, section 1).
# Usage: scripts/fetch-diarizer.sh [version] [pre-downloaded archive.tar.bz2]
set -euo pipefail

VERSION="${1:-v2.0.1}"
ARCHIVE="${2:-}"
case "$(uname -m)" in
  arm64)  TRIPLE="aarch64-apple-darwin"; SHERPA_ARCH="arm64" ;;
  x86_64) TRIPLE="x86_64-apple-darwin"; SHERPA_ARCH="x86_64" ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
NAME="sherpa-onnx-${VERSION}-osx-${SHERPA_ARCH}-static"
TOOL="sherpa-onnx-offline-speaker-diarization"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/frontend/src-tauri/binaries/${TOOL}-${TRIPLE}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if [ -z "$ARCHIVE" ]; then
  ARCHIVE="$TMP/$NAME.tar.bz2"
  echo "downloading $NAME.tar.bz2 (about 250 MB)"
  curl -fL --retry 3 -o "$ARCHIVE" "https://github.com/k2-fsa/sherpa-onnx/releases/download/${VERSION}/${NAME}.tar.bz2"
fi

tar -xjf "$ARCHIVE" -C "$TMP" "$NAME/bin/$TOOL"
BIN="$TMP/$NAME/bin/$TOOL"
file "$BIN" | grep -q "Mach-O" || { echo "$BIN is not a Mach-O binary" >&2; exit 1; }
lipo -info "$BIN" | grep -q "$SHERPA_ARCH" || { echo "$BIN is not built for $SHERPA_ARCH" >&2; exit 1; }

mkdir -p "$(dirname "$DEST")"
cp "$BIN" "$DEST"
chmod +x "$DEST"
echo "installed $DEST ($(du -h "$DEST" | cut -f1))"
```

- [ ] **Step 2: Make it executable and run it**

Run: `chmod +x scripts/fetch-diarizer.sh && scripts/fetch-diarizer.sh`
Expected: last line `installed .../binaries/sherpa-onnx-offline-speaker-diarization-aarch64-apple-darwin (NN M)`. Write the size into the Outcome section at the end of this plan.

- [ ] **Step 3: Smoke the binary**

Run: `frontend/src-tauri/binaries/sherpa-onnx-offline-speaker-diarization-aarch64-apple-darwin --help 2>&1 | head -5`
Expected: usage text mentioning `--segmentation.pyannote-model`. If macOS blocks it with "cannot be opened", run `xattr -d com.apple.quarantine <path>` once and retry.

- [ ] **Step 4: Register the sidecar with Tauri**

In `frontend/src-tauri/tauri.conf.json`, change:

```json
        "externalBin": [
            "binaries/llama-helper",
            "binaries/ffmpeg"
        ],
```

to:

```json
        "externalBin": [
            "binaries/llama-helper",
            "binaries/ffmpeg",
            "binaries/sherpa-onnx-offline-speaker-diarization"
        ],
```

- [ ] **Step 5: Add the CI step**

In `.github/workflows/build-macos.yml`, after the "Build llama-helper sidecar" step (ends at `ls -la frontend/src-tauri/binaries/`) and before "Cache FFmpeg binary", insert:

```yaml
      - name: Fetch speaker diarization sidecar
        run: scripts/fetch-diarizer.sh v2.0.1
```

- [ ] **Step 6: Check the dev build still compiles**

Run: `cd frontend/src-tauri && cargo check 2>&1 | tail -2`
Expected: `Finished` with no error. (The running `tauri:dev` will also pick up the new sidecar on its next rebuild.)

- [ ] **Step 7: Commit**

```bash
/usr/bin/git add scripts/fetch-diarizer.sh frontend/src-tauri/tauri.conf.json .github/workflows/build-macos.yml
/usr/bin/git commit -m "build: sherpa-onnx speaker diarization tool as a sidecar"
```

---

### Task 2: Diarization helpers (pure functions)

**Files:**
- Create: `frontend/src-tauri/src/audio/diarization.rs`
- Modify: `frontend/src-tauri/src/audio/mod.rs` (add `pub mod diarization;` after `pub mod claude_mcp;`)

**Interfaces:**
- Consumes: `super::speaker_energy::{EnergyTimeline, Speaker}` (`timeline.window_ms: u32`, `timeline.references() -> (f32, f32)`, `timeline.label_span(start_ms, end_ms, refs) -> Option<Speaker>`).
- Produces (used by Tasks 3 and 7):
  - `pub struct DiarSegment { pub start_ms: f64, pub end_ms: f64, pub speaker: String }`
  - `pub const SAMPLE_RATE: u32 = 16_000; pub const MIN_CLUSTER_SECONDS: f64 = 5.0; pub const MIN_RUN_MS: f64 = 1000.0; pub const CLUSTER_THRESHOLD: f32 = 0.5;`
  - `pub fn samples_per_window(timeline: &EnergyTimeline) -> usize`
  - `pub fn owner_mask(timeline: &EnergyTimeline, total_samples: usize) -> Vec<bool>`
  - `pub fn write_masked_wav(path: &Path, samples: &[f32], per_window: usize, mask: &[bool]) -> anyhow::Result<()>`
  - `pub fn parse_output(stdout: &str) -> Vec<(f64, f64, u32)>` (start ms, end ms, raw cluster id)
  - `pub fn number_clusters(raw: &[(f64, f64, u32)]) -> Vec<DiarSegment>`
  - `pub fn speaker_cuts(start_ms: f64, end_ms: f64, diar: &[DiarSegment]) -> Vec<f64>`
  - `pub fn label_for(start_ms: f64, end_ms: f64, diar: &[DiarSegment]) -> Option<String>`

- [ ] **Step 1: Write the failing tests**

Create `frontend/src-tauri/src/audio/diarization.rs` with the module header, the constants and the struct, then the tests (the functions come in Step 3):

```rust
//! Speaker diarization (docs/superpowers/specs/2026-10-29-speaker-diarization-design.md).
//! The enhance job hands over its 16 kHz samples; the owner's windows are silenced, the
//! sherpa-onnx sidecar clusters the rest, and the clusters become `s1`, `s2`, ... labels
//! on the remote pieces of the transcript.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use super::speaker_energy::{EnergyTimeline, Speaker};

pub const SIDECAR_NAME: &str = "sherpa-onnx-offline-speaker-diarization";
pub const SAMPLE_RATE: u32 = 16_000;
/// Clusters with less speech than this fall back to `them`.
pub const MIN_CLUSTER_SECONDS: f64 = 5.0;
/// A remote speaker run shorter than this is absorbed into its neighbour.
pub const MIN_RUN_MS: f64 = 1000.0;
pub const CLUSTER_THRESHOLD: f32 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub struct DiarSegment {
    pub start_ms: f64,
    pub end_ms: f64,
    /// `s1`, `s2`, ... numbered by first appearance
    pub speaker: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 'm' = owner loud, 't' = remote loud, 'b' = both, '.' = silence; one char per 50 ms window
    fn timeline(pattern: &str) -> EnergyTimeline {
        let mut t = EnergyTimeline::new();
        for c in pattern.chars() {
            match c {
                'm' => t.push(0.5, 0.0),
                't' => t.push(0.0, 0.5),
                'b' => t.push(0.5, 0.5),
                _ => t.push(0.0, 0.0),
            }
        }
        t
    }

    fn seg(start_ms: f64, end_ms: f64, speaker: &str) -> DiarSegment {
        DiarSegment { start_ms, end_ms, speaker: speaker.to_string() }
    }

    #[test]
    fn owner_mask_marks_only_owner_windows() {
        let t = timeline("mtb.");
        assert_eq!(samples_per_window(&t), 800);
        assert_eq!(owner_mask(&t, 4 * 800), vec![true, false, false, false]);
        // a partial trailing window still gets a flag; windows past the timeline are not the owner
        assert_eq!(owner_mask(&t, 5 * 800 + 1), vec![true, false, false, false, false, false]);
    }

    #[test]
    fn masked_wav_has_a_pcm_header_and_silenced_windows() {
        let dir = std::env::temp_dir().join(format!("noesis-diar-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".speakers.wav");
        let samples = vec![0.5f32; 1600];
        write_masked_wav(&path, &samples, 800, &[true, false]).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 3200);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1, "mono");
        assert_eq!(u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]), 16_000);
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16, "bits per sample");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]), 3200);
        assert_eq!(i16::from_le_bytes([bytes[44], bytes[45]]), 0, "first window silenced");
        assert_eq!(i16::from_le_bytes([bytes[44 + 1600], bytes[45 + 1600]]), 16383, "second window kept");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parse_output_reads_segment_lines_and_ignores_the_rest() {
        let out = "Duration : 12.000 s\n0.500 -- 5.125 speaker_00\n5.500 -- 9.000 speaker_01\nbad line\n9.000 -- 8.000 speaker_02\n";
        // 0.5, 5.125, 5.5 and 9.0 are exact in binary, so the millisecond products compare exactly
        assert_eq!(parse_output(out), vec![(500.0, 5125.0, 0), (5500.0, 9000.0, 1)]);
    }

    #[test]
    fn number_clusters_drops_short_ones_and_numbers_by_first_appearance() {
        // cluster 1: 0-3 s and 10-13 s (6 s); cluster 0: 5-6 s (1 s, dropped); cluster 2: 2-8 s (6 s)
        let raw = vec![(5000.0, 6000.0, 0), (0.0, 3000.0, 1), (10_000.0, 13_000.0, 1), (2000.0, 8000.0, 2)];
        let out = number_clusters(&raw);
        assert_eq!(out, vec![seg(0.0, 3000.0, "s1"), seg(2000.0, 8000.0, "s2"), seg(10_000.0, 13_000.0, "s1")]);
    }

    #[test]
    fn speaker_cuts_fall_on_changes_and_absorb_short_runs() {
        let diar = vec![seg(0.0, 5000.0, "s1"), seg(5000.0, 5500.0, "s2"), seg(5500.0, 8000.0, "s1"), seg(8000.0, 12_000.0, "s2")];
        assert_eq!(speaker_cuts(0.0, 12_000.0, &diar), vec![8000.0]);
        // a piece inside one speaker has no cuts; a short leading run joins the next
        assert_eq!(speaker_cuts(500.0, 4000.0, &diar), Vec::<f64>::new());
        let lead = vec![seg(0.0, 400.0, "s2"), seg(400.0, 6000.0, "s1")];
        assert_eq!(speaker_cuts(0.0, 6000.0, &lead), Vec::<f64>::new());
        // segments outside the piece are ignored
        assert_eq!(speaker_cuts(6000.0, 12_000.0, &diar), vec![8000.0]);
    }

    #[test]
    fn label_for_picks_the_largest_overlap_or_nothing() {
        let diar = vec![seg(0.0, 2000.0, "s1"), seg(2000.0, 4000.0, "s2")];
        assert_eq!(label_for(1000.0, 4000.0, &diar).as_deref(), Some("s2"));
        assert_eq!(label_for(0.0, 1500.0, &diar).as_deref(), Some("s1"));
        assert_eq!(label_for(5000.0, 6000.0, &diar), None);
    }
}
```

Add `pub mod diarization;` to `frontend/src-tauri/src/audio/mod.rs` right after `pub mod claude_mcp;`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib diarization 2>&1 | grep -E "^error|cannot find" | head -5`
Expected: compile errors `cannot find function` for `samples_per_window`, `owner_mask`, `write_masked_wav`, `parse_output`, `number_clusters`, `speaker_cuts`, `label_for`.

- [ ] **Step 3: Implement the helpers**

Insert between the struct and `#[cfg(test)]`:

```rust
/// Samples per energy window at the sidecar's rate (800 for the 50 ms default).
pub fn samples_per_window(timeline: &EnergyTimeline) -> usize {
    ((SAMPLE_RATE as f64 * timeline.window_ms as f64 / 1000.0).round() as usize).max(1)
}

/// One flag per window: true where the energy timeline says the owner is speaking.
pub fn owner_mask(timeline: &EnergyTimeline, total_samples: usize) -> Vec<bool> {
    let refs = timeline.references();
    let window_ms = timeline.window_ms as f64;
    let per_window = samples_per_window(timeline);
    let n = total_samples.div_ceil(per_window);
    (0..n)
        .map(|i| {
            let start = i as f64 * window_ms;
            timeline.label_span(start, start + window_ms, refs) == Some(Speaker::Me)
        })
        .collect()
}

/// 16 kHz mono 16-bit PCM WAV; every window flagged in `mask` is written as silence.
pub fn write_masked_wav(path: &Path, samples: &[f32], per_window: usize, mask: &[bool]) -> Result<()> {
    let file = std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut w = std::io::BufWriter::new(file);
    let data_len = (samples.len() * 2) as u32;
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_len).to_le_bytes())?;
    w.write_all(b"WAVEfmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // fmt chunk size
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&1u16.to_le_bytes())?; // mono
    w.write_all(&SAMPLE_RATE.to_le_bytes())?;
    w.write_all(&(SAMPLE_RATE * 2).to_le_bytes())?; // byte rate
    w.write_all(&2u16.to_le_bytes())?; // block align
    w.write_all(&16u16.to_le_bytes())?; // bits per sample
    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())?;
    for (i, chunk) in samples.chunks(per_window.max(1)).enumerate() {
        let silent = mask.get(i).copied().unwrap_or(false);
        for s in chunk {
            let v: i16 = if silent { 0 } else { (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16 };
            w.write_all(&v.to_le_bytes())?;
        }
    }
    w.flush()?;
    Ok(())
}

/// `0.031 -- 5.120 speaker_00` lines from the sidecar's stdout, as (start ms, end ms, cluster).
pub fn parse_output(stdout: &str) -> Vec<(f64, f64, u32)> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let start: f64 = it.next()?.parse().ok()?;
            if it.next()? != "--" {
                return None;
            }
            let end: f64 = it.next()?.parse().ok()?;
            let id: u32 = it.next()?.strip_prefix("speaker_")?.parse().ok()?;
            (end > start).then_some((start * 1000.0, end * 1000.0, id))
        })
        .collect()
}

/// Drops clusters with under `MIN_CLUSTER_SECONDS` of speech and renumbers the rest
/// `s1`, `s2`, ... by the start of their earliest segment. Output is sorted by start.
pub fn number_clusters(raw: &[(f64, f64, u32)]) -> Vec<DiarSegment> {
    let mut total: BTreeMap<u32, f64> = BTreeMap::new();
    let mut first: BTreeMap<u32, f64> = BTreeMap::new();
    for &(s, e, id) in raw {
        *total.entry(id).or_default() += (e - s) / 1000.0;
        first.entry(id).and_modify(|f| *f = f.min(s)).or_insert(s);
    }
    let mut kept: Vec<(f64, u32)> = first
        .into_iter()
        .filter(|(id, _)| total[id] >= MIN_CLUSTER_SECONDS)
        .map(|(id, f)| (f, id))
        .collect();
    kept.sort_by(|a, b| a.0.total_cmp(&b.0));
    let names: BTreeMap<u32, String> = kept.iter().enumerate().map(|(i, (_, id))| (*id, format!("s{}", i + 1))).collect();
    let mut out: Vec<DiarSegment> = raw
        .iter()
        .filter_map(|&(s, e, id)| names.get(&id).map(|n| DiarSegment { start_ms: s, end_ms: e, speaker: n.clone() }))
        .collect();
    out.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));
    out
}

/// Positions inside (start_ms, end_ms) where the remote speaker changes. Runs shorter than
/// `MIN_RUN_MS` are absorbed into the previous run, or into the next one when first.
pub fn speaker_cuts(start_ms: f64, end_ms: f64, diar: &[DiarSegment]) -> Vec<f64> {
    let mut runs: Vec<(f64, f64, &str)> = Vec::new();
    for d in diar {
        let (s, e) = (d.start_ms.max(start_ms), d.end_ms.min(end_ms));
        if e <= s {
            continue;
        }
        match runs.last_mut() {
            Some(r) if r.2 == d.speaker => r.1 = e,
            _ => runs.push((s, e, d.speaker.as_str())),
        }
    }
    let mut merged: Vec<(f64, f64, &str)> = Vec::new();
    for r in runs {
        let short = r.1 - r.0 < MIN_RUN_MS;
        match merged.last_mut() {
            None => merged.push(r),
            Some(prev) if short => prev.1 = r.1,
            Some(prev) if prev.2 == r.2 => prev.1 = r.1,
            Some(prev) if prev.1 - prev.0 < MIN_RUN_MS => {
                // a short first run takes the speaker of the long run that follows it
                prev.1 = r.1;
                prev.2 = r.2;
            }
            Some(_) => merged.push(r),
        }
    }
    merged.iter().skip(1).map(|r| r.0).collect()
}

/// The cluster with the most overlap with [start_ms, end_ms); None when nothing overlaps.
pub fn label_for(start_ms: f64, end_ms: f64, diar: &[DiarSegment]) -> Option<String> {
    let mut by: BTreeMap<&str, f64> = BTreeMap::new();
    for d in diar {
        let overlap = d.end_ms.min(end_ms) - d.start_ms.max(start_ms);
        if overlap > 0.0 {
            *by.entry(d.speaker.as_str()).or_default() += overlap;
        }
    }
    by.into_iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|(s, _)| s.to_string())
}
```

The unused imports (`anyhow`, `bail`, `BufRead`, `BufReader`, `Read`, `PathBuf`, `Command`, `Stdio`, `Mutex`) are consumed by Task 3; leave them in place, the warnings are temporary.

- [ ] **Step 4: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib diarization 2>&1 | grep -E "^test |test result"`
Expected: 6 tests `ok`, `test result: ok. 6 passed`.

- [ ] **Step 5: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/audio/diarization.rs frontend/src-tauri/src/audio/mod.rs
/usr/bin/git commit -m "feat(diarization): owner mask, masked WAV, cluster numbering, speaker cuts and labels"
```

---

### Task 3: Sidecar runner, cancel, and the `diarize` entry point

**Files:**
- Modify: `frontend/src-tauri/src/audio/diarization.rs` (append below the helpers of Task 2)

**Interfaces:**
- Consumes: `crate::jobs::progress(id: &str, pct: u8, detail: Option<String>)`; `super::speaker_models::installed(app) -> Option<Models>` (Task 4; until then the `available` function does not compile, so Task 4 must land before `cargo check` of the whole crate passes. Run this task's tests with the filter; they do not touch `available`).
- Produces (used by Task 7):
  - `pub struct Models { pub segmentation: PathBuf, pub embedding: PathBuf }`
  - `pub fn sidecar_path() -> Option<PathBuf>`
  - `pub fn available<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<(PathBuf, Models)>`
  - `pub fn run_sidecar(sidecar: &Path, models: &Models, wav: &Path, on_progress: &mut dyn FnMut(u8)) -> Result<Vec<DiarSegment>>` (blocking)
  - `pub async fn diarize(job_id: &str, folder: &Path, samples: Vec<f32>, timeline: &EnergyTimeline, sidecar: PathBuf, models: Models) -> Result<Vec<DiarSegment>>`
  - `pub fn cancel()`

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests`:

```rust
    #[test]
    fn sidecar_path_honours_the_env_override_and_requires_a_file() {
        let dir = std::env::temp_dir().join(format!("noesis-diar-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("diarizer");
        std::fs::write(&fake, b"#!/bin/sh\n").unwrap();
        std::env::set_var("NOESIS_DIARIZER", &fake);
        assert_eq!(sidecar_path(), Some(fake.clone()));
        std::env::set_var("NOESIS_DIARIZER", dir.join("missing"));
        // falls through to the executable's directory, which has no sidecar in a test run
        assert_eq!(sidecar_path(), None);
        std::env::remove_var("NOESIS_DIARIZER");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn run_sidecar_reports_progress_and_the_last_error_line() {
        let dir = std::env::temp_dir().join(format!("noesis-diar-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // a stand-in for the real tool: progress on stderr, segments on stdout
        let ok = dir.join("ok.sh");
        std::fs::write(&ok, "#!/bin/sh\necho 'progress 50.00%' >&2\necho '0.000 -- 6.000 speaker_01'\necho '6.000 -- 12.000 speaker_00'\necho 'progress 100.00%' >&2\n").unwrap();
        let failing = dir.join("fail.sh");
        std::fs::write(&failing, "#!/bin/sh\necho 'progress 10.00%' >&2\necho 'model file not found' >&2\nexit 3\n").unwrap();
        for p in [&ok, &failing] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let models = Models { segmentation: dir.join("seg.onnx"), embedding: dir.join("emb.onnx") };
        let wav = dir.join("x.wav");

        let mut seen = Vec::new();
        let out = run_sidecar(&ok, &models, &wav, &mut |p| seen.push(p)).unwrap();
        assert_eq!(seen, vec![50, 100]);
        assert_eq!(out, vec![seg(0.0, 6000.0, "s1"), seg(6000.0, 12_000.0, "s2")]);

        let err = run_sidecar(&failing, &models, &wav, &mut |_| {}).unwrap_err();
        assert_eq!(err.to_string(), "model file not found");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Needs the real sidecar and models. Run with:
    /// NOESIS_DIARIZE_SAMPLE=/path/to/2-two-speakers-en.wav cargo test --lib diarization::tests::sidecar_finds_two_speakers -- --ignored --nocapture
    /// The sample is `2-two-speakers-en.wav` from https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-segmentation-models
    #[test]
    #[ignore]
    fn sidecar_finds_two_speakers() {
        let Ok(sample) = std::env::var("NOESIS_DIARIZE_SAMPLE") else {
            eprintln!("NOESIS_DIARIZE_SAMPLE not set, skipping");
            return;
        };
        let home = std::env::var("HOME").unwrap();
        let models_dir = PathBuf::from(format!("{}/Library/Application Support/com.noesis.app/models/speakers", home));
        let models = Models { segmentation: models_dir.join("segmentation.onnx"), embedding: models_dir.join("embedding.onnx") };
        let sidecar = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries").join(format!("{}-aarch64-apple-darwin", SIDECAR_NAME));
        assert!(sidecar.is_file(), "run scripts/fetch-diarizer.sh first");
        assert!(models.segmentation.is_file() && models.embedding.is_file(), "download the speaker models in Settings, Models first");
        let started = std::time::Instant::now();
        let out = run_sidecar(&sidecar, &models, Path::new(&sample), &mut |p| eprintln!("progress {}%", p)).unwrap();
        let speakers: std::collections::BTreeSet<&str> = out.iter().map(|d| d.speaker.as_str()).collect();
        eprintln!("{} segments, speakers {:?}, {:.1}s", out.len(), speakers, started.elapsed().as_secs_f64());
        assert_eq!(speakers.len(), 2);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib diarization 2>&1 | grep -E "^error|cannot find" | head -5`
Expected: `cannot find` errors for `sidecar_path`, `Models`, `run_sidecar`.

- [ ] **Step 3: Implement the runner**

Insert above `#[cfg(test)]`:

```rust
pub struct Models {
    pub segmentation: PathBuf,
    pub embedding: PathBuf,
}

/// pid of the running sidecar, so a cancel can reach it. One enhance runs at a time.
static CHILD_PID: Mutex<Option<u32>> = Mutex::new(None);

fn target_triple() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "x86_64-apple-darwin"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "unsupported"
    }
}

/// `NOESIS_DIARIZER` overrides (dev and tests); otherwise the sidecar sits next to the
/// executable, where Tauri copies `externalBin` entries in dev and in the bundle.
pub fn sidecar_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NOESIS_DIARIZER") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join(format!("{}-{}", SIDECAR_NAME, target_triple()));
    p.is_file().then_some(p)
}

/// Both the sidecar and the two model files are present.
pub fn available<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<(PathBuf, Models)> {
    Some((sidecar_path()?, super::speaker_models::installed(app)?))
}

/// Runs the sidecar on `wav` and returns the numbered clusters. Blocking: call it from
/// `spawn_blocking`. `on_progress` receives 0..=100 from the tool's stderr.
pub fn run_sidecar(sidecar: &Path, models: &Models, wav: &Path, on_progress: &mut dyn FnMut(u8)) -> Result<Vec<DiarSegment>> {
    let threads = std::thread::available_parallelism().map(|n| n.get().saturating_sub(1)).unwrap_or(1).clamp(1, 8);
    let mut child = Command::new(sidecar)
        .arg(format!("--segmentation.pyannote-model={}", models.segmentation.display()))
        .arg(format!("--embedding.model={}", models.embedding.display()))
        .arg("--clustering.num-clusters=-1")
        .arg(format!("--clustering.cluster-threshold={}", CLUSTER_THRESHOLD))
        .arg(format!("--segmentation.num-threads={}", threads))
        .arg(format!("--embedding.num-threads={}", threads))
        .arg(wav)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting {}", sidecar.display()))?;
    *CHILD_PID.lock().unwrap_or_else(|e| e.into_inner()) = Some(child.id());
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    // stdout is small but must be drained while stderr streams, or a full pipe would stall the tool
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        BufReader::new(stdout).read_to_string(&mut text).map(|_| text)
    });
    let mut last_line = String::new();
    for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
        match line.strip_prefix("progress ").and_then(|p| p.trim().trim_end_matches('%').parse::<f32>().ok()) {
            Some(p) => on_progress(p.clamp(0.0, 100.0) as u8),
            None if !line.trim().is_empty() => last_line = line,
            None => {}
        }
    }
    let status = child.wait()?;
    *CHILD_PID.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let stdout = reader.join().map_err(|_| anyhow!("stdout reader panicked"))??;
    if !status.success() {
        bail!("{}", if last_line.is_empty() { format!("diarizer exited with {}", status) } else { last_line });
    }
    Ok(number_clusters(&parse_output(&stdout)))
}

/// Kills the running sidecar, if any: TERM now, KILL two seconds later. Safe when nothing runs.
pub fn cancel() {
    let pid = CHILD_PID.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(pid) = pid {
        let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
        // ponytail: the KILL can hit a reused pid in a 2 s window; the tool exits on TERM in practice
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
        });
    }
}

/// The Speakers step: silence the owner, write the WAV, run the sidecar, remove the WAV.
/// Takes the samples by value so the buffer is freed as soon as the file is written.
pub async fn diarize(job_id: &str, folder: &Path, samples: Vec<f32>, timeline: &EnergyTimeline, sidecar: PathBuf, models: Models) -> Result<Vec<DiarSegment>> {
    let mask = owner_mask(timeline, samples.len());
    let per_window = samples_per_window(timeline);
    let wav = folder.join(".speakers.wav");
    let (job, wav_for_task) = (job_id.to_string(), wav.clone());
    let result = tokio::task::spawn_blocking(move || {
        write_masked_wav(&wav_for_task, &samples, per_window, &mask)?;
        drop(samples);
        let mut report = |p: u8| crate::jobs::progress(&job, p, Some("Identifying speakers".to_string()));
        run_sidecar(&sidecar, &models, &wav_for_task, &mut report)
    })
    .await;
    let _ = std::fs::remove_file(&wav);
    result.map_err(|e| anyhow!("speaker identification task panicked: {}", e))?
}
```

- [ ] **Step 4: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib diarization 2>&1 | grep -E "^test |test result"`
Expected: 8 `ok`, 1 `ignored`, `test result: ok. 8 passed; 0 failed; 1 ignored`. If the crate does not compile because `speaker_models` does not exist yet, create an empty `frontend/src-tauri/src/audio/speaker_models.rs` containing only `pub fn installed<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Option<super::diarization::Models> { None }` and add `pub mod speaker_models;` to `audio/mod.rs`; Task 4 replaces it.

- [ ] **Step 5: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/audio/diarization.rs frontend/src-tauri/src/audio/mod.rs frontend/src-tauri/src/audio/speaker_models.rs
/usr/bin/git commit -m "feat(diarization): run the sherpa-onnx sidecar with progress and cancel"
```

---

### Task 4: Speaker models (download, status, delete) and the job queue entries

**Files:**
- Create or replace: `frontend/src-tauri/src/audio/speaker_models.rs`
- Modify: `frontend/src-tauri/src/jobs.rs` (`Control` enum near line 54, `stop_work` near line 416, `retry_job` near line 452)
- Modify: `frontend/src-tauri/src/lib.rs:919` (command registration)
- Modify: `frontend/src-tauri/src/audio/mod.rs` (`pub mod speaker_models;` after `pub mod diarization;`)

**Interfaces:**
- Consumes: `crate::jobs::{add, progress, finish, JobKind, Control, Outcome}`; `super::diarization::Models`.
- Produces:
  - `pub fn installed<R: Runtime>(app: &AppHandle<R>) -> Option<Models>` and `pub fn installed_in(dir: &Path) -> Option<Models>`
  - `pub fn resume_offset(part: &Path) -> u64`
  - Tauri commands `speakers_models_status(app) -> Vec<SpeakersModelInfo>`, `speakers_download_models(app, model_name: Option<String>)`, `speakers_cancel_download(model_name: Option<String>)`, `speakers_delete_models(app)`
  - Events `speakers-model-download-progress { modelName, progress }`, `speakers-model-download-complete { modelName }`, `speakers-model-download-error { modelName, error }`
  - `jobs::Control::SpeakersDownload`

- [ ] **Step 1: Write the failing tests**

Create `frontend/src-tauri/src/audio/speaker_models.rs`:

```rust
//! Speaker identification models (spec 2026-10-29, section 2): two ONNX files under
//! `<app data>/models/speakers/`, downloaded together with resume and cancel.

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use log::{info, warn};
use once_cell::sync::Lazy;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use super::diarization::Models;

pub struct FileSpec {
    pub name: &'static str,
    pub url: &'static str,
    pub bytes: u64,
}

pub const FILES: [FileSpec; 2] = [
    FileSpec {
        name: "segmentation.onnx",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx",
        bytes: 5_992_913,
    },
    FileSpec {
        name: "embedding.onnx",
        // the upstream release tag really is spelled "recongition"
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
        bytes: 29_596_978,
    },
];
pub const MODEL_NAME: &str = "speaker-identification";
pub const SIZE_MB: u32 = 36;

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("noesis-spk-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn installed_requires_both_files_at_their_exact_sizes() {
        let d = temp_dir();
        assert!(installed_in(&d).is_none());
        std::fs::write(d.join("segmentation.onnx"), vec![0u8; FILES[0].bytes as usize]).unwrap();
        assert!(installed_in(&d).is_none(), "one file is not enough");
        std::fs::write(d.join("embedding.onnx"), vec![0u8; 10]).unwrap();
        assert!(installed_in(&d).is_none(), "a truncated file does not count");
        std::fs::write(d.join("embedding.onnx"), vec![0u8; FILES[1].bytes as usize]).unwrap();
        let m = installed_in(&d).unwrap();
        assert_eq!(m.segmentation, d.join("segmentation.onnx"));
        assert_eq!(m.embedding, d.join("embedding.onnx"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn resume_offset_is_the_part_file_size_or_zero() {
        let d = temp_dir();
        assert_eq!(resume_offset(&d.join("x.part")), 0);
        std::fs::write(d.join("x.part"), [1u8; 1234]).unwrap();
        assert_eq!(resume_offset(&d.join("x.part")), 1234);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn status_json_matches_the_models_tab_shapes() {
        assert_eq!(status_value(None, true), serde_json::json!("Available"));
        assert_eq!(status_value(None, false), serde_json::json!("Missing"));
        assert_eq!(status_value(Some(40), false), serde_json::json!({"Downloading": {"progress": 40}}));
    }
}
```

Add `pub mod speaker_models;` to `audio/mod.rs` after `pub mod diarization;` (skip if Task 3 already added it).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib speaker_models 2>&1 | grep -E "^error|cannot find" | head -5`
Expected: `cannot find function` for `installed_in`, `resume_offset`, `status_value`.

- [ ] **Step 3: Implement the module**

Insert between the constants and `#[cfg(test)]`:

```rust
pub fn dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    Ok(app.path().app_data_dir().context("app data dir")?.join("models").join("speakers"))
}

/// Both files present with their exact sizes.
pub fn installed_in(dir: &Path) -> Option<Models> {
    let exact = |f: &FileSpec| std::fs::metadata(dir.join(f.name)).map(|m| m.len() == f.bytes).unwrap_or(false);
    FILES.iter().all(exact).then(|| Models { segmentation: dir.join(FILES[0].name), embedding: dir.join(FILES[1].name) })
}

pub fn installed<R: Runtime>(app: &AppHandle<R>) -> Option<Models> {
    installed_in(&dir(app).ok()?)
}

/// Bytes already in a `.part` file, or 0.
pub fn resume_offset(part: &Path) -> u64 {
    std::fs::metadata(part).map(|m| m.len()).unwrap_or(0)
}

static ACTIVE: Lazy<Mutex<Option<CancellationToken>>> = Lazy::new(|| Mutex::new(None));
static PROGRESS: Mutex<Option<u8>> = Mutex::new(None);

/// The Models tab understands `"Available"`, `"Missing"` and `{"Downloading": {"progress": n}}`.
pub fn status_value(progress: Option<u8>, installed: bool) -> serde_json::Value {
    match (progress, installed) {
        (Some(p), _) => serde_json::json!({"Downloading": {"progress": p}}),
        (None, true) => serde_json::json!("Available"),
        (None, false) => serde_json::json!("Missing"),
    }
}

#[derive(Serialize)]
pub struct SpeakersModelInfo {
    pub name: &'static str,
    pub size_mb: u32,
    pub status: serde_json::Value,
}

#[tauri::command]
pub fn speakers_models_status<R: Runtime>(app: AppHandle<R>) -> Vec<SpeakersModelInfo> {
    let progress = *PROGRESS.lock().unwrap_or_else(|e| e.into_inner());
    vec![SpeakersModelInfo { name: MODEL_NAME, size_mb: SIZE_MB, status: status_value(progress, installed(&app).is_some()) }]
}

/// One file: resume from `.part` when the server honours Range, verify the exact size, rename into place.
/// `on_progress` gets whole-download percentages and is called only when the integer changes.
async fn download_file(client: &reqwest::Client, dir: &Path, spec: &FileSpec, token: &CancellationToken, done_before: u64, total: u64, on_progress: &dyn Fn(u8)) -> Result<()> {
    let final_path = dir.join(spec.name);
    if std::fs::metadata(&final_path).map(|m| m.len() == spec.bytes).unwrap_or(false) {
        return Ok(());
    }
    let part = dir.join(format!("{}.part", spec.name));
    let mut have = resume_offset(&part).min(spec.bytes);
    let mut request = client.get(spec.url);
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={}-", have));
    }
    let response = tokio::select! {
        biased;
        _ = token.cancelled() => bail!("cancelled"),
        r = request.send() => r.with_context(|| format!("starting {}", spec.name))?,
    };
    match response.status() {
        reqwest::StatusCode::OK => have = 0, // full body: the server ignored the range, or there was none
        reqwest::StatusCode::PARTIAL_CONTENT => {}
        other => bail!("{} returned HTTP {}", spec.name, other),
    }
    let mut file = tokio::fs::OpenOptions::new().create(true).write(true).truncate(have == 0).append(have > 0).open(&part).await?;
    let mut stream = response.bytes_stream();
    let mut last_pct: Option<u8> = None;
    loop {
        let chunk = tokio::select! {
            biased;
            _ = token.cancelled() => bail!("cancelled"),
            c = stream.next() => match c { Some(c) => c?, None => break },
        };
        file.write_all(&chunk).await?;
        have += chunk.len() as u64;
        let pct = (((done_before + have) as f64 / total as f64) * 100.0).min(100.0) as u8;
        if last_pct != Some(pct) {
            last_pct = Some(pct);
            on_progress(pct);
        }
    }
    file.flush().await?;
    drop(file);
    let got = std::fs::metadata(&part)?.len();
    if got != spec.bytes {
        let _ = std::fs::remove_file(&part);
        bail!("{} is {} bytes, expected {}", spec.name, got, spec.bytes);
    }
    std::fs::rename(&part, &final_path)?;
    info!("Speaker model {} downloaded", spec.name);
    Ok(())
}

async fn download_all<R: Runtime>(app: &AppHandle<R>, job_id: &str, token: &CancellationToken) -> Result<()> {
    let dir = dir(app)?;
    std::fs::create_dir_all(&dir)?;
    let client = reqwest::Client::builder().user_agent("noesis").build()?;
    let total: u64 = FILES.iter().map(|f| f.bytes).sum();
    let mut done = 0u64;
    for spec in FILES.iter() {
        let (app, job) = (app.clone(), job_id.to_string());
        let on_progress = move |p: u8| {
            *PROGRESS.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
            crate::jobs::progress(&job, p, Some(format!("{} MB", SIZE_MB)));
            let _ = app.emit("speakers-model-download-progress", serde_json::json!({"modelName": MODEL_NAME, "progress": p}));
        };
        download_file(&client, &dir, spec, token, done, total, &on_progress).await?;
        done += spec.bytes;
    }
    Ok(())
}

/// Downloads both files as one job. `model_name` is accepted so the Models tab can call every
/// engine the same way; its value is ignored.
#[tauri::command]
pub async fn speakers_download_models<R: Runtime>(app: AppHandle<R>, model_name: Option<String>) -> Result<(), String> {
    let _ = model_name;
    let token = CancellationToken::new();
    {
        let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        if active.is_some() {
            return Err("Speaker models are already downloading".to_string());
        }
        *active = Some(token.clone());
    }
    *PROGRESS.lock().unwrap_or_else(|e| e.into_inner()) = Some(0);
    let job_id = uuid::Uuid::new_v4().to_string();
    crate::jobs::add(&job_id, crate::jobs::JobKind::Download, None, "Speaker identification", crate::jobs::Control::SpeakersDownload, true);
    let result = download_all(&app, &job_id, &token).await;
    *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *PROGRESS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    match result {
        Ok(()) => {
            crate::jobs::finish(&job_id, crate::jobs::Outcome::Done);
            let _ = app.emit("speakers-model-download-complete", serde_json::json!({"modelName": MODEL_NAME}));
            Ok(())
        }
        Err(_) if token.is_cancelled() => {
            // the Models tab refreshes itself after a cancel click; no error toast for a cancel
            crate::jobs::finish(&job_id, crate::jobs::Outcome::Cancelled);
            Err("Cancelled".to_string())
        }
        Err(e) => {
            warn!("Speaker model download failed: {}", e);
            crate::jobs::finish(&job_id, crate::jobs::Outcome::Failed(e.to_string()));
            let _ = app.emit("speakers-model-download-error", serde_json::json!({"modelName": MODEL_NAME, "error": e.to_string()}));
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn speakers_cancel_download(model_name: Option<String>) -> Result<(), String> {
    let _ = model_name;
    match ACTIVE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(token) => {
            token.cancel();
            Ok(())
        }
        None => Err("No speaker model download in progress".to_string()),
    }
}

/// Removes both files and any partial download; the Speakers step then no longer runs.
#[tauri::command]
pub async fn speakers_delete_models<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let dir = dir(&app).map_err(|e| e.to_string())?;
    for f in FILES.iter() {
        let _ = std::fs::remove_file(dir.join(f.name));
        let _ = std::fs::remove_file(dir.join(format!("{}.part", f.name)));
    }
    Ok(())
}
```

- [ ] **Step 4: Wire the job queue**

In `frontend/src-tauri/src/jobs.rs`:

1. In `pub enum Control`, after `ParakeetDownload,` add `SpeakersDownload,`.
2. In `stop_work`, after the `Control::ParakeetDownload => { ... }` arm add:

```rust
        Control::SpeakersDownload => {
            crate::audio::speaker_models::speakers_cancel_download(None).await?;
        }
```

3. In `retry_job`, after the `Control::ParakeetDownload => ...` line add:

```rust
            Control::SpeakersDownload => crate::audio::speaker_models::speakers_download_models(app, None).await,
```

- [ ] **Step 5: Register the commands**

In `frontend/src-tauri/src/lib.rs`, after the line `audio::claude_mcp::register_mcp_with_claude,` add:

```rust
            audio::speaker_models::speakers_models_status,
            audio::speaker_models::speakers_download_models,
            audio::speaker_models::speakers_cancel_download,
            audio::speaker_models::speakers_delete_models,
```

- [ ] **Step 6: Run the tests and the jobs tests**

Run: `cd frontend/src-tauri && cargo test --lib speaker_models 2>&1 | grep -E "^test |test result"`
Expected: 3 `ok`.
Run: `cd frontend/src-tauri && cargo test --lib jobs 2>&1 | grep -E "test result"`
Expected: all `ok` (the existing jobs tests still pass with the new variant).

- [ ] **Step 7: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/audio/speaker_models.rs frontend/src-tauri/src/audio/mod.rs frontend/src-tauri/src/jobs.rs frontend/src-tauri/src/lib.rs
/usr/bin/git commit -m "feat(speakers): download, status and delete for the speaker identification models"
```

---

### Task 5: transcript.md renders names from an ordered list

**Files:**
- Modify: `frontend/src-tauri/src/audio/transcript_md.rs:19-20, 62-65, 75-79` and its tests at lines 118, 145, 159, 177-213
- Modify: `frontend/src-tauri/src/audio/recording_saver.rs:596` (live writer)
- Modify: `frontend/src-tauri/src/audio/retranscription.rs:735-756` (enhance writer; a temporary adaptation, Task 7 rewrites this block)

**Interfaces:**
- Produces: `TranscriptMdMeta.speaker_names: &'a [(String, String)]`, ordered (tag, name) pairs; empty means no `speakers:` line and bare lines.

- [ ] **Step 1: Replace the two speaker tests and adapt the others**

In `transcript_md.rs` tests, change every `speaker_names: None,` (three places) to `speaker_names: &[],`. Replace the test `tags_render_after_speakers_before_the_closing_rule` and the test `labelled_lines_carry_names_and_frontmatter_lists_speakers` with:

```rust
    #[test]
    fn tags_render_after_speakers_before_the_closing_rule() {
        let tags = vec!["a".to_string(), "b".to_string()];
        let names = vec![("me".to_string(), "Me".to_string()), ("them".to_string(), "Others".to_string())];
        let meta = TranscriptMdMeta {
            title: "t", project: "dex", meeting_id: "m", started_at: "2026-10-27T12:03:09-04:00",
            duration_seconds: None, quality: "enhanced", model: "whisper large-v3",
            speaker_names: &names,
            tags: &tags,
        };
        let md = render_transcript_md(&meta, &[]);
        assert!(md.contains("speakers: me=Me, them=Others\ntags: [a, b]\n---\n"), "{}", md);
    }

    // spec 4.4 and spec 2026-10-29: every tag in the list gets its name; both and unknown stay bare
    #[test]
    fn labelled_lines_carry_names_and_frontmatter_lists_speakers() {
        let names = vec![
            ("me".to_string(), "Alexandre".to_string()),
            ("them".to_string(), "Others".to_string()),
            ("s1".to_string(), "Speaker 1".to_string()),
            ("s2".to_string(), "Marie".to_string()),
        ];
        let meta = TranscriptMdMeta {
            title: "t", project: "dex", meeting_id: "m", started_at: "2026-10-27T12:03:09-04:00",
            duration_seconds: None, quality: "enhanced", model: "whisper large-v3",
            speaker_names: &names,
            tags: &[],
        };
        let segments = vec![
            MdSegment { start: 0.0, speaker: Some("me".into()), text: "Hello.".into() },
            MdSegment { start: 4.0, speaker: Some("s1".into()), text: "Hi.".into() },
            MdSegment { start: 9.0, speaker: Some("s2".into()), text: "Bonjour.".into() },
            MdSegment { start: 12.0, speaker: Some("both".into()), text: "Yes yes.".into() },
            MdSegment { start: 15.0, speaker: Some("them".into()), text: "Who.".into() },
            MdSegment { start: 18.0, speaker: None, text: "Unknown.".into() },
        ];
        let md = render_transcript_md(&meta, &segments);
        assert!(md.contains("model: whisper large-v3\nspeakers: me=Alexandre, them=Others, s1=Speaker 1, s2=Marie\n---\n"), "{}", md);
        assert!(md.ends_with("[00:00] Alexandre: Hello.\n[00:04] Speaker 1: Hi.\n[00:09] Marie: Bonjour.\n[00:12] Yes yes.\n[00:15] Others: Who.\n[00:18] Unknown.\n"), "{}", md);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib transcript_md 2>&1 | grep -E "^error" -A 3 | head -8`
Expected: type errors on `speaker_names` (expected `Option<(&str, &str)>`, found `&[]` / `&Vec`).

- [ ] **Step 3: Change the struct and the renderer**

In `transcript_md.rs` replace:

```rust
    /// spec 4.4: (name for "me", name for "them"); None when the meeting has no labels
    pub speaker_names: Option<(&'a str, &'a str)>,
```

with:

```rust
    /// spec 4.4 and spec 2026-10-29: (tag, name) in front-matter order: me, them, s1, s2, ...
    /// Empty when the meeting has no labels.
    pub speaker_names: &'a [(String, String)],
```

Replace the `MdSegment.speaker` doc comment `/// "me", "them", "both" or None` with `/// "me", "them", "both", "s1", "s2", ... or None`.

Replace:

```rust
    // spec 4.4
    if let Some((me, them)) = meta.speaker_names {
        out.push_str(&format!("speakers: me={}, them={}\n", me, them));
    }
```

with:

```rust
    // spec 4.4 and spec 2026-10-29
    if !meta.speaker_names.is_empty() {
        let list = meta.speaker_names.iter().map(|(tag, name)| format!("{}={}", tag, name)).collect::<Vec<_>>().join(", ");
        out.push_str(&format!("speakers: {}\n", list));
    }
```

Replace:

```rust
        let name = match (seg.speaker.as_deref(), meta.speaker_names) {
            (Some("me"), Some((me, _))) => Some(me),
            (Some("them"), Some((_, them))) => Some(them),
            _ => None,
        };
```

with:

```rust
        let name = seg
            .speaker
            .as_deref()
            .and_then(|tag| meta.speaker_names.iter().find(|(t, _)| t == tag))
            .map(|(_, n)| n.as_str());
```

- [ ] **Step 4: Adapt the two writers**

In `recording_saver.rs`, just above the `let md_meta = super::transcript_md::TranscriptMdMeta {` near line 588, add:

```rust
            let live_names: Vec<(String, String)> = if any_labelled {
                vec![("me".to_string(), owner_name.to_string()), ("them".to_string(), "Others".to_string())]
            } else {
                Vec::new()
            };
```

and change `speaker_names: if any_labelled { Some((owner_name.as_str(), "Others")) } else { None }, // spec 4.4` to `speaker_names: &live_names, // spec 4.4`.

In `retranscription.rs`, replace the block:

```rust
        let speaker_names = if segments.iter().any(|s| s.speaker.is_some()) {
            Some((
                meta_json["speakers"]["me"].as_str().unwrap_or("Me"),
                meta_json["speakers"]["them"].as_str().unwrap_or("Others"),
            ))
        } else {
            None
        };
```

with:

```rust
        let speaker_names: Vec<(String, String)> = if segments.iter().any(|s| s.speaker.is_some()) {
            vec![
                ("me".to_string(), meta_json["speakers"]["me"].as_str().unwrap_or("Me").to_string()),
                ("them".to_string(), meta_json["speakers"]["them"].as_str().unwrap_or("Others").to_string()),
            ]
        } else {
            Vec::new()
        };
```

and change `speaker_names, // spec 4.4` in the `TranscriptMdMeta { ... }` literal to `speaker_names: &speaker_names, // spec 4.4`.

- [ ] **Step 5: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib transcript_md 2>&1 | grep -E "^test |test result"`
Expected: all `ok`.
Run: `cd frontend/src-tauri && cargo test --lib retranscription 2>&1 | grep -E "test result"`
Expected: `ok`.

- [ ] **Step 6: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/audio/transcript_md.rs frontend/src-tauri/src/audio/recording_saver.rs frontend/src-tauri/src/audio/retranscription.rs
/usr/bin/git commit -m "feat(transcript-md): speaker names as an ordered tag list"
```

---

### Task 6: Speaker names module, `rename_speaker`, extras

**Files:**
- Create: `frontend/src-tauri/src/audio/speaker_names.rs`
- Modify: `frontend/src-tauri/src/audio/audio_cleanup.rs:71, 140, 168-185` (`read_meta` and `folder_of` become `pub(crate)`; `MeetingExtras.speakers`)
- Modify: `frontend/src-tauri/src/audio/mod.rs` (`pub mod speaker_names;`), `frontend/src-tauri/src/lib.rs` (register `rename_speaker`)

**Interfaces:**
- Consumes: `super::metadata_file::update_metadata(folder, |obj: &mut Map<String, Value>| -> std::io::Result<T>) -> std::io::Result<T>`; `super::transcript_md::{TranscriptMdMeta, MdSegment, write_transcript_md, local_iso}`; `super::search_index::metadata_tags(folder) -> Vec<String>`; `super::library::INBOX`; `crate::state::AppState` with `db_manager.pool()`.
- Produces (used by Task 7 and the UI):
  - `pub fn is_cluster_tag(tag: &str) -> bool`
  - `pub fn default_name(tag: &str) -> Option<String>`
  - `pub fn display_name(tag: &str, names: &Map<String, Value>) -> Option<String>`
  - `pub fn ordered_names(names: &Map<String, Value>, tags_present: &[String]) -> Vec<(String, String)>`
  - `pub fn reset_cluster_names(names: &mut Map<String, Value>, owner_name: &str, cluster_tags: &[String])`
  - `pub fn validate(tag: &str, name: &str) -> Result<String, String>`
  - `pub fn model_label(meta: &Value) -> (String, String)` (quality, model)
  - `pub async fn rewrite_transcript_md<R: Runtime>(app: &AppHandle<R>, meeting_id: &str, folder: &Path) -> anyhow::Result<()>`
  - Tauri command `rename_speaker(app, meeting_id: String, tag: String, name: String) -> Result<(), String>` emitting `meeting-speakers-changed { meeting_id }`
  - `MeetingExtras.speakers: serde_json::Map<String, serde_json::Value>`

- [ ] **Step 1: Write the failing tests**

Create `frontend/src-tauri/src/audio/speaker_names.rs`:

```rust
//! Speaker display names (spec 2026-10-29, Contracts): `metadata.json` `speakers` maps a
//! segment tag (`me`, `them`, `s1`, ...) to a name. Missing keys fall back to Me, Others and
//! Speaker N. `both` never has a name.

use anyhow::Result;
use serde_json::{Map, Value};
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager, Runtime};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn cluster_tags_are_s_followed_by_a_positive_number() {
        assert!(is_cluster_tag("s1") && is_cluster_tag("s12"));
        assert!(!is_cluster_tag("s0") && !is_cluster_tag("s01") && !is_cluster_tag("sx") && !is_cluster_tag("me") && !is_cluster_tag("system"));
    }

    #[test]
    fn names_come_from_the_map_then_defaults_and_never_for_both() {
        let names = map(json!({"me": "Alexandre", "s2": "Marie", "both": "Nope"}));
        assert_eq!(display_name("me", &names).as_deref(), Some("Alexandre"));
        assert_eq!(display_name("them", &names).as_deref(), Some("Others"));
        assert_eq!(display_name("s1", &names).as_deref(), Some("Speaker 1"));
        assert_eq!(display_name("s2", &names).as_deref(), Some("Marie"));
        assert_eq!(display_name("both", &names), None);
        assert_eq!(display_name("system", &names), None);
    }

    #[test]
    fn ordered_names_list_me_them_then_clusters_present() {
        let names = map(json!({"s2": "Marie"}));
        let tags = vec!["s2".to_string(), "both".to_string(), "s1".to_string(), "s2".to_string(), "me".to_string()];
        assert_eq!(
            ordered_names(&names, &tags),
            vec![
                ("me".to_string(), "Me".to_string()),
                ("them".to_string(), "Others".to_string()),
                ("s1".to_string(), "Speaker 1".to_string()),
                ("s2".to_string(), "Marie".to_string()),
            ]
        );
    }

    #[test]
    fn reset_keeps_me_and_them_and_replaces_clusters() {
        let mut names = map(json!({"me": "Alexandre", "s1": "Old", "s3": "Gone"}));
        reset_cluster_names(&mut names, "Owner", &["s1".to_string(), "s2".to_string()]);
        assert_eq!(names, map(json!({"me": "Alexandre", "them": "Others", "s1": "Speaker 1", "s2": "Speaker 2"})));
        let mut empty = Map::new();
        reset_cluster_names(&mut empty, "Owner", &[]);
        assert_eq!(empty, map(json!({"me": "Owner", "them": "Others"})));
    }

    #[test]
    fn validate_rejects_bad_tags_and_names() {
        assert_eq!(validate("s2", "  Marie "), Ok("Marie".to_string()));
        assert!(validate("both", "x").is_err());
        assert!(validate("s0", "x").is_err());
        assert!(validate("me", "   ").is_err());
        assert!(validate("me", "a\nb").is_err());
        assert!(validate("me", &"x".repeat(61)).is_err());
        assert!(validate("them", &"é".repeat(60)).is_ok());
    }

    #[test]
    fn model_label_prefers_a_finished_enhance() {
        let done = json!({"transcription": {"live": {"provider": "parakeet", "model": "p"}, "enhanced": {"provider": "whisper", "model": "large-v3", "status": "done"}}});
        assert_eq!(model_label(&done), ("enhanced".to_string(), "whisper large-v3".to_string()));
        let running = json!({"transcription": {"live": {"provider": "parakeet", "model": "p"}, "enhanced": {"status": "running"}}});
        assert_eq!(model_label(&running), ("live".to_string(), "parakeet p".to_string()));
        assert_eq!(model_label(&json!({})), ("live".to_string(), String::new()));
    }
}
```

Add `pub mod speaker_names;` to `audio/mod.rs` after `pub mod speaker_models;`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend/src-tauri && cargo test --lib speaker_names 2>&1 | grep -E "cannot find" | head -5`
Expected: `cannot find function` for `is_cluster_tag`, `display_name`, `ordered_names`, `reset_cluster_names`, `validate`, `model_label`.

- [ ] **Step 3: Implement**

Insert between the imports and `#[cfg(test)]`:

```rust
/// `s1`, `s2`, ... (no leading zero).
pub fn is_cluster_tag(tag: &str) -> bool {
    match tag.strip_prefix('s') {
        Some(n) => !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

pub fn default_name(tag: &str) -> Option<String> {
    match tag {
        "me" => Some("Me".to_string()),
        "them" => Some("Others".to_string()),
        t if is_cluster_tag(t) => Some(format!("Speaker {}", &t[1..])),
        _ => None,
    }
}

/// The map's entry, else the default. None for `both` and unknown tags.
pub fn display_name(tag: &str, names: &Map<String, Value>) -> Option<String> {
    if tag == "both" {
        return None;
    }
    match names.get(tag).and_then(|v| v.as_str()) {
        Some(n) if default_name(tag).is_some() => Some(n.to_string()),
        _ => default_name(tag),
    }
}

/// Front-matter order: me, them, then the cluster tags present, by number.
pub fn ordered_names(names: &Map<String, Value>, tags_present: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for fixed in ["me", "them"] {
        if let Some(n) = display_name(fixed, names) {
            out.push((fixed.to_string(), n));
        }
    }
    let mut numbers: Vec<u32> = tags_present.iter().filter(|t| is_cluster_tag(t)).filter_map(|t| t[1..].parse().ok()).collect();
    numbers.sort_unstable();
    numbers.dedup();
    for n in numbers {
        let tag = format!("s{}", n);
        if let Some(name) = display_name(&tag, names) {
            out.push((tag, name));
        }
    }
    out
}

/// After an enhance: `me` and `them` are kept (seeded when missing), every `s*` entry is
/// replaced by the default for the clusters found this time.
pub fn reset_cluster_names(names: &mut Map<String, Value>, owner_name: &str, cluster_tags: &[String]) {
    names.retain(|k, _| !is_cluster_tag(k));
    names.entry("me").or_insert_with(|| Value::String(owner_name.to_string()));
    names.entry("them").or_insert_with(|| Value::String("Others".to_string()));
    for tag in cluster_tags {
        if let Some(d) = default_name(tag) {
            names.insert(tag.clone(), Value::String(d));
        }
    }
}

/// `tag` must be me, them or a cluster tag; the name is trimmed, 1 to 60 characters, one line.
pub fn validate(tag: &str, name: &str) -> Result<String, String> {
    if tag != "me" && tag != "them" && !is_cluster_tag(tag) {
        return Err(format!("Unknown speaker tag: {}", tag));
    }
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 || name.contains(['\n', '\r']) {
        return Err("A speaker name is 1 to 60 characters on one line".to_string());
    }
    Ok(name.to_string())
}

/// ("enhanced", "whisper large-v3") once the enhanced pass is done, else ("live", "<provider> <model>").
/// Mirrors `modelLabel` in the MCP server.
pub fn model_label(meta: &Value) -> (String, String) {
    let label = |v: &Value| {
        format!("{} {}", v["provider"].as_str().unwrap_or(""), v["model"].as_str().unwrap_or("")).trim().to_string()
    };
    let t = &meta["transcription"];
    if t["enhanced"]["status"].as_str() == Some("done") {
        ("enhanced".to_string(), label(&t["enhanced"]))
    } else {
        ("live".to_string(), label(&t["live"]))
    }
}

/// Rebuilds transcript.md from the stored segments and the current metadata (names, title, tags).
pub async fn rewrite_transcript_md<R: Runtime>(app: &AppHandle<R>, meeting_id: &str, folder: &Path) -> Result<()> {
    let state = app.state::<crate::state::AppState>();
    let rows: Vec<(String, Option<f64>, Option<String>)> =
        sqlx::query_as("SELECT transcript, audio_start_time, speaker FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time, id")
            .bind(meeting_id)
            .fetch_all(state.db_manager.pool())
            .await?;
    let meta = super::audio_cleanup::read_meta(folder);
    let names = meta["speakers"].as_object().cloned().unwrap_or_default();
    let tags: Vec<String> = rows.iter().filter_map(|r| r.2.clone()).collect();
    let speaker_names = if tags.is_empty() { Vec::new() } else { ordered_names(&names, &tags) };
    let title = meta["title"].as_str().or_else(|| meta["meeting_name"].as_str()).unwrap_or("");
    let project = meta["project"].as_str().unwrap_or(super::library::INBOX);
    let started_at = super::transcript_md::local_iso(meta["created_at"].as_str().unwrap_or(""));
    let (quality, model) = model_label(&meta);
    let topic_tags = super::search_index::metadata_tags(folder);
    let md_meta = super::transcript_md::TranscriptMdMeta {
        title,
        project,
        meeting_id,
        started_at: &started_at,
        duration_seconds: meta["duration_seconds"].as_f64(),
        quality: &quality,
        model: &model,
        speaker_names: &speaker_names,
        tags: &topic_tags,
    };
    let segments: Vec<super::transcript_md::MdSegment> = rows
        .into_iter()
        .map(|(text, start, speaker)| super::transcript_md::MdSegment { start: start.unwrap_or(0.0), speaker, text })
        .collect();
    super::transcript_md::write_transcript_md(folder, &md_meta, &segments)
}

/// Renames one speaker of one meeting: metadata map, transcript.md, then an event for the UI.
#[tauri::command]
pub async fn rename_speaker<R: Runtime>(app: AppHandle<R>, meeting_id: String, tag: String, name: String) -> Result<(), String> {
    let name = validate(&tag, &name)?;
    let folder = super::audio_cleanup::folder_of(&app, &meeting_id)
        .await?
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
        .ok_or_else(|| "Meeting folder not found".to_string())?;
    let tag_for_meta = tag.clone();
    super::metadata_file::update_metadata(&folder, move |obj| {
        let entry = obj.entry("speakers").or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        entry.as_object_mut().expect("object").insert(tag_for_meta, Value::String(name));
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    rewrite_transcript_md(&app, &meeting_id, &folder).await.map_err(|e| e.to_string())?;
    let _ = app.emit("meeting-speakers-changed", serde_json::json!({ "meeting_id": meeting_id }));
    Ok(())
}
```

- [ ] **Step 4: Open the helpers it needs and extend the extras**

In `audio_cleanup.rs`:

1. Change `fn read_meta(folder: &Path) -> serde_json::Value {` to `pub(crate) fn read_meta(folder: &Path) -> serde_json::Value {`.
2. Change `async fn folder_of<R: Runtime>(` to `pub(crate) async fn folder_of<R: Runtime>(`.
3. Replace the struct and command:

```rust
pub struct MeetingExtras {
    pub tags: Vec<String>,
    pub audio_deleted_at: Option<String>,
    pub has_audio: bool,
    /// spec 2026-10-29: tag -> display name from metadata.json; empty for legacy meetings
    pub speakers: serde_json::Map<String, serde_json::Value>,
}

#[tauri::command]
pub async fn get_meeting_extras<R: Runtime>(app: AppHandle<R>, meeting_id: String) -> Result<MeetingExtras, String> {
    let folder = folder_of(&app, &meeting_id).await?.map(PathBuf::from).filter(|p| p.is_dir());
    let Some(folder) = folder else {
        return Ok(MeetingExtras { tags: vec![], audio_deleted_at: None, has_audio: false, speakers: Default::default() });
    };
    let meta = read_meta(&folder);
    Ok(MeetingExtras {
        tags: super::search_index::metadata_tags(&folder),
        audio_deleted_at: meta["audio_deleted_at"].as_str().map(str::to_string),
        has_audio: super::retranscription::find_audio_file(&folder).is_ok(),
        speakers: meta["speakers"].as_object().cloned().unwrap_or_default(),
    })
}
```

In `lib.rs`, after `audio::speaker_models::speakers_delete_models,` add `audio::speaker_names::rename_speaker,`.

- [ ] **Step 5: Run the tests**

Run: `cd frontend/src-tauri && cargo test --lib speaker_names 2>&1 | grep -E "^test |test result"`
Expected: 6 `ok`.
Run: `cd frontend/src-tauri && cargo test --lib audio_cleanup 2>&1 | grep -E "test result"`
Expected: `ok`.

- [ ] **Step 6: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/audio/speaker_names.rs frontend/src-tauri/src/audio/audio_cleanup.rs frontend/src-tauri/src/audio/mod.rs frontend/src-tauri/src/lib.rs
/usr/bin/git commit -m "feat(speakers): names map, rename_speaker command, speakers in meeting extras"
```

---

### Task 7: The Speakers step inside the enhance job

**Files:**
- Modify: `frontend/src-tauri/src/jobs.rs:17-18` (constant), `:405-410` (`stop_work` Enhance arm), tests
- Modify: `frontend/src-tauri/src/audio/retranscription.rs:8` (imports), `:166-167` (job start), `:204` (call), `:389-396` (signature), `:464-480` (VAD block), `:566-581` (labelling loop), the transcript.md names block from Task 5

**Interfaces:**
- Consumes: `diarization::{available, diarize, speaker_cuts, label_for, DiarSegment}` (Tasks 2, 3), `speaker_names::{is_cluster_tag, reset_cluster_names, ordered_names}` (Task 6), `metadata_file::update_metadata`, `recording_preferences::load_recording_preferences(&app).await -> Result<RecordingPreferences>` with `owner_name: String`.
- Produces: `jobs::STEP_SPEAKERS = "Speakers"`; a `run_retranscription` that takes `job_id: String` as its second argument.

- [ ] **Step 1: Write the failing jobs test**

In `jobs.rs` tests, add:

```rust
    #[test]
    fn steps_keep_insertion_order_and_update_in_place() {
        let mut r = Registry::default();
        r.add("a", JobKind::Enhance, Some("m"), "", enhance(), true, Instant::now());
        r.step("a", STEP_SPEAKERS, JobState::Queued, None);
        r.step("a", STEP_TRANSCRIBE, JobState::Running, None);
        r.step("a", STEP_TRANSCRIBE, JobState::Queued, None);
        r.step("a", STEP_SPEAKERS, JobState::Running, None);
        let steps = &r.snapshot()[0].steps;
        assert_eq!(steps.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec![STEP_SPEAKERS, STEP_TRANSCRIBE]);
        assert_eq!(steps[0].state, JobState::Running);
        assert_eq!(steps[1].state, JobState::Queued);
    }
```

(`enhance()` is the existing test helper that builds a `Control::Enhance`; it is used by the neighbouring tests.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cd frontend/src-tauri && cargo test --lib steps_keep_insertion_order 2>&1 | grep -E "cannot find|^error" | head -3`
Expected: `cannot find value STEP_SPEAKERS`.

- [ ] **Step 3: Add the constant and the cancel branch**

In `jobs.rs`, after `pub const STEP_TITLE: &str = "Title/tags";` add:

```rust
/// spec 2026-10-29: remote speaker identification, before Transcribe
pub const STEP_SPEAKERS: &str = "Speakers";
```

Replace the `Control::Enhance { .. } => { ... }` arm of `stop_work`:

```rust
        Control::Enhance { .. } => {
            let running = |name: &str| job.steps.iter().any(|s| s.name == name && s.state == JobState::Running);
            if running(STEP_SPEAKERS) || running(STEP_TRANSCRIBE) {
                crate::audio::retranscription::cancel_retranscription(job.meeting_id.as_deref())?;
                if running(STEP_SPEAKERS) {
                    crate::audio::diarization::cancel();
                }
            }
        }
```

Run: `cd frontend/src-tauri && cargo test --lib jobs 2>&1 | grep -E "test result"`
Expected: `ok`.

- [ ] **Step 4: Plumb `job_id` and the step into `start_retranscription`**

In `retranscription.rs`:

1. Change the import to `use crate::jobs::{self, Control, JobKind, JobState, Outcome, STEP_SPEAKERS, STEP_TITLE, STEP_TRANSCRIBE};`.
2. Replace:

```rust
    jobs::start(&job_id);
    jobs::step(&job_id, STEP_TRANSCRIBE, JobState::Running, None);
```

with:

```rust
    jobs::start(&job_id);
    // spec 2026-10-29: the Speakers step is listed first when it will run; Transcribe shows
    // progress during decode and VAD, then yields to Speakers, then runs for real
    if super::diarization::available(&app).is_some() {
        jobs::step(&job_id, STEP_SPEAKERS, JobState::Queued, None);
    }
    jobs::step(&job_id, STEP_TRANSCRIBE, JobState::Running, None);
```

3. Change the call to `let result = run_retranscription(app.clone(), job_id.clone(), meeting_id.clone(), meeting_folder_path, language, model, provider).await;`.
4. Change the signature:

```rust
async fn run_retranscription<R: Runtime>(
    app: AppHandle<R>,
    job_id: String,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionResult> {
```

- [ ] **Step 5: Keep the samples after VAD and run the step**

Replace the VAD block:

```rust
    let speech_segments = tokio::task::spawn_blocking(move || {
        get_speech_chunks_with_progress(
            &audio_samples,
```

with:

```rust
    let (audio_samples, speech_segments) = tokio::task::spawn_blocking(move || {
        get_speech_chunks_with_progress(
            &audio_samples,
```

and its tail:

```rust
        )
    })
    .await
    .map_err(|e| anyhow!("VAD task panicked: {}", e))?
    .map_err(|e| anyhow!("VAD processing failed: {}", e))?;
```

with:

```rust
        )
        .map(|segments| (audio_samples, segments))
    })
    .await
    .map_err(|e| anyhow!("VAD task panicked: {}", e))?
    .map_err(|e| anyhow!("VAD processing failed: {}", e))?;
```

Then replace the labelling block, from `// spec 4.4: split at speaker turns and remember who spoke, when the recording has an energy timeline` through the closing `}` of the `for segment in processable_segments { ... }` loop, with:

```rust
    // spec 4.4: split at speaker turns and remember who spoke, when the recording has an energy timeline
    let timeline = super::speaker_energy::EnergyTimeline::load(&folder_path);
    let refs = timeline.as_ref().map(|t| t.references());

    // spec 2026-10-29: cluster the remote side. Needs the timeline (owner mask), the models and the sidecar.
    let diar: Vec<super::diarization::DiarSegment> = match (&timeline, super::diarization::available(&app)) {
        (Some(t), Some((sidecar, models))) => {
            jobs::step(&job_id, STEP_TRANSCRIBE, JobState::Queued, None);
            jobs::step(&job_id, STEP_SPEAKERS, JobState::Running, None);
            emit_progress(&app, &meeting_id, "speakers", 25, "Identifying speakers...");
            match super::diarization::diarize(&job_id, &folder_path, audio_samples, t, sidecar, models).await {
                Ok(d) => {
                    info!("Speaker identification found {} remote speakers for {}", d.iter().map(|s| s.speaker.as_str()).collect::<std::collections::BTreeSet<_>>().len(), meeting_id);
                    jobs::step(&job_id, STEP_SPEAKERS, JobState::Done, None);
                    d
                }
                Err(_) if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) => {
                    jobs::step(&job_id, STEP_SPEAKERS, JobState::Cancelled, None);
                    return Err(anyhow!("Retranscription cancelled"));
                }
                Err(e) => {
                    // the transcript is the success criterion: carry on with plain `them` labels
                    warn!("Speaker identification failed for {}: {}", meeting_id, e);
                    let first_line = e.to_string().lines().next().unwrap_or("").to_string();
                    jobs::step(&job_id, STEP_SPEAKERS, JobState::Failed, Some(first_line));
                    Vec::new()
                }
            }
        }
        _ => Vec::new(),
    };
    jobs::step(&job_id, STEP_TRANSCRIBE, JobState::Running, None);

    let mut labelled_segments: Vec<(crate::audio::vad::SpeechSegment, Option<String>)> = Vec::new();
    for segment in processable_segments {
        match (&timeline, refs) {
            (Some(t), Some(r)) => {
                let cuts = t.turn_boundaries(segment.start_timestamp_ms, segment.end_timestamp_ms, r, 1000);
                for part in split_at_boundaries(&segment, &cuts) {
                    let label = t.label_span(part.start_timestamp_ms, part.end_timestamp_ms, r).map(|s| s.as_str().to_string());
                    if label.as_deref() == Some("them") && !diar.is_empty() {
                        // spec 2026-10-29: cut remote speech at speaker changes and name each piece after its cluster
                        let more = super::diarization::speaker_cuts(part.start_timestamp_ms, part.end_timestamp_ms, &diar);
                        for piece in split_at_boundaries(&part, &more) {
                            let who = super::diarization::label_for(piece.start_timestamp_ms, piece.end_timestamp_ms, &diar)
                                .unwrap_or_else(|| "them".to_string());
                            labelled_segments.push((piece, Some(who)));
                        }
                    } else {
                        labelled_segments.push((part, label));
                    }
                }
            }
            _ => labelled_segments.push((segment, None)),
        }
    }
```

If `audio_samples` is reported as unused when diarization is unavailable, that is fine: the `_` arm does not need it and Rust does not warn about a moved-in-one-arm value.

- [ ] **Step 6: Reset the names map and render from it**

In the `// spec 4.3: rewrite transcript.md with the enhanced segments` block, replace the `let speaker_names: Vec<(String, String)> = if segments.iter().any(...) { vec![...] } else { Vec::new() };` block written in Task 5 with:

```rust
        // spec 2026-10-29: me and them keep their names, cluster names reset to Speaker N
        let labelled: Vec<String> = segments.iter().filter_map(|s| s.speaker.clone()).collect();
        let speaker_names: Vec<(String, String)> = if labelled.is_empty() {
            Vec::new()
        } else {
            let owner = super::recording_preferences::load_recording_preferences(&app)
                .await
                .map(|p| p.owner_name)
                .unwrap_or_else(|_| "Me".to_string());
            let mut cluster_tags: Vec<String> = labelled.iter().filter(|t| super::speaker_names::is_cluster_tag(t)).cloned().collect();
            cluster_tags.sort();
            cluster_tags.dedup();
            let names = super::metadata_file::update_metadata(&folder_path, |obj| {
                let entry = obj.entry("speakers").or_insert_with(|| serde_json::Value::Object(Default::default()));
                if !entry.is_object() {
                    *entry = serde_json::Value::Object(Default::default());
                }
                let map = entry.as_object_mut().expect("object");
                super::speaker_names::reset_cluster_names(map, &owner, &cluster_tags);
                Ok(map.clone())
            })
            .unwrap_or_else(|e| {
                warn!("Could not update speaker names for {}: {}", meeting_id, e);
                Default::default()
            });
            super::speaker_names::ordered_names(&names, &labelled)
        };
```

This block must run before `let meta_json: serde_json::Value = std::fs::read_to_string(folder_path.join("metadata.json"))...` so the title and project are read after the map is written; move the `meta_json` read below it if it is not already.

- [ ] **Step 7: Build and run the enhance-related tests**

Run: `cd frontend/src-tauri && cargo test --lib retranscription 2>&1 | grep -E "^error|test result" | head -5`
Expected: `test result: ok`.
Run: `cd frontend/src-tauri && cargo test --lib 2>&1 | grep -E "test result|FAILED"`
Expected: one `test result: ok` line, `0 failed`.

- [ ] **Step 8: Live check in the dev app**

The owner's `tauri:dev` rebuilds on save. In the app: Settings, Models, download "Speaker identification" (Task 9 adds the row; until then run `curl -L -o` the two files into `~/Library/Application Support/com.noesis.app/models/speakers/` as `segmentation.onnx` and `embedding.onnx`). Right-click a meeting with audio, Enhance. Expected in the Jobs panel: `Speakers` first with a percentage, then `Transcribe`, then `Title/tags`. transcript.md ends up with `s1`, `s2` lines named `Speaker 1`, `Speaker 2`. Cancel during Speakers: the job ends Cancelled and `.speakers.wav` is gone from the folder.

- [ ] **Step 9: Commit**

```bash
/usr/bin/git add frontend/src-tauri/src/jobs.rs frontend/src-tauri/src/audio/retranscription.rs
/usr/bin/git commit -m "feat(enhance): Speakers step clusters remote speech before transcription"
```

---

### Task 8: MCP renderer reads the names map

**Files:**
- Modify: `mcp/src/library.ts:132-152, 173-180`
- Test: `mcp/test/library.test.ts` (after the test `readTranscript md fallback defaults names to Me and Others`)

**Interfaces:**
- Produces: `export type SpeakerNames = Record<string, string>; export function speakerName(tag, names): string | null; export function orderedNames(names, tags): Array<[string, string]>`; `renderTranscriptMd(info, model, segments, names?: SpeakerNames)`.

- [ ] **Step 1: Write the failing test**

Add after the `defaults names to Me and Others` test:

```ts
const clustered = { version: '1.0', total_segments: 5, segments: [
  { id: 'a', sequence_id: 0, text: 'Hello.', audio_start_time: 0, speaker: 'me' },
  { id: 'b', sequence_id: 1, text: 'Hi.', audio_start_time: 4, speaker: 's1' },
  { id: 'c', sequence_id: 2, text: 'Bonjour.', audio_start_time: 9, speaker: 's2' },
  { id: 'd', sequence_id: 3, text: 'Mm.', audio_start_time: 12, speaker: 'both' },
  { id: 'e', sequence_id: 4, text: 'Who.', audio_start_time: 15, speaker: 'them' },
] };

test('readTranscript md fallback names clusters from metadata and defaults the rest (spec 2026-10-29)', () => {
  const info = labelledMeeting('2026-10-27_14-03 c', { me: 'Alexandre', s2: 'Marie' }, clustered);
  const md = readTranscript(info, 'md');
  assert.match(md, /speakers: me=Alexandre, them=Others, s1=Speaker 1, s2=Marie\n/);
  assert.ok(md.endsWith('---\n[00:00] Alexandre: Hello.\n[00:04] Speaker 1: Hi.\n[00:09] Marie: Bonjour.\n[00:12] Mm.\n[00:15] Others: Who.\n'), md);
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd mcp && npm test 2>&1 | grep -E "^# (pass|fail)|not ok" | head -4`
Expected: `# fail 1`, the new test `not ok`.

- [ ] **Step 3: Implement**

In `mcp/src/library.ts`, replace the `renderTranscriptMd` function (from `/** spec 5.3, same bytes as the Rust renderer. */` to its closing `}`) with:

```ts
export type SpeakerNames = Record<string, string>;
const CLUSTER_TAG = /^s[1-9][0-9]*$/;

function defaultSpeakerName(tag: string): string | null {
  if (tag === 'me') return 'Me';
  if (tag === 'them') return 'Others';
  return CLUSTER_TAG.test(tag) ? `Speaker ${tag.slice(1)}` : null;
}

/** spec 2026-10-29: the map's name, else the default; nothing for `both` and unknown tags. Mirrors `speaker_names::display_name`. */
export function speakerName(tag: string | null | undefined, names: SpeakerNames): string | null {
  if (!tag || tag === 'both') return null;
  const fallback = defaultSpeakerName(tag);
  if (fallback === null) return null;
  return typeof names[tag] === 'string' ? names[tag] : fallback;
}

/** Front-matter order: me, them, then the cluster tags present, by number. Mirrors `speaker_names::ordered_names`. */
export function orderedNames(names: SpeakerNames, tags: string[]): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (const fixed of ['me', 'them']) out.push([fixed, speakerName(fixed, names)!]);
  const numbers = [...new Set(tags.filter((t) => CLUSTER_TAG.test(t)).map((t) => Number(t.slice(1))))].sort((a, b) => a - b);
  for (const n of numbers) out.push([`s${n}`, speakerName(`s${n}`, names)!]);
  return out;
}

/** spec 5.3, same bytes as the Rust renderer. `names` undefined means the meeting has no labels. */
export function renderTranscriptMd(info: MeetingInfo, model: string, segments: Array<{ start: number; text: string; speaker?: string | null }>, names?: SpeakerNames): string {
  let out = '---\n';
  out += `title: ${yamlString(info.title)}\n`;
  out += `project: ${info.project}\n`;
  out += `meeting_id: ${info.meeting_id ?? ''}\n`;
  out += `started_at: ${toLocalIso(info.started_at)}\n`;
  if (info.duration_seconds !== null) out += `duration_seconds: ${Math.round(info.duration_seconds)}\n`;
  out += `quality: ${info.quality}\n`;
  out += `model: ${model}\n`;
  if (names) {
    const tags = segments.map((s) => s.speaker).filter((t): t is string => typeof t === 'string');
    out += `speakers: ${orderedNames(names, tags).map(([t, n]) => `${t}=${n}`).join(', ')}\n`;
  }
  out += '---\n';
  for (const s of segments) {
    const line = s.text.split(/\s+/).filter(Boolean).join(' ');
    if (!line) continue;
    const name = names ? speakerName(s.speaker, names) : null;
    out += name ? `[${formatMmss(s.start)}] ${name}: ${line}\n` : `[${formatMmss(s.start)}] ${line}\n`;
  }
  return out;
}
```

In `readTranscript`, replace from `// spec 4.4: names only when some segment is labelled, like the Rust saver` through `return renderTranscriptMd(info, modelLabel(meta), segments, names);` with:

```ts
  // spec 4.4: names only when some segment is labelled, like the Rust saver
  const labelled = segments.some((s) => typeof s.speaker === 'string');
  const sp = meta?.speakers;
  const names: SpeakerNames | undefined = labelled ? (sp && typeof sp === 'object' ? (sp as SpeakerNames) : {}) : undefined;
  return renderTranscriptMd(info, modelLabel(meta), segments, names);
```

- [ ] **Step 4: Run the tests**

Run: `cd mcp && npm test 2>&1 | grep -E "^# (pass|fail)"`
Expected: `# pass 25`, `# fail 0` (the three older speaker tests still pass: `{me, them}` maps render as before, and a missing map still gives Me and Others).

- [ ] **Step 5: Commit**

```bash
/usr/bin/git add mcp/src/library.ts mcp/test/library.test.ts
/usr/bin/git commit -m "feat(mcp): transcript names from the speakers map with Speaker N defaults"
```

---

### Task 9: UI: names, rename, and the Models tab row

**Files:**
- Create: `frontend/src/lib/speakers.ts`, `frontend/src/lib/speakers.test.ts`
- Modify: `frontend/src/components/VirtualizedTranscriptView.tsx:10-11, 36-41, 56-62, 78-124, 126-144, 319-328, 377-386`
- Modify: `frontend/src/components/MeetingDetails/TranscriptPanel.tsx:84-97, 171-186`
- Modify: `frontend/src/components/ModelsSettings.tsx:18-23, 58-98, 239, 299`

**Interfaces:**
- Consumes: Tauri commands `get_meeting_extras` (now with `speakers`), `rename_speaker`, `speakers_models_status`, `speakers_download_models`, `speakers_cancel_download`, `speakers_delete_models`; events `speakers-model-download-progress`, `speakers-model-download-complete`, `speakers-model-download-error`, `meeting-speakers-changed`.
- Produces: `speakerName(tag, names)`, `speakerClass(tag)`, `canRename(tag)` in `lib/speakers.ts`; `VirtualizedTranscriptView` props `speakerNames?: SpeakerNames` and `onRenameSpeaker?: (tag: string, name: string) => void | Promise<void>`.

- [ ] **Step 1: Write the failing frontend test**

Create `frontend/src/lib/speakers.test.ts`:

```ts
// Run with `pnpm test` (Node's built-in runner, types stripped natively).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { speakerName, speakerClass, canRename } from './speakers.ts';

test('the map wins, then defaults, nothing for both or unknown tags', () => {
  assert.equal(speakerName('s2', { s2: 'Marie' }), 'Marie');
  assert.equal(speakerName('s3', {}), 'Speaker 3');
  assert.equal(speakerName('me', {}), 'Me');
  assert.equal(speakerName('mic', {}), 'Me');
  assert.equal(speakerName('them', { them: 'Client' }), 'Client');
  assert.equal(speakerName('system', {}), 'Others');
  assert.equal(speakerName('both', { both: 'x' }), null);
  assert.equal(speakerName('s0', {}), null);
  assert.equal(speakerName(null, {}), null);
});

test('colors: me is primary, clusters cycle a palette, others muted', () => {
  assert.equal(speakerClass('me'), 'text-primary');
  assert.equal(speakerClass('them'), 'text-muted-foreground');
  assert.notEqual(speakerClass('s1'), speakerClass('s2'));
  assert.equal(speakerClass('s1'), speakerClass('s7'));
});

test('rename applies to me, them and clusters only', () => {
  assert.ok(canRename('me') && canRename('them') && canRename('s4'));
  assert.ok(!canRename('both') && !canRename('mic') && !canRename('system') && !canRename(null) && !canRename(undefined));
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd frontend && pnpm test 2>&1 | grep -E "^# (pass|fail)|Cannot find"`
Expected: a module-not-found error for `./speakers.ts`.

- [ ] **Step 3: Write the helper**

Create `frontend/src/lib/speakers.ts`:

```ts
// Speaker tags (spec 2026-10-29): "me", "them", "both", "s1", "s2", ... Names come from
// metadata.json `speakers`; "mic" and "system" are upstream's older names for me and them.
export type SpeakerNames = Record<string, string>;
export const CLUSTER_TAG = /^s[1-9][0-9]*$/;

export function defaultSpeakerName(tag: string): string | null {
  if (tag === 'me' || tag === 'mic') return 'Me';
  if (tag === 'them' || tag === 'system') return 'Others';
  return CLUSTER_TAG.test(tag) ? `Speaker ${tag.slice(1)}` : null;
}

export function speakerName(tag: string | null | undefined, names: SpeakerNames = {}): string | null {
  if (!tag || tag === 'both') return null;
  const fallback = defaultSpeakerName(tag);
  if (fallback === null) return null;
  return typeof names[tag] === 'string' ? names[tag] : fallback;
}

// six distinct tones for clusters; Tailwind sees these literals in the source
const PALETTE = [
  'text-sky-600 dark:text-sky-400',
  'text-emerald-600 dark:text-emerald-400',
  'text-amber-600 dark:text-amber-400',
  'text-rose-600 dark:text-rose-400',
  'text-violet-600 dark:text-violet-400',
  'text-teal-600 dark:text-teal-400',
];

export function speakerClass(tag: string): string {
  if (tag === 'me' || tag === 'mic') return 'text-primary';
  if (CLUSTER_TAG.test(tag)) return PALETTE[(Number(tag.slice(1)) - 1) % PALETTE.length];
  return 'text-muted-foreground';
}

/** Tags the user may rename: me, them and clusters. Overlap and upstream's raw names are not renameable. */
export function canRename(tag: string | null | undefined): tag is string {
  return tag === 'me' || tag === 'them' || (typeof tag === 'string' && CLUSTER_TAG.test(tag));
}
```

Run: `cd frontend && pnpm test 2>&1 | grep -E "^# (pass|fail)"`
Expected: `# fail 0`.

- [ ] **Step 4: Labels and rename in the transcript view**

In `VirtualizedTranscriptView.tsx`:

1. After `import { hitSegmentIndex } from "@/lib/hitSegment";` add `import { canRename, speakerClass, speakerName, type SpeakerNames } from "@/lib/speakers";`.
2. In `VirtualizedTranscriptViewProps`, after the `highlight?` line add:

```ts
    /** spec 2026-10-29: tag -> name from metadata.json; defaults apply when absent */
    speakerNames?: SpeakerNames;
    /** When set, me, them and cluster labels become click-to-rename */
    onRenameSpeaker?: (tag: string, name: string) => void | Promise<void>;
```

3. Delete the `SPEAKER_LABEL` constant and its comment (lines 56-62).
4. Replace the `TranscriptSegment` component (from `const TranscriptSegment = memo(function TranscriptSegment({` to its closing `});`) with:

```tsx
const TranscriptSegment = memo(function TranscriptSegment({
    id,
    timestamp,
    text,
    speaker,
    speakerNames,
    onRenameSpeaker,
    confidence,
    isStreaming,
    showConfidence,
    flash = false,
}: {
    id: string;
    timestamp: number;
    text: string;
    speaker?: string | null;
    speakerNames?: SpeakerNames;
    onRenameSpeaker?: (tag: string, name: string) => void | Promise<void>;
    confidence?: number;
    isStreaming: boolean;
    showConfidence: boolean;
    flash?: boolean;
}) {
    const displayText = cleanStopWords(text) || (text.trim() === '' ? '[Silence]' : text);
    const label = speakerName(speaker, speakerNames);
    // the tag the click renames; null when renaming is off or the tag is not renameable
    const renameTag = onRenameSpeaker && canRename(speaker) ? speaker : null;
    const [draft, setDraft] = useState<string | null>(null);
    const labelClass = `text-[11px] font-semibold uppercase tracking-wide ${speaker ? speakerClass(speaker) : ''}`;

    return (
        <div id={`segment-${id}`} className={`grid grid-cols-[56px_1fr] gap-3 max-w-[72ch] pb-3.5 ${flash ? 'search-flash' : ''}`}>
            <Tooltip>
                <TooltipTrigger asChild>
                    <span className="font-mono text-xs leading-[1.6] text-muted-foreground tabular-nums text-left">
                        {formatRecordingTime(timestamp)}
                    </span>
                </TooltipTrigger>
                {confidence !== undefined && showConfidence && (
                    <TooltipContent>
                        <ConfidenceIndicator confidence={confidence} showIndicator={showConfidence} />
                    </TooltipContent>
                )}
            </Tooltip>
            <div className="min-w-0">
                {label && (renameTag ? (
                    draft === null ? (
                        <button type="button" title="Rename speaker" className={`${labelClass} hover:underline`} onClick={() => setDraft(label)}>
                            {label}
                        </button>
                    ) : (
                        <input
                            autoFocus
                            aria-label="Speaker name"
                            className="h-5 w-44 rounded border border-border bg-background px-1 text-[11px]"
                            value={draft}
                            onChange={(e) => setDraft(e.target.value)}
                            onKeyDown={(e) => {
                                if (e.key === 'Enter') e.currentTarget.blur();
                                if (e.key === 'Escape') { e.currentTarget.dataset.cancel = '1'; e.currentTarget.blur(); }
                            }}
                            onBlur={(e) => {
                                const next = e.currentTarget.dataset.cancel ? '' : draft.trim();
                                setDraft(null);
                                if (next && next !== label) void onRenameSpeaker!(renameTag, next);
                            }}
                        />
                    )
                ) : (
                    <span className={labelClass}>{label}</span>
                ))}
                <p className={`text-sm leading-normal ${label ? 'mt-0.5' : ''} ${isStreaming ? 'text-muted-foreground' : 'text-foreground'}`}>
                    {displayText}
                </p>
            </div>
        </div>
    );
});
```

5. In the `VirtualizedTranscriptView` destructuring, after `highlight,` add `speakerNames,` and `onRenameSpeaker,`.
6. At both `<TranscriptSegment` call sites (the virtualized one near line 319 and the plain one near line 377), after the `speaker={segment.speaker}` line add:

```tsx
                                        speakerNames={speakerNames}
                                        onRenameSpeaker={onRenameSpeaker}
```

- [ ] **Step 5: Wire the meeting page**

In `MeetingDetails/TranscriptPanel.tsx`:

1. Change both `{ tags: string[]; audio_deleted_at: string | null; has_audio: boolean }` types (the `useState` and the `invoke` generic) to `{ tags: string[]; audio_deleted_at: string | null; has_audio: boolean; speakers: Record<string, string> }`.
2. In the extras `useEffect`, after the `tagging-status-changed` listener add a second one so a rename from elsewhere refreshes too:

```ts
    let unlistenSpeakers: (() => void) | undefined;
    listen<{ meeting_id: string }>('meeting-speakers-changed', (e) => { if (e.payload.meeting_id === meetingId) load(); })
      .then((fn) => { if (cancelled) fn(); else unlistenSpeakers = fn; });
```

and call `unlistenSpeakers?.();` in the cleanup next to `unlisten?.();`.

3. After the `extras` effect add:

```ts
  // spec 2026-10-29: rename a speaker of this meeting; the event above reloads the names
  const enhanceLive = status?.enhanceStatus === 'queued' || status?.enhanceStatus === 'running';
  const renameSpeaker = async (tag: string, name: string) => {
    if (!meetingId) return;
    try {
      await invoke('rename_speaker', { meetingId, tag, name });
    } catch (error) {
      toast.error('Could not rename the speaker', { description: error instanceof Error ? error.message : String(error) });
    }
  };
```

4. In the `<VirtualizedTranscriptView ... />` element, after `highlight={highlight}` add:

```tsx
          speakerNames={extras?.speakers ?? {}}
          onRenameSpeaker={enhanceLive ? undefined : renameSpeaker}
```

- [ ] **Step 6: The Models tab row**

In `ModelsSettings.tsx`:

1. `type Engine = 'whisper' | 'parakeet' | 'builtin' | 'speakers';`
2. Add `speakers: 'speakers_download_models'` to `DOWNLOAD_CMD` and `speakers: 'speakers_cancel_download'` to `CANCEL_CMD`.
3. In `useModelRows.refresh`, fetch a fourth list and map it:

```ts
    const [whisper, parakeet, builtin, speakers] = await Promise.all([
      invoke<any[]>('whisper_get_available_models').catch(() => []),
      invoke<any[]>('parakeet_get_available_models').catch(() => []),
      invoke<any[]>('builtin_ai_list_models').catch(() => []),
      invoke<any[]>('speakers_models_status').catch(() => []),
    ]);
```

and append to the `setRows([...])` array:

```ts
      ...speakers.map((m): ModelRow => ({ engine: 'speakers', name: m.name, label: 'Speaker identification', description: 'Splits the remote side of a call into Speaker 1, Speaker 2, ... during enhance. pyannote segmentation 3.0 and 3D-Speaker CAM++.', sizeMb: m.size_mb, state: toState(m.status) })),
```

4. In the `subs` array add:

```ts
      listen<{ modelName: string; progress: number }>('speakers-model-download-progress', (e) => setProgress('speakers', e.payload.modelName, e.payload.progress)),
      listen('speakers-model-download-complete', refresh),
      listen<{ modelName: string; error: string }>('speakers-model-download-error', (e) => { refresh(); toast.error(`Download failed: ${e.payload.error}`); }),
```

5. Change `const transcriptionRows = rows.filter((r) => r.engine !== 'builtin');` to `const transcriptionRows = rows.filter((r) => r.engine === 'whisper' || r.engine === 'parakeet');` and add `const speakersRow = rows.find((r) => r.engine === 'speakers');`.
6. After the `</SettingsGroup>` that closes "Audio transcription" (just before `<SettingsGroup title="Summarization">`), add:

```tsx
      {/* spec 2026-10-29: downloaded means the Speakers step runs in every enhance */}
      <SettingsGroup title="Speakers">
        <SettingRow
          label="Speaker identification"
          hint="Names the remote voices Speaker 1, Speaker 2, ... after each enhance; rename them on the meeting page. Downloading turns it on, deleting turns it off."
          control={
            speakersRow && (
              <div className="flex items-center gap-1">
                <DownloadButton row={speakersRow} onChanged={refresh} />
                {speakersRow.state.kind === 'ready' && (
                  <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={() => invoke('speakers_delete_models').catch((e) => toast.error(String(e))).finally(refresh)}>
                    Delete
                  </Button>
                )}
              </div>
            )
          }
        />
      </SettingsGroup>
```

- [ ] **Step 7: Type-check and look at it**

Run: `cd frontend && ./node_modules/.bin/tsc --noEmit -p tsconfig.json && echo tsc clean`
Expected: `tsc clean`.
In the dev app: Settings, Models shows the "Speakers" group with a 36 MB download button; downloading shows a percentage and a Jobs panel entry; once ready, "Downloaded" and a Delete button. On an enhanced meeting with clusters, speaker labels show `Speaker 1` in a palette color; clicking one opens an input; Enter saves and the label, transcript.md and `get_transcript` from Claude Code show the new name; Escape leaves it unchanged. During an enhance of that meeting the labels are not clickable.

- [ ] **Step 8: Commit**

```bash
/usr/bin/git add frontend/src/lib/speakers.ts frontend/src/lib/speakers.test.ts frontend/src/components/VirtualizedTranscriptView.tsx frontend/src/components/MeetingDetails/TranscriptPanel.tsx frontend/src/components/ModelsSettings.tsx
/usr/bin/git commit -m "feat(ui): speaker names from metadata, click-to-rename, speaker models in the Models tab"
```

---

### Task 10: Docs, measurement, acceptance

**Files:**
- Modify: `CLAUDE.md:15` (the fork bullet list; add one bullet after the post-processing one)
- Modify: `docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md:198` (section 5 header)
- Modify: this plan's Outcome section

- [ ] **Step 1: Point the docs at the contract**

In `CLAUDE.md`, after the bullet that starts with `- Post-processing, the after-enhance hook`, add:

```markdown
- Speaker identification (`docs/superpowers/specs/2026-10-29-speaker-diarization-design.md`): the enhance job runs sherpa-onnx's diarization tool as a sidecar (`audio/diarization.rs`, binary from `scripts/fetch-diarizer.sh`, models from the Models tab via `audio/speaker_models.rs`) on the mono mix with the owner's windows silenced, and labels remote pieces `s1`, `s2`, ... Segment tags are `me`, `them`, `both`, `s<N>`; `metadata.json` `speakers` maps tags to names (`audio/speaker_names.rs`, `rename_speaker`); transcript.md and the MCP renderer read that map with `Me`, `Others`, `Speaker N` defaults. A re-enhance resets cluster names.
```

In the recorder spec, right after the line `## 5. Contracts`, add:

```markdown
> Amended on 2026-10-29 by `2026-10-29-speaker-diarization-design.md` (Contracts section): segment tags gain `s<N>`, `metadata.json` `speakers` becomes a tag-to-name map, and the transcript.md front matter lists every named tag.
```

- [ ] **Step 2: Measure on a real recording**

With the sidecar and models installed, run the ignored integration test on sherpa's sample once:

```bash
cd frontend/src-tauri
curl -sL -o /tmp/2-two-speakers-en.wav https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/2-two-speakers-en.wav
NOESIS_DIARIZE_SAMPLE=/tmp/2-two-speakers-en.wav cargo test --lib diarization::tests::sidecar_finds_two_speakers -- --ignored --nocapture 2>&1 | grep -E "segments|test result"
```

Expected: `N segments, speakers {"s1", "s2"}, X.Xs` and `ok`. Then, in the dev app, enhance one real meeting of at least 20 minutes and read the Speakers step's wall time from the Jobs panel or the log line `Speaker identification found`. Write both numbers into the Outcome section below.

- [ ] **Step 3: Acceptance checklist (owner, in the dev app)**

1. A call with at least two remote people: the transcript shows `Speaker 1` and `Speaker 2` on the remote side and your name on yours.
2. Rename `Speaker 2` to a name: the label changes, `transcript.md` carries it, and `get_transcript` from Claude Code shows it.
3. Cancel an enhance while `Speakers` runs: the job ends Cancelled and the meeting folder has no `.speakers.wav`.
4. Delete the speaker models in Settings, Models, then enhance: no `Speakers` step, labels are Me and Others.
5. A legacy meeting without clusters: rename `Others` to a person's name; the transcript and transcript.md follow.

- [ ] **Step 4: Run every suite once more**

```bash
cd frontend/src-tauri && cargo test --lib 2>&1 | grep -E "test result|FAILED"
cd ../../frontend && ./node_modules/.bin/tsc --noEmit -p tsconfig.json && pnpm test 2>&1 | grep -E "^# (pass|fail)"
cd ../mcp && npm test 2>&1 | grep -E "^# (pass|fail)"
```

Expected: `0 failed` everywhere.

- [ ] **Step 5: Commit**

```bash
/usr/bin/git add CLAUDE.md docs/superpowers/specs/2026-10-27-personal-meeting-recorder-design.md docs/superpowers/plans/2026-10-29-speaker-diarization.md
/usr/bin/git commit -m "docs: speaker identification contract pointers and measurements"
```

---

## Outcome

Filled in during execution.

- Sidecar binary size (Task 1, Step 2): _not measured yet_
- Sample diarization time (Task 10, Step 2): _not measured yet_
- Real meeting: duration, Speakers step wall time, speakers found: _not measured yet_
- Deviations from the plan: _none yet_
