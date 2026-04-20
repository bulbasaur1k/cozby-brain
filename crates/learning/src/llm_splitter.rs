//! LLM-powered lesson splitter. Reads raw text and asks LLM to split it into
//! a sequence of self-contained lessons with titles and bodies.
//!
//! This is the "MCP-like" integration point: any raw material (llm.txt,
//! markdown book chapter, article) goes through the LLM, which returns a
//! structured array of lessons ready to be delivered daily.
//!
//! Large inputs (a full book, 500KB+) would blow past the model's context
//! window in a single call. We chunk by paragraphs with small overlap,
//! process chunks sequentially, and deduplicate lessons by title.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use application::ports::{LessonDraft, LessonSplitter, LlmClient, LlmError};

/// Inputs up to this size are sent as a single LLM call (current behavior).
/// Above — we chunk. Chosen to leave room for ~4K-token output on inputs
/// that expand (polish, formatting).
const MAX_SINGLE_CHARS: usize = 12_000;

/// Target size of each chunk when input exceeds `MAX_SINGLE_CHARS`.
/// Keeps prompt + output comfortably under 8K tokens for most providers.
const CHUNK_SIZE: usize = 8_000;

/// Tail of the previous chunk carried into the next, so the LLM keeps
/// narrative/topic continuity across chunk boundaries.
const CHUNK_OVERLAP: usize = 400;

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

    async fn split_chunk(
        &self,
        track_title: &str,
        chunk_text: &str,
        chunk_idx: usize,
        total_chunks: usize,
    ) -> Result<Vec<LessonDraft>, LlmError> {
        let header = if total_chunks > 1 {
            format!(
                "Track title: {track_title}\n\n\
                 NOTE: this is chunk {n}/{total} of a larger text. \
                 Produce lessons only for the material in THIS chunk. \
                 Do not invent content beyond it. Skip partial fragments at \
                 the very beginning/end if they are clearly the tail of a \
                 lesson from an adjacent chunk.\n\n\
                 ---\n\nRaw material to split:\n\n{body}",
                n = chunk_idx + 1,
                total = total_chunks,
                body = chunk_text
            )
        } else {
            format!("Track title: {track_title}\n\n---\n\nRaw material to split:\n\n{chunk_text}")
        };

        let text = self.llm.complete_text(SPLITTER_SYSTEM, &header).await?;
        parse_lessons(&text)
    }
}

