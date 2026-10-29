//! spec 4.3 / 5.3: the transcript.md contract. One function renders it, one
//! writes it atomically. Consumers (the MCP server, your own tools) read this file
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
    /// spec 4.4: (name for "me", name for "them"); None when the meeting has no labels
    pub speaker_names: Option<(&'a str, &'a str)>,
    /// phase 9: topic tags from metadata.json; the line is omitted when empty
    pub tags: &'a [String],
}

/// spec 4.4
pub struct MdSegment {
    pub start: f64,
    /// "me", "them", "both" or None
    pub speaker: Option<String>,
    pub text: String,
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

pub fn render_transcript_md(meta: &TranscriptMdMeta, segments: &[MdSegment]) -> String {
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
    // spec 4.4
    if let Some((me, them)) = meta.speaker_names {
        out.push_str(&format!("speakers: me={}, them={}\n", me, them));
    }
    if !meta.tags.is_empty() {
        out.push_str(&format!("tags: [{}]\n", meta.tags.join(", ")));
    }
    out.push_str("---\n");
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
    out
}

/// Write `transcript.md` through a temp file and rename.
pub fn write_transcript_md(folder: &Path, meta: &TranscriptMdMeta, segments: &[MdSegment]) -> Result<()> {
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
            speaker_names: None,
            tags: &[],
        };
        let segments = vec![
            MdSegment { start: 0.0, speaker: None, text: "Thank you. Hello.".to_string() },
            MdSegment { start: 4.2, speaker: None, text: "Yes, it's been\na long time.".to_string() },
        ];
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
            speaker_names: None,
            tags: &[],
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
            speaker_names: None,
            tags: &[],
        };
        write_transcript_md(dir.path(), &meta, &[MdSegment { start: 0.0, speaker: None, text: "hi".into() }]).unwrap();
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

    #[test]
    fn tags_render_after_speakers_before_the_closing_rule() {
        let tags = vec!["a".to_string(), "b".to_string()];
        let meta = TranscriptMdMeta {
            title: "t", project: "dex", meeting_id: "m", started_at: "2026-10-27T12:03:09-04:00",
            duration_seconds: None, quality: "enhanced", model: "whisper large-v3",
            speaker_names: Some(("Me", "Others")),
            tags: &tags,
        };
        let md = render_transcript_md(&meta, &[]);
        assert!(md.contains("speakers: me=Me, them=Others\ntags: [a, b]\n---\n"), "{}", md);
    }

    // spec 4.4
    #[test]
    fn labelled_lines_carry_names_and_frontmatter_lists_speakers() {
        let meta = TranscriptMdMeta {
            title: "t", project: "dex", meeting_id: "m", started_at: "2026-10-27T12:03:09-04:00",
            duration_seconds: None, quality: "enhanced", model: "whisper large-v3",
            speaker_names: Some(("Alexandre", "Others")),
            tags: &[],
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
}
