//! spec 4.4: who is speaking, from energy alone. The microphone is the owner ("me"),
//! system audio is everyone else ("them"). The pipeline records the RMS of both
//! streams per 50 ms window while they are still separate; this module turns that
//! timeline into labels and into split points at speaker turns.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const WINDOW_MS: u32 = 50;
const NOISE_FLOOR: f32 = 0.1;
const ME_SHARE: f32 = 0.7;
const THEM_SHARE: f32 = 0.3;
/// A stream that never rises above this RMS is treated as silent: its windows normalize
/// against the floor and fall under NOISE_FLOOR instead of being inflated by a tiny reference.
/// Trade-off: remote speech that never exceeds this level for a whole meeting is scored against the floor and can come out as "both" when the microphone also hears it.
const REFERENCE_FLOOR: f32 = 0.01;

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
        let mic_rms = if mic_rms.is_finite() { mic_rms } else { 0.0 };
        let sys_rms = if sys_rms.is_finite() { sys_rms } else { 0.0 };
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
        let mic: Vec<f32> = self.windows.iter().map(|w| w.0).filter(|m| *m > REFERENCE_FLOOR).collect();
        let sys: Vec<f32> = self.windows.iter().map(|w| w.1).filter(|s| *s > REFERENCE_FLOOR).collect();
        (p95(mic), p95(sys))
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
        // Backfill leading unknowns with the first known dominant so run lengths cover every window
        if let Some(first) = dominant.iter().flatten().next().copied() {
            for d in dominant.iter_mut().take_while(|d| d.is_none()) {
                *d = Some(first);
            }
        }
        // Run-length encode (all-noise ranges produce no runs)
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
                serde_json::json!([((*m as f64) * 100000.0).round() / 100000.0, ((*s as f64) * 100000.0).round() / 100000.0])
            }).collect::<Vec<_>>(),
        });
        let tmp = folder.join(".speakers.json.tmp");
        std::fs::write(&tmp, serde_json::to_string(&json)?)?;
        std::fs::rename(&tmp, folder.join("speakers.json"))?;
        Ok(())
    }

    /// Read speakers.json; None when missing or unreadable.
    pub fn load(folder: &Path) -> Option<EnergyTimeline> {
        let text = std::fs::read_to_string(folder.join("speakers.json")).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let window_ms = u32::try_from(v.get("window_ms")?.as_u64()?).ok().filter(|w| *w > 0)?;
        let mut t = EnergyTimeline { window_ms, ..EnergyTimeline::new() };
        for w in v.get("windows")?.as_array()? {
            let a = w.as_array()?;
            t.push(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32);
        }
        Some(t)
    }
}

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
        assert_eq!(timeline("....").references(), (0.01, 0.01), "silence falls back to a floor");
    }

    #[test]
    fn faint_system_stream_does_not_steal_the_label() {
        // Mic speaking normally, system audio never silent but far below speech (ScreenCaptureKit hiss)
        let mut t = EnergyTimeline::new();
        for _ in 0..40 {
            t.push(0.20, 0.0005);
        }
        assert_eq!(t.label_span(0.0, 2000.0, t.references()), Some(Speaker::Me));
        assert_eq!(t.label_span(0.0, 2000.0, t.running_references()), Some(Speaker::Me));
        assert!(t.turn_boundaries(0.0, 2000.0, t.references(), 1000).is_empty());
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
        assert!(text.contains("\"window_ms\":50") && text.contains("\"streams\""));
        assert!(text.contains("0.2") && text.contains("0.08") && !text.contains("0.20000000298"));
        let back = EnergyTimeline::load(dir.path()).unwrap();
        assert_eq!(back.windows.len(), 4);
        assert_eq!(back.references(), t.references());
        assert!(EnergyTimeline::load(Path::new("/nonexistent/folder")).is_none());
    }

    #[test]
    fn leading_noise_does_not_shift_boundaries() {
        let t = timeline(&format!("....{}{}", "m".repeat(40), "t".repeat(40)));
        let b = t.turn_boundaries(0.0, 84.0 * 50.0, t.references(), 1000);
        assert_eq!(b, vec![44.0 * 50.0]);
    }

    #[test]
    fn load_rejects_zero_window() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("speakers.json"),
            r#"{"version":1,"window_ms":0,"streams":["me","them"],"windows":[[0.2,0.01]]}"#,
        )
        .unwrap();
        assert!(EnergyTimeline::load(dir.path()).is_none());
    }
}
