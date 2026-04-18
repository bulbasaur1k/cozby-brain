use std::sync::Arc;

use ractor::ActorRef;
use sqlx::PgPool;

use application::ports::{EmbeddingClient, LlmClient, VectorStore};
use actors::learning_actor::LearningMsg;
use actors::note_actor::NoteMsg;
use actors::reminder_actor::ReminderMsg;
use actors::todo_actor::TodoMsg;

#[derive(Clone)]
pub struct AppState {
    pub note_actor: ActorRef<NoteMsg>,
    pub todo_actor: ActorRef<TodoMsg>,
    pub reminder_actor: ActorRef<ReminderMsg>,
    pub learning_actor: ActorRef<LearningMsg>,
    pub llm: Arc<dyn LlmClient>,
    pub embedding: Arc<dyn EmbeddingClient>,
    pub vector: Arc<dyn VectorStore>,
    #[allow(dead_code)]
    pub db: PgPool,
}
