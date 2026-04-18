use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

use application::ports::{RepoError, TodoRepository};
use domain::entities::Todo;

pub struct PgTodoRepository {
    pool: PgPool,
}

impl PgTodoRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(FromRow)]
struct TodoRow {
    id: String,
    title: String,
    done: bool,
    due_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

impl From<TodoRow> for Todo {
    fn from(r: TodoRow) -> Self {
        Todo {
            id: r.id,
            title: r.title,
            done: r.done,
            due_at: r.due_at,
            created_at: r.created_at,
            completed_at: r.completed_at,
        }
    }
}

#[async_trait]
impl TodoRepository for PgTodoRepository {
    async fn upsert(&self, todo: &Todo) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO todos (id, title, done, due_at, created_at, completed_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (id) DO UPDATE SET
                title        = EXCLUDED.title,
                done         = EXCLUDED.done,
                due_at       = EXCLUDED.due_at,
                completed_at = EXCLUDED.completed_at
            "#,
        )
        .bind(&todo.id)
        .bind(&todo.title)
        .bind(todo.done)
        .bind(todo.due_at)
        .bind(todo.created_at)
        .bind(todo.completed_at)
        .execute(&self.pool)
        .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM todos WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<Todo>, RepoError> {
        let rows: Vec<TodoRow> = sqlx::query_as(
            r#"
            SELECT id, title, done, due_at, created_at, completed_at
            FROM todos
            ORDER BY done ASC, COALESCE(due_at, created_at) ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}
