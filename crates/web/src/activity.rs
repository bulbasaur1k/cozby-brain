//! Live-наблюдаемость сервера: broadcast-шина событий активности
//! (LLM-запросы, embedding, MCP-вызовы, шаги агента) + статические факты для
//! `/api/status`. TUI подписывается на поток и показывает «что происходит».

use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::broadcast;

use application::ports::{EmbeddingClient, LlmClient, LlmError};

/// Одно событие активности сервера.
#[derive(Clone, Debug, Serialize)]
pub struct ActivityEvent {
    /// RFC3339 timestamp.
    pub ts: String,
    /// "start" | "done" | "error" | "info".
    pub level: String,
    /// "llm" | "embed" | "mcp" | "agent" | "system".
    pub kind: String,
    pub detail: String,
}

/// Шина событий. Клонируется свободно; `emit` не блокирует и не падает без
/// подписчиков.
#[derive(Clone)]
pub struct ActivityBus {
    tx: broadcast::Sender<ActivityEvent>,
}

impl ActivityBus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(256);
        Self { tx }
    }

    pub fn emit(&self, level: &str, kind: &str, detail: impl Into<String>) {
        let ev = ActivityEvent {
            ts: chrono::Utc::now().to_rfc3339(),
            level: level.to_string(),
            kind: kind.to_string(),
            detail: detail.into(),
        };
        let _ = self.tx.send(ev);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ActivityEvent> {
        self.tx.subscribe()
    }
}

impl Default for ActivityBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Статические факты о сервере для снапшота `/api/status`.
#[derive(Clone, Serialize)]
pub struct StatusInfo {
    pub llm_name: String,
    pub llm_model: Option<String>,
    pub llm_configured: bool,
    pub embedding_configured: bool,
    pub vector_configured: bool,
    pub storage_configured: bool,
}

fn first_line(s: &str, n: usize) -> String {
    s.lines().next().unwrap_or("").chars().take(n).collect()
}

/// Декоратор LlmClient — эмитит start/done/error вокруг каждого запроса.
/// Покрывает ВСЕ вызовы (classify, match, agent, RAG) без правки call-site'ов.
pub struct InstrumentedLlm {
    inner: Arc<dyn LlmClient>,
    bus: ActivityBus,
}

impl InstrumentedLlm {
    pub fn new(inner: Arc<dyn LlmClient>, bus: ActivityBus) -> Self {
        Self { inner, bus }
    }
}

#[async_trait]
impl LlmClient for InstrumentedLlm {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn complete_text(&self, system: &str, user: &str) -> Result<String, LlmError> {
        if self.inner.name() == "noop" {
            return self.inner.complete_text(system, user).await;
        }
        self.bus
            .emit("start", "llm", format!("LLM запрос: {}", first_line(user, 60)));
        let res = self.inner.complete_text(system, user).await;
        match &res {
            Ok(t) => self
                .bus
                .emit("done", "llm", format!("LLM ответ ({} симв.)", t.chars().count())),
            Err(e) => self.bus.emit("error", "llm", format!("LLM ошибка: {e}")),
        }
        res
    }
}

/// Декоратор EmbeddingClient — эмитит события вокруг каждого embedding-запроса.
pub struct InstrumentedEmbedding {
    inner: Arc<dyn EmbeddingClient>,
    bus: ActivityBus,
}

impl InstrumentedEmbedding {
    pub fn new(inner: Arc<dyn EmbeddingClient>, bus: ActivityBus) -> Self {
        Self { inner, bus }
    }
}

#[async_trait]
impl EmbeddingClient for InstrumentedEmbedding {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, LlmError> {
        if self.inner.name() == "noop" {
            return self.inner.embed(text).await;
        }
        self.bus
            .emit("start", "embed", format!("embedding: {}", first_line(text, 40)));
        let res = self.inner.embed(text).await;
        match &res {
            Ok(_) => self.bus.emit("done", "embed", "embedding готов"),
            Err(LlmError::NotConfigured) => {}
            Err(e) => self.bus.emit("error", "embed", format!("embed ошибка: {e}")),
        }
        res
    }
}
