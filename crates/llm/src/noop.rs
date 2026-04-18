use async_trait::async_trait;

use application::ports::{EmbeddingClient, LlmClient, LlmError};

/// Default client used when no LLM is configured. Every call returns
/// `NotConfigured`, and use-cases fall back to naive heuristics.
pub struct NoopLlmClient;

#[async_trait]
impl LlmClient for NoopLlmClient {
    fn name(&self) -> &str {
        "noop"
    }

    async fn complete_text(&self, _system: &str, _user: &str) -> Result<String, LlmError> {
        Err(LlmError::NotConfigured)
    }
}

#[async_trait]
impl EmbeddingClient for NoopLlmClient {
    fn name(&self) -> &str {
        "noop"
    }

    async fn embed(&self, _text: &str) -> Result<Vec<f32>, LlmError> {
        Err(LlmError::NotConfigured)
    }
}