#[async_trait]
impl LessonSplitter for LlmLessonSplitter {
    async fn split(
        &self,
        track_title: &str,
        raw_text: &str,
    ) -> Result<Vec<LessonDraft>, LlmError> {
        let chunks = chunk_text(raw_text, MAX_SINGLE_CHARS, CHUNK_SIZE, CHUNK_OVERLAP);
        let total = chunks.len();

        if total > 1 {
            tracing::info!(
                track = %track_title,
                chunks = total,
                total_chars = raw_text.chars().count(),
                "splitting large material into chunks"
            );
        }

        let mut lessons = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut last_err: Option<LlmError> = None;

        for (idx, chunk) in chunks.iter().enumerate() {
            match self.split_chunk(track_title, chunk, idx, total).await {
                Ok(drafts) => {
                    for d in drafts {
                        let key = d.title.trim().to_lowercase();
                        if !key.is_empty() && seen.insert(key) {
                            lessons.push(d);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        chunk = idx + 1,
                        total = total,
                        error = %e,
                        "chunk split failed, continuing with remaining chunks"
                    );
                    last_err = Some(e);
                }
            }
        }

        if lessons.is_empty() {
            return Err(last_err
                .unwrap_or_else(|| LlmError::BadResponse("no lessons in response".into())));
        }
        Ok(lessons)
    }
}

fn parse_lessons(text: &str) -> Result<Vec<LessonDraft>, LlmError> {
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

/// Splits `text` into chunks suitable for sequential LLM processing.
///
/// - If total length ≤ `single_threshold`, returns one chunk (original text).
/// - Otherwise greedily accumulates paragraphs (split by `\n\n`) up to
///   `target_size`, then starts a new chunk seeded with the last
///   `overlap` chars of the previous one.
/// - A single paragraph larger than `target_size` is hard-broken on char
///   boundaries with the same overlap.
///
/// Sizes are measured in Unicode scalar values (chars), not bytes.
fn chunk_text(
    text: &str,
    single_threshold: usize,
    target_size: usize,
    overlap: usize,
) -> Vec<String> {
    let total = text.chars().count();
    if total <= single_threshold {
        return vec![text.to_string()];
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_chars: usize = 0;

    let flush = |chunks: &mut Vec<String>, current: &mut String, current_chars: &mut usize| {
        if !current.is_empty() {
            chunks.push(std::mem::take(current));
            *current_chars = 0;
        }
    };

    let start_with_overlap =
        |prev: &str, current: &mut String, current_chars: &mut usize, overlap: usize| {
            if overlap == 0 || prev.is_empty() {
                return;
            }
            let prev_chars = prev.chars().count();
            let skip = prev_chars.saturating_sub(overlap);
            let tail: String = prev.chars().skip(skip).collect();
            *current_chars = tail.chars().count();
            *current = tail;
        };

    for para in text.split("\n\n") {
        let para_chars = para.chars().count();

        // Oversized paragraph: hard-break on char boundaries.
        if para_chars > target_size {
            flush(&mut chunks, &mut current, &mut current_chars);
            let chars: Vec<char> = para.chars().collect();
            let mut start = 0;
            while start < chars.len() {
                let end = (start + target_size).min(chars.len());
                let piece: String = chars[start..end].iter().collect();
                chunks.push(piece);
                if end == chars.len() {
                    break;
                }
                start = end.saturating_sub(overlap);
            }
            continue;
        }

        let sep_cost = if current.is_empty() { 0 } else { 2 };
        if current_chars + sep_cost + para_chars > target_size && !current.is_empty() {
            let prev = current.clone();
            flush(&mut chunks, &mut current, &mut current_chars);
            start_with_overlap(&prev, &mut current, &mut current_chars, overlap);
        }

        if !current.is_empty() {
            current.push_str("\n\n");
            current_chars += 2;
        }
        current.push_str(para);
        current_chars += para_chars;
    }

    flush(&mut chunks, &mut current, &mut current_chars);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_input_single_chunk() {
        let text = "short material\n\nwith two paragraphs";
        let chunks = chunk_text(text, 12_000, 8_000, 400);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], text);
    }

    #[test]
    fn large_input_splits_on_paragraph() {
        let para = "a".repeat(3_000);
        let text = format!("{para}\n\n{para}\n\n{para}\n\n{para}\n\n{para}");
        let chunks = chunk_text(&text, 12_000, 8_000, 400);
        assert!(chunks.len() >= 2, "expected multiple chunks, got {}", chunks.len());
        for (i, c) in chunks.iter().enumerate() {
            assert!(
                c.chars().count() <= 8_000 + 400,
                "chunk {i} too big: {}",
                c.chars().count()
            );
        }
    }

    #[test]
    fn oversized_paragraph_hard_break() {
        let monster = "x".repeat(20_000);
        let chunks = chunk_text(&monster, 12_000, 8_000, 400);
        assert!(chunks.len() >= 3);
        for c in &chunks {
            assert!(c.chars().count() <= 8_000);
        }
    }

    #[test]
    fn overlap_carried_across_chunks() {
        let mut text = String::new();
        for i in 0..10 {
            text.push_str(&format!("paragraph-{i}-{}\n\n", "z".repeat(1_500)));
        }
        let chunks = chunk_text(&text, 12_000, 8_000, 400);
        assert!(chunks.len() >= 2);
        // Every chunk after the first should start with the overlap tail
        // of the previous chunk.
        for i in 1..chunks.len() {
            let prev = &chunks[i - 1];
            let prev_tail: String = prev
                .chars()
                .skip(prev.chars().count().saturating_sub(400))
                .collect();
            assert!(
                chunks[i].starts_with(&prev_tail[..prev_tail.len().min(50)]),
                "chunk {i} does not begin with overlap from chunk {}",
                i - 1
            );
        }
    }

    #[test]
    fn unicode_counted_as_chars_not_bytes() {
        // 4000 cyrillic chars ~ 8000 bytes; with threshold 12000 chars,
        // input should stay single-chunk.
        let text = "я".repeat(4_000);
        let chunks = chunk_text(&text, 12_000, 8_000, 400);
        assert_eq!(chunks.len(), 1);
    }
}
