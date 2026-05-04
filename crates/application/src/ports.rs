use async_trait::async_trait;

use domain::entities::{
    Attachment, DocPage, DocPageVersion, LearningTrack, Lesson, Note, Project, Reminder, Todo,
};

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

/// Kind of a vector entry — determines how server interprets the payload
/// and where to fetch full content from when building RAG context.
pub const KIND_NOTE: &str = "note";
pub const KIND_DOC_PAGE: &str = "doc_page";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimilarItem {
    pub id: String,
    /// "note" | "doc_page"
    pub kind: String,
    pub title: String,
    pub score: f32,
}

/// Back-compat alias — older call sites still use this name.
pub type SimilarNote = SimilarItem;

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
        kind: &str,
        vector: Vec<f32>,
        title: &str,
        tags: &[String],
    ) -> Result<(), RepoError>;
    /// Search across ALL kinds. Caller can filter client-side by `kind`.
    async fn search(&self, vector: Vec<f32>, limit: usize)
        -> Result<Vec<SimilarItem>, RepoError>;
    /// Search restricted to a specific kind.
    async fn search_by_kind(
        &self,
        kind: &str,
        vector: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<SimilarItem>, RepoError>;
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

// ---------------- Documentation ----------------

#[async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn upsert(&self, project: &Project) -> Result<(), RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
    async fn get_by_id(&self, id: &str) -> Result<Option<Project>, RepoError>;
    async fn get_by_slug(&self, slug: &str) -> Result<Option<Project>, RepoError>;
    /// Fuzzy lookup — try exact slug, then title ILIKE.
    async fn find_by_title_like(&self, query: &str) -> Result<Vec<Project>, RepoError>;
    async fn list(&self) -> Result<Vec<Project>, RepoError>;
}

#[async_trait]
pub trait DocPageRepository: Send + Sync {
    async fn upsert(&self, page: &DocPage) -> Result<(), RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
    async fn get_by_id(&self, id: &str) -> Result<Option<DocPage>, RepoError>;
    async fn get_by_slug(
        &self,
        project_id: &str,
        slug: &str,
    ) -> Result<Option<DocPage>, RepoError>;
    /// Fuzzy lookup within a project — title ILIKE.
    async fn find_by_title_like(
        &self,
        project_id: &str,
        query: &str,
    ) -> Result<Vec<DocPage>, RepoError>;
    async fn list_by_project(&self, project_id: &str) -> Result<Vec<DocPage>, RepoError>;
    /// Global keyword search across all projects — matches title OR content (ILIKE).
    /// Used as fallback when vector search misses (Qdrant down or page not yet indexed).
    async fn search_all(&self, query: &str, limit: usize) -> Result<Vec<DocPage>, RepoError>;
}

#[async_trait]
pub trait DocPageHistoryRepository: Send + Sync {
    async fn insert(&self, version: &DocPageVersion) -> Result<(), RepoError>;
    async fn list_by_page(&self, page_id: &str) -> Result<Vec<DocPageVersion>, RepoError>;
    async fn get_version(
        &self,
        page_id: &str,
        version: i32,
    ) -> Result<Option<DocPageVersion>, RepoError>;
}

#[async_trait]
pub trait AttachmentRepository: Send + Sync {
    async fn insert(&self, attachment: &Attachment) -> Result<(), RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
    async fn get(&self, id: &str) -> Result<Option<Attachment>, RepoError>;
    async fn list_by_page(&self, page_id: &str) -> Result<Vec<Attachment>, RepoError>;
}

// ---------------- Attachment blob storage (S3/MinIO) ----------------

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("storage not configured")]
    NotConfigured,
    #[error("transport: {0}")]
    Transport(String),
    #[error("not found")]
    NotFound,
    #[error("other: {0}")]
    Other(String),
}

#[async_trait]
pub trait AttachmentStore: Send + Sync {
    fn name(&self) -> &str;
    /// Upload bytes under a key. Returns the stored key.
    async fn put(
        &self,
        key: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<String, StorageError>;
    /// Fetch bytes by key.
    async fn get(&self, key: &str) -> Result<Vec<u8>, StorageError>;
    /// Remove object by key.
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
    /// Optional — return a presigned URL for direct browser access (if supported).
    async fn presigned_url(&self, key: &str, ttl_secs: u64) -> Result<Option<String>, StorageError>;
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
