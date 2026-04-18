use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub http_addr: String,
    /// Base URL of an OpenAI-compatible `chat/completions` endpoint.
    /// Examples:
    ///   `https://api.z.ai/api/paas/v4`          (GLM, free tier)
    ///   `http://localhost:11434/v1`             (Ollama, local)
    ///   `https://openrouter.ai/api/v1`          (OpenRouter)
    #[serde(default)]
    pub llm_base_url: Option<String>,
    /// Bearer token for the LLM endpoint. Leave empty for Ollama.
    #[serde(default)]
    pub llm_api_key: Option<String>,
    /// Model name. Examples: `glm-4.5-flash`, `llama3.2`, `qwen2.5`.
    #[serde(default)]
    pub llm_model: Option<String>,
    /// Embedding model name (uses same base_url/api_key as LLM).
    #[serde(default)]
    pub embedding_model: Option<String>,
    /// Qdrant gRPC URL. Default: http://localhost:6334
    #[serde(default)]
    pub qdrant_url: Option<String>,
    /// Qdrant collection name. Default: cozby_notes
    #[serde(default)]
    pub qdrant_collection: Option<String>,
    /// MinIO/S3 endpoint URL (e.g. `http://localhost:9000`).
    #[serde(default)]
    pub s3_endpoint: Option<String>,
    #[serde(default)]
    pub s3_region: Option<String>,
    #[serde(default)]
    pub s3_access_key: Option<String>,
    #[serde(default)]
    pub s3_secret_key: Option<String>,
    #[serde(default)]
    pub s3_bucket: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, config::ConfigError> {
        dotenvy::dotenv().ok();
        config::Config::builder()
            .set_default("http_addr", "0.0.0.0:8080")?
            .add_source(config::Environment::default())
            .build()?
            .try_deserialize()
    }

    /// Returns LLM configuration tuple if the user enabled it (base_url + model set).
    pub fn llm(&self) -> Option<(String, Option<String>, String, Option<String>)> {
        match (&self.llm_base_url, &self.llm_model) {
            (Some(base), Some(model)) if !base.is_empty() && !model.is_empty() => Some((
                base.clone(),
                self.llm_api_key.clone().filter(|k| !k.is_empty()),
                model.clone(),
                self.embedding_model.clone().filter(|m| !m.is_empty()),
            )),
            _ => None,
        }
    }

    pub fn qdrant_url(&self) -> Option<String> {
        self.qdrant_url
            .clone()
            .filter(|u| !u.is_empty())
    }

    pub fn qdrant_collection(&self) -> String {
        self.qdrant_collection
            .clone()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "cozby_notes".to_string())
    }

    /// Returns S3/MinIO config tuple if fully configured.
    pub fn s3(&self) -> Option<(String, String, String, String, String)> {
        match (
            &self.s3_endpoint,
            &self.s3_access_key,
            &self.s3_secret_key,
            &self.s3_bucket,
        ) {
            (Some(endpoint), Some(access), Some(secret), Some(bucket))
                if !endpoint.is_empty()
                    && !access.is_empty()
                    && !secret.is_empty()
                    && !bucket.is_empty() =>
            {
                let region = self
                    .s3_region
                    .clone()
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| "us-east-1".to_string());
                Some((
                    endpoint.clone(),
                    region,
                    access.clone(),
                    secret.clone(),
                    bucket.clone(),
                ))
            }
            _ => None,
        }
    }
}
