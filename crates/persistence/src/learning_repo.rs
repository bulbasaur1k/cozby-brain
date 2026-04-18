use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

use application::ports::{LearningTrackRepository, LessonRepository, RepoError};
use domain::entities::{LearningTrack, Lesson, LessonStatus};

// ---------------- LearningTrack ----------------

pub struct PgLearningTrackRepository {
    pool: PgPool,
}

impl PgLearningTrackRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(FromRow)]
struct TrackRow {
    id: String,
    title: String,
    source_ref: String,
    total_lessons: i32,
    current_lesson: i32,
    pace_hours: i32,
    last_delivered_at: Option<DateTime<Utc>>,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
}

impl From<TrackRow> for LearningTrack {
    fn from(r: TrackRow) -> Self {
        LearningTrack {
            id: r.id,
            title: r.title,
            source_ref: r.source_ref,
            total_lessons: r.total_lessons,
            current_lesson: r.current_lesson,
            pace_hours: r.pace_hours,
            last_delivered_at: r.last_delivered_at,
            tags: r.tags,
            created_at: r.created_at,
        }
    }
}

#[async_trait]
impl LearningTrackRepository for PgLearningTrackRepository {
    async fn upsert(&self, track: &LearningTrack) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO learning_tracks (
                id, title, source_ref, total_lessons, current_lesson,
                pace_hours, last_delivered_at, tags, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (id) DO UPDATE SET
                title             = EXCLUDED.title,
                source_ref        = EXCLUDED.source_ref,
                total_lessons     = EXCLUDED.total_lessons,
                current_lesson    = EXCLUDED.current_lesson,
                pace_hours        = EXCLUDED.pace_hours,
                last_delivered_at = EXCLUDED.last_delivered_at,
                tags              = EXCLUDED.tags
            "#,
        )
        .bind(&track.id)
        .bind(&track.title)
        .bind(&track.source_ref)
        .bind(track.total_lessons)
        .bind(track.current_lesson)
        .bind(track.pace_hours)
        .bind(track.last_delivered_at)
        .bind(&track.tags)
        .bind(track.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM learning_tracks WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<LearningTrack>, RepoError> {
        let row: Option<TrackRow> = sqlx::query_as(
            r#"SELECT id, title, source_ref, total_lessons, current_lesson,
                      pace_hours, last_delivered_at, tags, created_at
               FROM learning_tracks WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(row.map(Into::into))
    }

    async fn list(&self) -> Result<Vec<LearningTrack>, RepoError> {
        let rows: Vec<TrackRow> = sqlx::query_as(
            r#"SELECT id, title, source_ref, total_lessons, current_lesson,
                      pace_hours, last_delivered_at, tags, created_at
               FROM learning_tracks ORDER BY created_at DESC"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}

// ---------------- Lesson ----------------

pub struct PgLessonRepository {
    pool: PgPool,
}

impl PgLessonRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(FromRow)]
struct LessonRow {
    id: String,
    track_id: String,
    lesson_num: i32,
    title: String,
    content: String,
    status: String,
    delivered_at: Option<DateTime<Utc>>,
    learned_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<LessonRow> for Lesson {
    fn from(r: LessonRow) -> Self {
        Lesson {
            id: r.id,
            track_id: r.track_id,
            lesson_num: r.lesson_num,
            title: r.title,
            content: r.content,
            status: LessonStatus::parse(&r.status).unwrap_or(LessonStatus::Pending),
            delivered_at: r.delivered_at,
            learned_at: r.learned_at,
            created_at: r.created_at,
        }
    }
}

#[async_trait]
impl LessonRepository for PgLessonRepository {
    async fn upsert(&self, lesson: &Lesson) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO lessons (
                id, track_id, lesson_num, title, content,
                status, delivered_at, learned_at, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (id) DO UPDATE SET
                title        = EXCLUDED.title,
                content      = EXCLUDED.content,
                status       = EXCLUDED.status,
                delivered_at = EXCLUDED.delivered_at,
                learned_at   = EXCLUDED.learned_at
            "#,
        )
        .bind(&lesson.id)
        .bind(&lesson.track_id)
        .bind(lesson.lesson_num)
        .bind(&lesson.title)
        .bind(&lesson.content)
        .bind(lesson.status.as_str())
        .bind(lesson.delivered_at)
        .bind(lesson.learned_at)
        .bind(lesson.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM lessons WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Lesson>, RepoError> {
        let row: Option<LessonRow> = sqlx::query_as(
            r#"SELECT id, track_id, lesson_num, title, content,
                      status, delivered_at, learned_at, created_at
               FROM lessons WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(row.map(Into::into))
    }

    async fn list_by_track(&self, track_id: &str) -> Result<Vec<Lesson>, RepoError> {
        let rows: Vec<LessonRow> = sqlx::query_as(
            r#"SELECT id, track_id, lesson_num, title, content,
                      status, delivered_at, learned_at, created_at
               FROM lessons WHERE track_id = $1 ORDER BY lesson_num ASC"#,
        )
        .bind(track_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn next_pending(&self, track_id: &str) -> Result<Option<Lesson>, RepoError> {
        let row: Option<LessonRow> = sqlx::query_as(
            r#"SELECT id, track_id, lesson_num, title, content,
                      status, delivered_at, learned_at, created_at
               FROM lessons
               WHERE track_id = $1 AND status = 'pending'
               ORDER BY lesson_num ASC LIMIT 1"#,
        )
        .bind(track_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(row.map(Into::into))
    }
}
