use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CreateNoteDto {
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNoteDto {
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateTodoDto {
    pub title: String,
    pub due_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateReminderDto {
    pub text: String,
    pub remind_at: DateTime<Utc>,
}

/// Raw user text for LLM-powered ingestion (notes / todos / reminders).
#[derive(Debug, Deserialize)]
pub struct IngestRawDto {
    pub raw: String,
}

#[derive(Debug, Deserialize)]
pub struct AskQuery {
    pub q: String,
}

#[derive(Debug, serde::Serialize)]
pub struct StructuredNoteDto {
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct StructuredTodoDto {
    pub title: String,
    pub due_at: Option<DateTime<Utc>>,
}

#[derive(Debug, serde::Serialize)]
pub struct StructuredReminderDto {
    pub text: String,
    pub remind_at: DateTime<Utc>,
}

/// Confirm step after ingest — user decides create or append (for notes).
#[derive(Debug, Deserialize)]
pub struct ConfirmIngestNoteDto {
    /// "create" or "append"
    pub action: String,
    /// Required for "append"
    pub target_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
}
