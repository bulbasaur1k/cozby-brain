use std::sync::Arc;

use ractor::ActorRef;
use sqlx::PgPool;

use application::ports::{EmbeddingClient, LlmClient, VectorStore};
use actors::doc_actor::DocMsg;
use actors::learning_actor::LearningMsg;
use actors::note_actor::NoteMsg;
use actors::reminder_actor::ReminderMsg;
use actors::todo_actor::TodoMsg;
use application::ports::AttachmentStore;

#[derive(Clone)]
pub struct AppState {
    pub note_actor: ActorRef<NoteMsg>,
    pub todo_actor: ActorRef<TodoMsg>,
    pub reminder_actor: ActorRef<ReminderMsg>,
    pub learning_actor: ActorRef<LearningMsg>,
    pub doc_actor: ActorRef<DocMsg>,
    pub llm: Arc<dyn LlmClient>,
    pub embedding: Arc<dyn EmbeddingClient>,
    pub vector: Arc<dyn VectorStore>,
    pub attachments: Arc<dyn AttachmentStore>,
    #[allow(dead_code)]
    pub db: PgPool,
}
