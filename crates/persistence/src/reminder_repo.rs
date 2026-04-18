use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

use application::ports::{ReminderRepository, RepoError};
use domain::entities::Reminder;

pub struct PgReminderRepository {
    pool: PgPool,
}

impl PgReminderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(FromRow)]
struct ReminderRow {
    id: String,
    text: String,
    remind_at: DateTime<Utc>,
    fired: bool,
    created_at: DateTime<Utc>,
}

impl From<ReminderRow> for Reminder {
    fn from(r: ReminderRow) -> Self {
        Reminder {
            id: r.id,
            text: r.text,
            remind_at: r.remind_at,
            fired: r.fired,
            created_at: r.created_at,
        }
    }
}

#[async_trait]
impl ReminderRepository for PgReminderRepository {
    async fn upsert(&self, reminder: &Reminder) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO reminders (id, text, remind_at, fired, created_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO UPDATE SET
                text      = EXCLUDED.text,
                remind_at = EXCLUDED.remind_at,
                fired     = EXCLUDED.fired
            "#,
        )
        .bind(&reminder.id)
        .bind(&reminder.text)
        .bind(reminder.remind_at)
        .bind(reminder.fired)
        .bind(reminder.created_at)
        .execute(&self.pool)
        .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM reminders WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<Reminder>, RepoError> {
        let rows: Vec<ReminderRow> = sqlx::query_as(
            "SELECT id, text, remind_at, fired, created_at FROM reminders ORDER BY remind_at ASC",
        )
        .fetch_all(&self.pool)
        .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn set_fired(&self, id: &str, fired: bool) -> Result<(), RepoError> {
        sqlx::query("UPDATE reminders SET fired = $1 WHERE id = $2")
            .bind(fired)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }
}
