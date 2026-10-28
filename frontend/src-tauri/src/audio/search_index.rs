//! spec phase 9 section 2: block-level full-text index over transcripts, titles and tags.

use sqlx::SqlitePool;

#[cfg(test)]
pub(crate) const SCHEMA: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(meeting_id UNINDEXED, start_seconds UNINDEXED, end_seconds UNINDEXED, title, tags, text, tokenize='unicode61 remove_diacritics 2')";

pub const BLOCK_SIZE: usize = 8;

pub struct Seg { pub start: f64, pub end: f64, pub text: String }
pub struct Block { pub start: f64, pub end: f64, pub text: String }

pub fn build_blocks(segs: &[Seg]) -> Vec<Block> {
    segs.chunks(BLOCK_SIZE)
        .map(|c| Block {
            start: c[0].start,
            end: c[c.len() - 1].end,
            text: c.iter().map(|s| s.text.trim()).collect::<Vec<_>>().join("\n"),
        })
        .collect()
}

/// Each term quoted so FTS5 syntax is literal, AND-ed, last term prefix-matched.
pub fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input.split_whitespace().map(|t| format!("\"{}\"", t.replace('"', "\"\""))).collect();
    if terms.is_empty() { return None; }
    Some(format!("{}*", terms.join(" AND ")))
}

pub fn metadata_tags(folder: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(folder.join("metadata.json")).ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("tags").and_then(|t| t.as_array()).cloned())
        .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// Replace one meeting's rows. Tags are stored space-joined in the `tags` column.
pub async fn insert_blocks(pool: &SqlitePool, meeting_id: &str, title: &str, tags: &[String], blocks: &[Block]) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM search_index WHERE meeting_id = ?").bind(meeting_id).execute(&mut *tx).await.map_err(|e| e.to_string())?;
    let tags_col = tags.join(" ");
    for b in blocks {
        sqlx::query("INSERT INTO search_index (meeting_id, start_seconds, end_seconds, title, tags, text) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(meeting_id).bind(b.start).bind(b.end).bind(title).bind(&tags_col).bind(&b.text)
            .execute(&mut *tx).await.map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())
}

pub async fn remove_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM search_index WHERE meeting_id = ?").bind(meeting_id).execute(pool).await.map(|_| ()).map_err(|e| e.to_string())
}

