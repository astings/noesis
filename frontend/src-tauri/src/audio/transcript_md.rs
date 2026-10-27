//! spec 4.3 / 5.3: the transcript.md contract. One function renders it, one
//! writes it atomically. Consumers (the wikis, the MCP server) read this file
//! and ignore frontmatter keys they do not know.

use anyhow::Result;
use std::path::Path;

pub struct TranscriptMdMeta<'a> {
    pub title: &'a str,
    pub project: &'a str,
    pub meeting_id: &'a str,
    /// ISO 8601 with offset, already in the zone you want shown
    pub started_at: &'a str,
    pub duration_seconds: Option<f64>,
    /// "live" or "enhanced"
    pub quality: &'a str,
    /// e.g. "whisper large-v3"
    pub model: &'a str,
}

/// `[mm:ss]` style time; minutes are not wrapped at 60 so a 2 hour call reads 120:00.
pub fn format_mmss(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// A UTC RFC 3339 string rendered in local time with its offset. Unparseable input is returned as is.
pub fn local_iso(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|d| d.with_timezone(&chrono::Local).to_rfc3339_opts(chrono::SecondsFormat::Secs, false))
        .unwrap_or_else(|_| rfc3339.to_string())
}

fn yaml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn render_transcript_md(meta: &TranscriptMdMeta, segments: &[(f64, String)]) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("title: {}\n", yaml_string(meta.title)));
    out.push_str(&format!("project: {}\n", meta.project));
    out.push_str(&format!("meeting_id: {}\n", meta.meeting_id));
    out.push_str(&format!("started_at: {}\n", meta.started_at));
    if let Some(d) = meta.duration_seconds {
        out.push_str(&format!("duration_seconds: {}\n", d.round() as i64));
    }
    out.push_str(&format!("quality: {}\n", meta.quality));
    out.push_str(&format!("model: {}\n", meta.model));
    out.push_str("---\n");
    for (start, text) in segments {
        let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if one_line.is_empty() {
            continue;
        }
        out.push_str(&format!("[{}] {}\n", format_mmss(*start), one_line));
    }
    out
}

/// Write `transcript.md` through a temp file and rename.
pub fn write_transcript_md(folder: &Path, meta: &TranscriptMdMeta, segments: &[(f64, String)]) -> Result<()> {
    let tmp = folder.join(".transcript.md.tmp");
    std::fs::write(&tmp, render_transcript_md(meta, segments))?;
    std::fs::rename(&tmp, folder.join("transcript.md"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mmss_pads_minutes_and_seconds() {
        assert_eq!(format_mmss(0.0), "00:00");
        assert_eq!(format_mmss(65.4), "01:05");
        assert_eq!(format_mmss(3599.9), "59:59");
        assert_eq!(format_mmss(3600.0), "60:00");
    }

    #[test]
    fn renders_frontmatter_and_one_line_per_segment() {
        let meta = TranscriptMdMeta {
            title: "Western \"Digital\" onboarding",
            project: "dex",
            meeting_id: "meeting-a",
            started_at: "2026-10-26T14:30:05-04:00",
            duration_seconds: Some(2555.2),
            quality: "enhanced",
            model: "whisper large-v3",
        };
        let segments = vec![(0.0, "Thank you. Hello.".to_string()), (4.2, "Yes, it's been\na long time.".to_string())];
        let md = render_transcript_md(&meta, &segments);
        let expected = "---\n\
title: \"Western \\\"Digital\\\" onboarding\"\n\
project: dex\n\
meeting_id: meeting-a\n\
started_at: 2026-10-26T14:30:05-04:00\n\
duration_seconds: 2555\n\
quality: enhanced\n\
model: whisper large-v3\n\
---\n\
[00:00] Thank you. Hello.\n\
[00:04] Yes, it's been a long time.\n";
        assert_eq!(md, expected);
    }

    #[test]
    fn omits_duration_when_unknown_and_handles_no_segments() {
        let meta = TranscriptMdMeta {
            title: "t", project: "_inbox", meeting_id: "m", started_at: "2026-10-27T12:03:09+00:00",
            duration_seconds: None, quality: "live", model: "parakeet parakeet-tdt-0.6b-v3-int8",
        };
        let md = render_transcript_md(&meta, &[]);
        assert!(!md.contains("duration_seconds"));
        assert!(md.ends_with("---\n"));
    }

    #[test]
    fn write_is_atomic_and_readable() {
        let dir = tempfile::tempdir().unwrap();
        let meta = TranscriptMdMeta {
            title: "t", project: "p", meeting_id: "m", started_at: "2026-10-27T12:03:09+00:00",
            duration_seconds: Some(1.0), quality: "live", model: "x",
        };
        write_transcript_md(dir.path(), &meta, &[(0.0, "hi".into())]).unwrap();
        let text = std::fs::read_to_string(dir.path().join("transcript.md")).unwrap();
        assert!(text.contains("[00:00] hi"));
        assert!(!dir.path().join(".transcript.md.tmp").exists());
    }

    #[test]
    fn local_iso_converts_utc_and_passes_through_garbage() {
        let out = local_iso("2026-10-27T16:03:09.210153+00:00");
        assert!(out.starts_with("2026-10-27T"), "{}", out);
        assert!(!out.ends_with("Z") || out.ends_with("+00:00"), "has an offset: {}", out);
        assert_eq!(local_iso("not a date"), "not a date");
    }
}
