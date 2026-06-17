use std::sync::Arc;

use ractor::ActorRef;
use sqlx::PgPool;

use application::ports::{EmbeddingClient, LlmClient, VectorStore};
use application::skills::Skill;
use mcp::McpClient;
use crate::activity::{ActivityBus, StatusInfo};
use actors::doc_actor::DocMsg;
use actors::learning_actor::LearningMsg;
use actors::node_actor::NodeMsg;
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
    pub node_actor: ActorRef<NodeMsg>,
    pub llm: Arc<dyn LlmClient>,
    pub embedding: Arc<dyn EmbeddingClient>,
    pub vector: Arc<dyn VectorStore>,
    pub attachments: Arc<dyn AttachmentStore>,
    /// Файловые скилы, загруженные при старте (агент инжектит их в промпт).
    pub skills: Arc<Vec<Skill>>,
    /// Подключённые MCP-серверы компании (источник внешних tool'ов для агента).
    pub mcp: Arc<McpClient>,
    /// Шина live-событий активности (LLM/embed/MCP/agent) для /api/events.
    pub activity: ActivityBus,
    /// Статические факты о сервере для /api/status.
    pub status_info: Arc<StatusInfo>,
    #[allow(dead_code)]
    pub db: PgPool,
}
