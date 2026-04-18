//! Universal OpenAI-compatible `chat/completions` client.
//!
//! Works with any provider that speaks the OpenAI wire format, including
//! reasoning models that return `content: null` + `reasoning` field.
//!
//! | Provider    | base_url                                 | model                 | cost       |
//! |-------------|------------------------------------------|-----------------------|------------|
//! | routerai.ru | https://routerai.ru/api/v1               | z-ai/glm-4.7-flash   | cheap      |
//! | Ollama      | http://localhost:11434/v1                | llama3.2, qwen2.5, …  | local/free |
//! | OpenRouter  | https://openrouter.ai/api/v1             | *free models*         | free tier  |
//! | Groq        | https://api.groq.com/openai/v1           | llama-3.1-8b-instant  | free tier  |
//!
//! `api_key` is optional — Ollama does not require it.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use application::ports::{EmbeddingClient, LlmClient, LlmError};

pub struct OpenAICompatClient {
    base_url: String,
    api_key: Option<String>,
    model: String,
    embedding_model: Option<String>,
    http: reqwest::Client,
}

impl OpenAICompatClient {
    pub fn new(base_url: String, api_key: Option<String>, model: String) -> Self {
        Self::with_embedding(base_url, api_key, model, None)
    }

    pub fn with_embedding(
        base_url: String,
        api_key: Option<String>,
        model: String,
        embedding_model: Option<String>,
    ) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest client");
        OpenAICompatClient {
            base_url,
            api_key,
            model,
            embedding_model,
            http,
        }
    }
}

// ---------------- Embedding ----------------

#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingClient for OpenAICompatClient {
    fn name(&self) -> &str {
        "openai-compat-embed"
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, LlmError> {
        let emb_model = self
            .embedding_model
            .as_deref()
            .ok_or(LlmError::NotConfigured)?;
        let url = format!("{}/embeddings", self.base_url);
        let body = EmbeddingRequest {
            model: emb_model,
            input: text,
        };
        let mut req = self.http.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        let status = resp.status();
        let raw = resp
            .text()
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(LlmError::Api(format!("{status}: {raw}")));
        }
        let parsed: EmbeddingResponse = serde_json::from_str(&raw)
            .map_err(|e| LlmError::BadResponse(format!("embedding parse: {e}")))?;
        parsed
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| LlmError::BadResponse("no embedding data".into()))
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Choice {
    message: RespMessage,
}

#[derive(Deserialize)]
struct RespMessage {
    /// Normal models put the answer here.
    #[serde(default)]
    content: Option<String>,
    /// Reasoning models (GLM-4.7, o1, etc.) may set content=null and put
    /// chain-of-thought here. We extract JSON from this as fallback.
    #[serde(default)]
    reasoning: Option<String>,
}

#[async_trait]
impl LlmClient for OpenAICompatClient {
    fn name(&self) -> &str {
        "openai-compat"
    }

    async fn complete_text(&self, system: &str, user: &str) -> Result<String, LlmError> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = ChatRequest {
            model: &self.model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: system,
                },
                ChatMessage {
                    role: "user",
                    content: user,
                },
            ],
            temperature: 0.2,
            max_tokens: Some(4096),
        };

        let mut req = self.http.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        let status = resp.status();
        let raw = resp
            .text()
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?;

        if !status.is_success() {
            return Err(LlmError::Api(format!("{status}: {raw}")));
        }

        let parsed: ChatResponse = serde_json::from_str(&raw)
            .map_err(|e| LlmError::BadResponse(format!("parse: {e}; body: {raw}")))?;

        if let Some(err) = parsed.error {
            return Err(LlmError::Api(err.to_string()));
        }

        let msg = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| LlmError::BadResponse("no choices".into()))?;

        // Priority: content (non-empty) > reasoning > error
        let content = msg
            .content
            .filter(|s| !s.trim().is_empty())
            .or(msg.reasoning)
            .ok_or_else(|| LlmError::BadResponse("both content and reasoning are empty".into()))?;

        tracing::debug!(len = content.len(), "llm response received");
        Ok(content)
    }
}
