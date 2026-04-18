use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

use application::ports::{NoteRepository, RepoError};
use domain::entities::Note;

pub struct PgNoteRepository {
    pool: PgPool,
}

impl PgNoteRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(FromRow)]
struct NoteRow {
    id: String,
    title: String,
    content: String,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<NoteRow> for Note {
    fn from(r: NoteRow) -> Self {
        Note {
            id: r.id,
            title: r.title,
            content: r.content,
            tags: r.tags,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[async_trait]
impl NoteRepository for PgNoteRepository {
    async fn upsert(&self, note: &Note) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO notes (id, title, content, tags, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (id) DO UPDATE SET
                title      = EXCLUDED.title,
                content    = EXCLUDED.content,
                tags       = EXCLUDED.tags,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&note.id)
        .bind(&note.title)
        .bind(&note.content)
        .bind(&note.tags)
        .bind(note.created_at)
        .bind(note.updated_at)
        .execute(&self.pool)
        .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM notes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Note>, RepoError> {
        let row: Option<NoteRow> =
            sqlx::query_as("SELECT id, title, content, tags, created_at, updated_at FROM notes WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(row.map(Into::into))
    }

    async fn list(&self) -> Result<Vec<Note>, RepoError> {
        let rows: Vec<NoteRow> = sqlx::query_as(
            "SELECT id, title, content, tags, created_at, updated_at FROM notes ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn search(&self, query: &str) -> Result<Vec<Note>, RepoError> {
        let pattern = format!("%{query}%");
        let rows: Vec<NoteRow> = sqlx::query_as(
            r#"
            SELECT id, title, content, tags, created_at, updated_at
            FROM notes
            WHERE title ILIKE $1 OR content ILIKE $1
            ORDER BY updated_at DESC
            "#,
        )
        .bind(pattern)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}