/// Reads segments from the `transcripts` table, title from `meetings`, tags from metadata.json.
pub async fn reindex_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<(), String> {
    let row: Option<(String, Option<String>)> = sqlx::query_as("SELECT title, folder_path FROM meetings WHERE id = ?")
        .bind(meeting_id).fetch_optional(pool).await.map_err(|e| e.to_string())?;
    let Some((title, folder)) = row else { return remove_meeting(pool, meeting_id).await };
    let rows: Vec<(String, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT transcript, audio_start_time, audio_end_time FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time, timestamp")
        .bind(meeting_id).fetch_all(pool).await.map_err(|e| e.to_string())?;
    let segs: Vec<Seg> = rows.into_iter().map(|(text, s, e)| Seg { start: s.unwrap_or(0.0), end: e.or(s).unwrap_or(0.0), text }).collect();
    let tags = folder.map(|f| metadata_tags(std::path::Path::new(&f))).unwrap_or_default();
    // a meeting with no transcript still gets one row so title and tag searches find it
    let mut blocks = build_blocks(&segs);
    if blocks.is_empty() { blocks.push(Block { start: 0.0, end: 0.0, text: String::new() }); }
    insert_blocks(pool, meeting_id, &title, &tags, &blocks).await
}

/// ponytail: full rebuild at startup; rebuild only rows whose meetings.updated_at moved if this gets slow.
pub async fn rebuild(pool: &SqlitePool) -> Result<usize, String> {
    let ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM meetings").fetch_all(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM search_index").execute(pool).await.map_err(|e| e.to_string())?;
    for (id,) in &ids {
        if let Err(e) = reindex_meeting(pool, id).await { log::warn!("search index: skip {}: {}", id, e); }
    }
    Ok(ids.len())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Hit { pub start_seconds: f64, pub end_seconds: f64, pub snippet: String, pub text_match: bool }
/// Title and tags are the same on every block of a meeting, so the best row's snippets say whether they matched.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MeetingHit { pub meeting_id: String, pub title: String, pub tags: Vec<String>, pub score: f64, pub hits: Vec<Hit>, pub title_match: bool, pub matched_tags: Vec<String> }

/// Tags whose words carry the snippet's ** marks (tags column, space-joined).
fn marked_tags(tags_snippet: &str) -> Vec<String> {
    tags_snippet.split_whitespace().filter(|t| t.contains("**")).map(|t| t.replace("**", "")).collect()
}

pub async fn search(pool: &SqlitePool, query: &str, limit: usize) -> Result<Vec<MeetingHit>, String> {
    let Some(q) = fts_query(query) else { return Ok(vec![]) };
    let rows: Vec<(String, f64, f64, String, String, String, f64, String, String)> = sqlx::query_as(
        "SELECT meeting_id, start_seconds, end_seconds, title, tags, \
         snippet(search_index, 5, '**', '**', '…', 24), bm25(search_index, 0, 0, 0, 5.0, 3.0, 1.0) AS score, \
         snippet(search_index, 3, '**', '**', '', 64), snippet(search_index, 4, '**', '**', '', 64) \
         FROM search_index WHERE search_index MATCH ? ORDER BY score LIMIT 500")
        .bind(&q).fetch_all(pool).await
        // search-as-you-type: an FTS5 syntax error (e.g. a lone quote) is "no results", not an error
        .unwrap_or_else(|e| { log::debug!("search index: query {:?} failed: {}", q, e); vec![] });
    let mut out: Vec<MeetingHit> = Vec::new();
    for (id, start, end, title, tags, snip, score, title_snip, tags_snip) in rows {
        let hit = Hit { start_seconds: start, end_seconds: end, text_match: snip.contains("**"), snippet: snip };
        match out.iter_mut().find(|m| m.meeting_id == id) {
            Some(m) => if m.hits.len() < 3 { m.hits.push(hit) },
            None => out.push(MeetingHit {
                meeting_id: id, title, score,
                tags: tags.split_whitespace().map(String::from).collect(),
                hits: vec![hit],
                title_match: title_snip.contains("**"),
                matched_tags: marked_tags(&tags_snip),
            }),
        }
    }
    out.truncate(limit);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::query(SCHEMA).execute(&pool).await.unwrap();
        pool
    }

    fn seg(start: f64, text: &str) -> Seg { Seg { start, end: start + 1.0, text: text.to_string() } }

    #[tokio::test]
    async fn fts5_is_available() {
        let p = pool().await;
        sqlx::query("INSERT INTO search_index (meeting_id, start_seconds, end_seconds, title, tags, text) VALUES ('m', 0, 1, 't', '', 'réunion budget')")
            .execute(&p).await.unwrap();
        let n: (i64,) = sqlx::query_as("SELECT count(*) FROM search_index WHERE search_index MATCH '\"reunion\"'")
            .fetch_one(&p).await.unwrap();
        assert_eq!(n.0, 1);
    }

    #[test]
    fn blocks_group_eight_segments_with_bounds() {
        let segs: Vec<Seg> = (0..10).map(|i| seg(i as f64 * 10.0, &format!("s{}", i))).collect();
        let b = build_blocks(&segs);
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].start, b[0].end), (0.0, 71.0));
        assert_eq!(b[0].text, "s0\ns1\ns2\ns3\ns4\ns5\ns6\ns7");
        assert_eq!((b[1].start, b[1].end), (80.0, 91.0));
        assert!(build_blocks(&[]).is_empty());
    }

    #[test]
    fn query_quotes_terms_and_prefixes_the_last() {
        assert_eq!(fts_query("  "), None);
        assert_eq!(fts_query("budget"), Some("\"budget\"*".to_string()));
        assert_eq!(fts_query("q3 \"plan\" NEAR -x"), Some("\"q3\" AND \"\"\"plan\"\"\" AND \"NEAR\" AND \"-x\"*".to_string()));
    }

    #[test]
    fn tags_from_metadata_ignore_non_strings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("metadata.json"), r#"{"tags":["budget",3,"wd"]}"#).unwrap();
        assert_eq!(metadata_tags(dir.path()), vec!["budget", "wd"]);
        assert!(metadata_tags(&dir.path().join("missing")).is_empty());
    }

    #[tokio::test]
    async fn title_hit_ranks_above_text_hit_and_hits_carry_times() {
        let p = pool().await;
        insert_blocks(&p, "m-text", "Weekly sync", &[], &[Block { start: 30.0, end: 60.0, text: "we discussed the budget".into() }]).await.unwrap();
        insert_blocks(&p, "m-title", "Budget review", &["finance".into()], &[Block { start: 0.0, end: 20.0, text: "hello".into() }]).await.unwrap();
        let r = search(&p, "budget", 10).await.unwrap();
        assert_eq!(r.iter().map(|m| m.meeting_id.as_str()).collect::<Vec<_>>(), vec!["m-title", "m-text"]);
        assert_eq!(r[1].hits[0].start_seconds, 30.0);
        assert!(r[1].hits[0].snippet.contains("**budget**"));
        assert!(r[0].title_match && r[0].matched_tags.is_empty() && !r[0].hits[0].text_match);
        assert!(!r[1].title_match && r[1].hits[0].text_match);
        let by_tag = search(&p, "finance", 10).await.unwrap();
        assert_eq!(by_tag[0].tags, vec!["finance"]);
        assert_eq!(by_tag[0].matched_tags, vec!["finance"]);
        assert!(!by_tag[0].title_match && !by_tag[0].hits[0].text_match);
        assert!(search(&p, "\"", 10).await.unwrap().is_empty());
    }
}
