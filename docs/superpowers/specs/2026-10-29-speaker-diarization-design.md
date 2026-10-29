# Speaker diarization

Date: 2026-10-29. Status: approved in brainstorming.

## Problem

Transcripts label lines Me or Others. The mic is you, so Me is right. Everything else arrives from the call as one mixed stream, so Others covers every remote voice at once. For a call with three people, who said what is lost.

Goal: after an enhance, remote speech is split into numbered speakers (Speaker 1, Speaker 2, ...) that can be renamed per meeting, in the app, in transcript.md and through the MCP server. Local only, no account, no network at run time beyond the one-time model download.

Out of scope: voiceprints that carry names across meetings, name suggestions from the calendar or the transcript, a separate remote audio stem, Windows and Linux binaries, keeping speaker names across a re-enhance, live labels during the recording.

## Decisions (brainstorming, 2026-10-29)

1. Engine: sherpa-onnx's own command-line diarization tool as a Tauri sidecar. Chosen over the Rust crate (a second ONNX runtime next to Parakeet's, dylib bundling) and over a pure Rust port on `ort` (a week of DSP to validate). Ceiling: the tool emits segments, not embeddings, so cross-meeting voiceprints would need a second tool later.
2. Input: the saved mono mix with the windows that belong to the owner silenced. Chosen over recording a separate remote stem, which would change the recording path and the folder contract and only help new recordings.
3. Scope: a minimal per-meeting rename is part of this phase. Numbered speakers alone are not useful enough.

## Design

### 1. Sidecar

`sherpa-onnx-offline-speaker-diarization` from the sherpa-onnx release pinned at `v2.0.1`, taken from the archive `sherpa-onnx-v2.0.1-osx-arm64-static.tar.bz2` (path `bin/sherpa-onnx-offline-speaker-diarization` inside it). It ships as `frontend/src-tauri/binaries/sherpa-onnx-offline-speaker-diarization-aarch64-apple-darwin`, listed in `bundle.externalBin` in `tauri.conf.json` next to `llama-helper`, gitignored like `llama-helper`.

`scripts/fetch-diarizer.sh` downloads the archive to a temporary directory, extracts that one file, verifies it is a Mach-O for the host architecture, and copies it into `binaries/` under the target-triple name. It is the dev and CI recipe; CI caches the archive. The script takes the version as its only argument and defaults to `v2.0.1`.

Invocation, one process per enhance:

```
sherpa-onnx-offline-speaker-diarization \
  --segmentation.pyannote-model=<models>/speakers/segmentation.onnx \
  --embedding.model=<models>/speakers/embedding.onnx \
  --clustering.num-clusters=-1 \
  --clustering.cluster-threshold=0.5 \
  --segmentation.num-threads=<N> \
  --embedding.num-threads=<N> \
  <meeting folder>/.speakers.wav
```

`N` is `available_parallelism() - 1`, clamped to 1..=8. stdout carries one line per segment, `0.031 -- 5.120 speaker_00`, in cluster order; stderr carries `progress 42.00%` lines and a final `Real time factor` line. A non-zero exit code is a failure; its stderr first line is the error text. Cancel kills the process (`SIGTERM`, then `SIGKILL` after 2 s).

Signing: the sidecar is signed and notarized with the app in the Developer ID step, the same way `llama-helper` is.

### 2. Models

A new model family `speakers`, one row "Speaker identification" in the Models tab (`ModelsSettings.tsx`), size shown as 36 MB. One download fetches two files into `<app data>/models/speakers/`:

| File | Source | Bytes | License |
|---|---|---|---|
| `segmentation.onnx` | `https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx` | 5,992,913 | MIT (pyannote/segmentation-3.0) |
| `embedding.onnx` | `https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx` | 29,596,978 | Apache-2.0 (3D-Speaker) |

The upstream release tag really is spelled `speaker-recongition-models`.

Download mechanics follow Parakeet's artifact downloader: exact byte sizes, `Range` resume, a `CancellationToken`, a `Control::SpeakersDownload` job in the queue, events `speakers-model-download-progress` and `speakers-model-download-complete` shaped like the Parakeet ones. A file whose size does not match after download is deleted and reported. Commands: `speakers_models_status` (both files present and the right size), `speakers_download_models`, `speakers_cancel_download`, `speakers_delete_models`.

No new preference. Both files present means the step runs; deleting the row's models turns it off. The Models tab row shows "Not downloaded", "Downloading", "Ready".

### 3. The Speakers step

A step named `Speakers` (`STEP_SPEAKERS`) in the enhance job, between VAD and transcription, so the order in the Jobs panel is Speakers, Transcribe, Title/tags; the job marks Transcribe running only after the Speakers step ends. It lives in a new module `audio/diarization.rs`; `retranscription.rs` calls it once.

Preconditions: both model files present, the sidecar present, `speakers.json` loadable. Any missing: no step row, nothing logged above info, labels as today.

Steps:

1. **Silence the owner.** From the 16 kHz mono samples the job already decoded and the `EnergyTimeline` from `speakers.json`: for every 50 ms window (800 samples at 16 kHz), `label_span` over that window alone with `references()`; a `Me` window is zeroed. `Them`, `Both` and all-noise windows are kept. The timeline's `window_ms` is read from the file, not assumed.
2. **Write the WAV.** `<folder>/.speakers.wav`, 16 kHz, mono, 16-bit PCM, written by a small writer in `diarization.rs` (44-byte header plus samples; no new crate). Deleted when the step ends, on every path.
3. **Run the sidecar** as above, with the child's pid kept in the module so cancel can reach it. Each `progress` line updates the job's progress with detail "Identifying speakers".
4. **Parse** stdout into `(start_s, end_s, cluster)` triples. A cluster whose summed duration is under 5.0 s is dropped. The remaining clusters are renumbered by the start of their earliest segment: `s1`, `s2`, ... .
5. **Split and label.** The existing me/them turn split still runs first. Every piece labelled `them` is then cut at remote speaker changes inside it, with the same rule as `turn_boundaries`: runs shorter than 1000 ms are absorbed into the neighbouring run; cutting uses `split_at_boundaries`. Each resulting piece gets the tag of the cluster with the largest time overlap, or stays `them` when nothing overlaps. `me` and `both` pieces are untouched.

