use crate::api::{TranscriptSearchResult, TranscriptSegment};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqlitePool};
use tracing::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
        meeting_id: Option<String>, // spec 4.2: reuse the id minted at record start
    ) -> Result<String, SqlxError> {
        let meeting_id = meeting_id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("meeting-{}", Uuid::new_v4()));

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now();

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting '{}': {}", meeting_title, e);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields
        for segment in transcripts {
            let transcript_id = format!("transcript-{}", Uuid::new_v4());
            let result = sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&transcript_id)
            .bind(&meeting_id)
            .bind(&segment.text)
            .bind(&segment.timestamp)
            .bind(segment.audio_start_time)
            .bind(segment.audio_end_time)
            .bind(segment.duration)
            .bind(&segment.speaker) // spec 4.4
            .execute(&mut *transaction)
            .await;

            if let Err(e) = result {
                error!(
                    "Failed to save transcript segment for meeting {}: {}",
                    meeting_id, e
                );
                transaction.rollback().await?;
                return Err(e);
            }
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        // Commit the transaction
        transaction.commit().await?;

        Ok(meeting_id)
    }

    /// Ranked search over titles, tags and transcript blocks (audio::search_index).
    /// One result per meeting; the best-matching block supplies the snippet and the timestamp.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        let hits = crate::audio::search_index::search(pool, query, 50)
            .await
            .map_err(SqlxError::Protocol)?;

        let mut results = Vec::with_capacity(hits.len());
        for m in hits {
            let created_at: Option<(String,)> =
                sqlx::query_as("SELECT CAST(created_at AS TEXT) FROM meetings WHERE id = ?")
                    .bind(&m.meeting_id)
                    .fetch_optional(pool)
                    .await?;
            let first = m.hits.first();
            results.push(TranscriptSearchResult {
                id: m.meeting_id,
                title: m.title,
                match_context: first.map(|h| h.snippet.clone()).unwrap_or_default(),
                timestamp: created_at.map(|c| c.0).unwrap_or_default(),
                tags: m.tags,
                start_seconds: first.map(|h| h.start_seconds).unwrap_or(0.0),
                hits: m.hits,
            });
        }
        Ok(results)
    }
}
