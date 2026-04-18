use async_trait::async_trait;

use domain::entities::{LearningTrack, Lesson, Note, Reminder, Todo};

// ---------------- LLM port ----------------

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("llm not configured (missing API key)")]
    NotConfigured,
    #[error("transport: {0}")]
    Transport(String),
    #[error("api: {0}")]
    Api(String),
    #[error("bad response: {0}")]
    BadResponse(String),
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Name for logs (`anthropic`, `noop`, ...).
    fn name(&self) -> &str;

    /// Free-form text completion. System prompt + single user turn.
    async fn complete_text(&self, system: &str, user: &str) -> Result<String, LlmError>;
}

// ---------------- Embedding / Vector store ----------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimilarNote {
    pub id: String,
    pub title: String,
    pub score: f32,
}

#[async_trait]
pub trait EmbeddingClient: Send + Sync {
    fn name(&self) -> &str;
    async fn embed(&self, text: &str) -> Result<Vec<f32>, LlmError>;
}

#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(
        &self,
        id: &str,
        vector: Vec<f32>,
        title: &str,
        tags: &[String],
    ) -> Result<(), RepoError>;
    async fn search(&self, vector: Vec<f32>, limit: usize)
        -> Result<Vec<SimilarNote>, RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
}

// ---------------- Repositories ----------------

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("database: {0}")]
    Database(String),
    #[error("vector store: {0}")]
    Vector(String),
}

#[async_trait]
pub trait NoteRepository: Send + Sync {
    async fn upsert(&self, note: &Note) -> Result<(), RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
    async fn get(&self, id: &str) -> Result<Option<Note>, RepoError>;
    async fn list(&self) -> Result<Vec<Note>, RepoError>;
    async fn search(&self, query: &str) -> Result<Vec<Note>, RepoError>;
}

#[async_trait]
pub trait TodoRepository: Send + Sync {
    async fn upsert(&self, todo: &Todo) -> Result<(), RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
    async fn list(&self) -> Result<Vec<Todo>, RepoError>;
}

#[async_trait]
pub trait ReminderRepository: Send + Sync {
    async fn upsert(&self, reminder: &Reminder) -> Result<(), RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
    async fn list(&self) -> Result<Vec<Reminder>, RepoError>;
    async fn set_fired(&self, id: &str, fired: bool) -> Result<(), RepoError>;
}

#[async_trait]
pub trait LearningTrackRepository: Send + Sync {
    async fn upsert(&self, track: &LearningTrack) -> Result<(), RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
    async fn get(&self, id: &str) -> Result<Option<LearningTrack>, RepoError>;
    async fn list(&self) -> Result<Vec<LearningTrack>, RepoError>;
}

#[async_trait]
pub trait LessonRepository: Send + Sync {
    async fn upsert(&self, lesson: &Lesson) -> Result<(), RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
    async fn get(&self, id: &str) -> Result<Option<Lesson>, RepoError>;
    async fn list_by_track(&self, track_id: &str) -> Result<Vec<Lesson>, RepoError>;
    /// Next pending lesson for a track (ordered by lesson_num).
    async fn next_pending(&self, track_id: &str) -> Result<Option<Lesson>, RepoError>;
}

// ---------------- Lesson splitter port (MCP-like) ----------------

#[derive(Debug, Clone)]
pub struct LessonDraft {
    pub title: String,
    pub content: String,
}

#[async_trait]
pub trait LessonSplitter: Send + Sync {
    /// Splits raw text into a sequence of lessons using LLM.
    async fn split(&self, track_title: &str, raw_text: &str) -> Result<Vec<LessonDraft>, LlmError>;
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    #[error("notifier failed: {0}")]
    Other(String),
}

#[async_trait]
pub trait Notifier: Send + Sync {
    fn name(&self) -> &str;
    async fn notify(&self, n: &Notification) -> Result<(), NotifyError>;
}
