//! LLM-powered lesson splitter. Reads raw text and asks LLM to split it into
//! a sequence of self-contained lessons with titles and bodies.
//!
//! This is the "MCP-like" integration point: any raw material (llm.txt,
//! markdown book chapter, article) goes through the LLM, which returns a
//! structured array of lessons ready to be delivered daily.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use application::ports::{LessonDraft, LessonSplitter, LlmClient, LlmError};

const SPLITTER_SYSTEM: &str = "\
You are a learning-content editor. You split raw text into a sequence of SELF-CONTAINED lessons.

Rules:
- Each lesson is independently readable; it may reference earlier lessons but does not require them mid-reading.
- Each lesson is ~500-1500 words. Do NOT pad or shrink the content drastically — preserve the user's material.
- Title is short (3-7 words) and concrete.
- Body is clean markdown (headings, bullets, code blocks where appropriate).
- If input is already structured with `## Lesson N:` or `---` separators — respect those splits.
- If input is a long unstructured article — split by logical topics.
- Preserve ALL substantive information from the input. You may polish grammar and structure.
- Write in the same language as the input.

Respond with JSON ONLY, no prose:
{\"lessons\": [{\"title\": string, \"content\": string (markdown)}, ...]}

Aim for 3-15 lessons depending on input size.";

pub struct LlmLessonSplitter {
    llm: Arc<dyn LlmClient>,
}

impl LlmLessonSplitter {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl LessonSplitter for LlmLessonSplitter {
    async fn split(
        &self,
        track_title: &str,
        raw_text: &str,
    ) -> Result<Vec<LessonDraft>, LlmError> {
        let user_msg = format!(
            "Track title: {track_title}\n\n---\n\nRaw material to split:\n\n{raw_text}"
        );
        let text = self.llm.complete_text(SPLITTER_SYSTEM, &user_msg).await?;

        // Extract outermost JSON object
        let start = text
            .find('{')
            .ok_or_else(|| LlmError::BadResponse("no json object".into()))?;
        let end = text
            .rfind('}')
            .ok_or_else(|| LlmError::BadResponse("no closing brace".into()))?;
        if end < start {
            return Err(LlmError::BadResponse("malformed json".into()));
        }
        let json = &text[start..=end];

        #[derive(Deserialize)]
        struct Parsed {
            #[serde(default)]
            lessons: Vec<Draft>,
        }
        #[derive(Deserialize)]
        struct Draft {
            title: String,
            content: String,
        }

        let parsed: Parsed = serde_json::from_str(json)
            .map_err(|e| LlmError::BadResponse(format!("parse: {e}")))?;

        if parsed.lessons.is_empty() {
            return Err(LlmError::BadResponse("no lessons in response".into()));
        }

        Ok(parsed
            .lessons
            .into_iter()
            .map(|d| LessonDraft {
                title: d.title.trim().to_string(),
                content: d.content,
            })
            .collect())
    }
}