Failure: the step is marked Failed with the error's first line; transcription continues with the labels from step 5 skipped, so every remote piece stays `them`. The transcript is the success criterion of the enhance, as before. Cancel: `stop_work` for an enhance job whose `Speakers` step is running calls `diarization::cancel()`, which kills the child; the step is Cancelled and the job ends Cancelled through the existing `RETRANSCRIPTION_CANCELLED` path.

Re-enhance recomputes clusters. Cluster numbers can change, so `s*` names in `metadata.json` are reset to `Speaker N`; `me` and `them` names are kept.

Expected cost: a few minutes of CPU per hour of audio on Apple Silicon. The first plan task measures it on a real recording and records the number in the plan.

### 4. Rename

On the meeting page every speaker label except `both` is a button. Click opens an inline input with the current name; Enter or blur saves, Escape cancels; empty is ignored. The button is disabled while an enhance job for that meeting is live.

Command `rename_speaker(meeting_id: String, tag: String, name: String) -> Result<(), String>`:

- `tag` must match `^(me|them|s[1-9][0-9]*)$`; `name` is trimmed, 1 to 60 characters, no line breaks.
- Updates `speakers[tag]` in `metadata.json` (read, modify, atomic write, as the other metadata writers do).
- Rewrites `transcript.md` from the stored segments with the existing writer, so the file matches the app.
- Emits `meeting-speakers-changed { meeting_id }`; the meeting page refetches its extras.

Renaming `me` or `them` changes that meeting only, which also serves legacy meetings without clusters: a 1:1 call can say "Marie" instead of Others. The global `owner_name` preference still seeds `me` at record time.

### 5. UI

`VirtualizedTranscriptView` takes a `speakerNames` map (from `get_meeting_extras`, which gains a `speakers` field) and derives the label: `speakerNames[tag]`, else `Me` for `me`, `Others` for `them`, `Speaker N` for `sN`, nothing for `both` or unknown tags. Colors: the current Me color for `me`, the current Others color for `them`, and a fixed palette of six colors cycled by `N` for `sN`.

The Jobs panel needs no change: steps and progress are generic.

## Contracts

Amendments to section 5 of `2026-10-27-personal-meeting-recorder-design.md`.

**Segment speaker tag** (SQLite `transcripts.speaker`, `transcripts.json` `speaker`): `me`, `them`, `both`, or `s<N>` with `N >= 1`, numbered by first appearance within the meeting. Readers treat any other value like `null`.

**metadata.json v2 `speakers`**: a map from tag to display name, written by the enhance with defaults and edited by rename:

```json
"speakers": { "me": "Alexandre", "them": "Others", "s1": "Speaker 1", "s2": "Marie" }
```

Readers fall back to `Me`, `Others` and `Speaker N` for missing keys. `speakers_file` is unchanged.

**transcript.md**: the front-matter line lists every tag that has a name, in the order `me`, `them`, `s1`, `s2`, ...: `speakers: me=Alexandre, them=Others, s1=Speaker 1, s2=Marie`. A line reads `[mm:ss] Marie: text`; `them` renders as the `them` name; `both` and unknown tags render without a name, as today.

**MCP**: `renderTranscriptMd` in `mcp/src/library.ts` maps every tag through `metadata.speakers` with the same fallbacks; the `md` format of `get_transcript` therefore carries names and the `json` format carries tags. Tool schemas are unchanged.

## Testing

Rust unit tests, in the modules they cover:

- `diarization`: owner windows are zeroed and others kept, with `window_ms` read from the timeline; a 2-second WAV round-trips through the writer with the right header fields; stdout parsing accepts the `start -- end speaker_NN` form and ignores other lines; clusters under 5 s are dropped and the rest renumbered by first appearance; remote pieces are split at speaker changes with runs under 1 s absorbed; a piece with no overlap stays `them`; `me` and `both` pieces are untouched.
- `transcript_md`: front matter and lines with `s1` and a renamed `s2`; `them` keeps its name; `both` stays unlabeled.
- metadata: enhance defaults keep existing `me` and `them` names and reset `s*` names; `rename_speaker` rejects bad tags, empty and over-long names.
- `jobs`: the `Speakers` step appears before `Transcribe`; cancel during `Speakers` ends the job Cancelled.
- MCP (`npm test`): `renderTranscriptMd` with `s*` names and with missing keys.

One Rust integration test, `#[ignore]` by default, runs the sidecar on sherpa-onnx's `2-two-speakers-en.wav` (1.1 MB, from the `speaker-segmentation-models` release) when the binary and the models are present, and asserts two clusters.

Manual acceptance: a real call with at least two remote people; the transcript shows numbered speakers, a rename shows in the app, in transcript.md and in `get_transcript` from Claude Code; cancelling during the Speakers step leaves no `.speakers.wav` behind.

## Risks

- Sidecar size: about 40 MB for arm64; the plan measures the extracted binary and records the number.
- Zoom's echo cancellation and compression, overlapping speech and short interjections lower accuracy. The 5 s cluster floor and the 1 s run minimum trade a little recall for fewer spurious speakers.
- Build-time download of the sherpa-onnx archive (250 MB) is a CI cost and, on this laptop, a Zscaler exposure; the fetch script accepts a pre-downloaded archive path to work around it.
