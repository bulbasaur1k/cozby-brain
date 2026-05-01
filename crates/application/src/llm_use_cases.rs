//! Use-cases that turn raw user text into structured domain data via an LLM.
//!
//! All functions are infallible — if the LLM is unavailable or returns malformed
//! output, we log and fall back to a naive heuristic. This keeps the product
//! usable without an API key and resilient to upstream outages.

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::ports::{LlmClient, LlmError};

#[derive(Debug, Clone)]
pub struct StructuredNote {
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StructuredTodo {
    pub title: String,
    pub due_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct StructuredReminder {
    pub text: String,
    pub remind_at: DateTime<Utc>,
    /// RRULE-lite (см. domain::recurrence), None = одноразовое.
    pub recurrence: Option<String>,
}

// ------------------------- universal classifier -------------------------

/// Result of classifying user input — one of the supported entity types.
#[derive(Debug, Clone)]
pub enum Classified {
    /// A note — markdown content with title and tags.
    Note(StructuredNote),
    /// A todo — imperative action, optional due date.
    Todo(StructuredTodo),
    /// A reminder — text + firing time.
    Reminder(StructuredReminder),
    /// A question / search query — returns keywords.
    Question(StructuredQuestion),
    /// A documentation page — belongs to a project, markdown content.
    Doc(StructuredDoc),
}

#[derive(Debug, Clone)]
pub struct StructuredQuestion {
    pub keywords: Vec<String>,
    /// Scope hint: "notes" | "todos" | "reminders" | "docs" | "all"
    pub scope: String,
    /// Original query for logging.
    pub query: String,
    /// State filter:
    ///   todos:     "done" | "undone" | "overdue"
    ///   reminders: "fired" | "pending" | "overdue"
    ///   notes/docs: None
    pub status: Option<String>,
    /// Relative time window for filtering by created_at / due_at / remind_at:
    ///   "today" | "yesterday" | "week" | "month" | "all"
    pub time_window: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocOperation {
    /// Create a new page. If page already exists — will fail (or use Replace).
    Create,
    /// Append new content at the end of existing page. Adds separator.
    Append,
    /// Replace entire page content (keeps history).
    Replace,
    /// Insert as a new section (`## Section`) — smart append that doesn't duplicate.
    Section,
}

impl DocOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            DocOperation::Create => "create",
            DocOperation::Append => "append",
            DocOperation::Replace => "replace",
            DocOperation::Section => "section",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "create" => Some(Self::Create),
            "append" => Some(Self::Append),
            "replace" => Some(Self::Replace),
            "section" => Some(Self::Section),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StructuredDoc {
    /// Project slug or title (user-readable). Actor resolves to actual project.
    pub project: String,
    /// Page slug or title (user-readable). Actor resolves/creates.
    pub page: String,
    /// Markdown content to add.
    pub content: String,
    /// Tags for this doc update.
    pub tags: Vec<String>,
    /// How to apply the content.
    pub operation: DocOperation,
    /// Optional — if operation=section, this is the section heading.
    pub section_title: Option<String>,
}

const CLASSIFY_SYSTEM: &str = "\
You are a personal AI agent. Classify user input and produce structured data.
The user may pack MULTIPLE items in one message, possibly of DIFFERENT TYPES and for DIFFERENT
PROJECTS (e.g. \"добавь в docs проекта X про Y, и создай задачу Z\") — in that case return
multiple items in the `items` array.

Types:
- note: quick personal knowledge / fact / idea / thought (no project context, no structure needed)
- doc: FORMAL documentation page that belongs to a PROJECT (has project name + page name)
- todo: imperative action (\"buy milk\", \"call mom\", \"надо купить\", \"сделать\")
- reminder: has explicit time phrase (\"через 30 минут\", \"завтра в 10\", \"in 2 hours\")
- question: search / query (\"what did I write about\", \"найди про\", \"покажи заметки\")

When to pick DOC vs NOTE:
- doc: user mentions a PROJECT name (\"в проекте X\", \"docs cozby-brain\", \"в документацию X\")
       OR wants structured documentation (\"создай страницу про Y\", \"документируй Z\")
- note: personal random thought without project context (\"сегодня узнал про ractor\")

STRICT note templates:
- TECH template (programming, technologies, tools, frameworks):
    # {title}
    ## Суть
    {one-line summary}
    ## Детали
    - {bullet points}
    ## Примеры
    {examples if mentioned or empty}
    ## Связанное
    {references or [[wiki-links]] if mentioned or empty}
- PERSONAL template (thoughts, ideas, non-technical):
    # {title}

    {clean paragraphs}

Pick TECH if topic is technical. Otherwise PERSONAL. Preserve ALL user info, improve grammar/structure.

ENRICHMENT RULES for note and doc content:
You are not a passive recorder — you are a writing assistant. When the user
gives bare, list-like, or messy input, organize it into clean markdown:
- Detect implicit structure: days, weeks, steps, levels, categories — turn
  them into `## headings` or `### sub-headings`. E.g. \"День 1\\nПрисед\\nЖим\"
  → `## День 1\\n- Присед\\n- Жим`.
- Group related items into bullet lists. Don't put each item on a separate
  paragraph if they belong together.
- Use tables when the data is naturally tabular (sets × reps, schedule,
  comparisons, key/value pairs).
- Use ```fenced``` code blocks for code, commands, configs.
- Use blockquotes (`>`) for cited quotes only.
- Add a short `## Notes` or `## TL;DR` section at the top ONLY if the input
  is long enough to benefit from one.
NEVER invent facts the user did not provide. NEVER pad — quality over volume.
The output must contain the same information as the input plus structure.

For todo: imperative action phrase (не вопрос, не пожелание). Short and clear.
For reminder: short text + `remind_at` RFC3339 UTC. If no time given, default: now + 1 hour.
For recurring reminders, also fill `recurrence` (RRULE-lite, see below). If the
phrase has no recurrence cue (\"каждый\", \"every\", \"по понедельникам\",
\"ежедневно\", \"раз в неделю\", \"15-го числа\" etc.), leave `recurrence` as
null. `remind_at` for a recurring reminder = the FIRST occurrence (today/now
or the first matching weekday/day-of-month at the requested time).
Recurrence format (string, all caps, `;`-separated):
  - FREQ=DAILY                             — каждый день
  - FREQ=DAILY;INTERVAL=2                  — через день
  - FREQ=WEEKLY;BYDAY=MO,WE,FR             — пн/ср/пт
  - FREQ=MONTHLY;BYMONTHDAY=15             — 15-го числа
For question: extract keywords + scope + OPTIONAL status/time_window filters.
  * keywords: 0-5 topic words (empty if pure listing like \"покажи все задачи\").
  * scope: 'notes' | 'todos' | 'reminders' | 'docs' | 'all'.
  * status (optional): for listing intents like \"невыполненные\", \"просроченные\", \"сделанные\":
      - todos:     \"done\" | \"undone\" | \"overdue\"
      - reminders: \"fired\" | \"pending\" | \"overdue\"
      - skip for notes/docs
  * time_window (optional): \"today\" | \"yesterday\" | \"week\" | \"month\" | \"all\"
For doc: extract project name, page name, content (clean markdown), operation.

DOC operations:
- append: add new info to an existing page (end of page) — DEFAULT when adding to existing
- section: add as a new section with `## heading` — when user explicitly wants a new subsection
- replace: rewrite entire page — ONLY when user explicitly says \"перепиши\" / \"замени\"
- create: brand-new page — only when user clearly wants new page (\"создай страницу\", \"новая страничка\")

Respond with ONE JSON object, no prose, matching:
{\"items\": [{\"type\": \"note\"|\"todo\"|\"reminder\"|\"question\"|\"doc\", \"data\": {...}}, ...]}

Where data for each type:
- note:     {\"title\": string, \"content\": string (markdown, strict template), \"tags\": string[] (1-5 lowercase)}
- todo:     {\"title\": string, \"due_at\": string|null (RFC3339 UTC)}
- reminder: {\"text\": string, \"remind_at\": string (RFC3339 UTC), \"recurrence\": string|null}
- question: {\"keywords\": string[], \"scope\": \"notes\"|\"todos\"|\"reminders\"|\"docs\"|\"all\", \"status\": string|null, \"time_window\": string|null}
- doc:      {\"project\": string, \"page\": string, \"content\": string (markdown), \"tags\": string[], \"operation\": \"append\"|\"section\"|\"replace\"|\"create\", \"section_title\": string|null}

Rules for JSON:
- ALL string values must be valid JSON — escape newlines as \\n, quotes as \\\", backslashes as \\\\.
- Do NOT wrap the JSON in markdown code fences.
- Return ONE object with `items` array (1 or more items).
- For doc: `project` and `page` should be short, human-readable. Server will normalize to slugs.

CRITICAL OUTPUT RULES:
- Do NOT write any reasoning, thinking, explanations, or preamble.
- Do NOT write \"Thinking Process:\", \"Let me analyze\", \"<think>\", \"Final Answer:\", etc.
- Your ENTIRE response must start with `{` and end with `}`.
- If you must think internally, do it silently and output ONLY the JSON.

Write in the same language as the input.";

/// Classifies the user input AND structures it into one or more items.
/// If user packed multiple items in one message — each becomes a separate entry.
pub async fn classify_and_structure(
    llm: &dyn LlmClient,
    raw: &str,
    now: DateTime<Utc>,
) -> Result<Vec<Classified>, LlmError> {
    classify_and_structure_with_context(llm, raw, now, &[]).await
}

/// Same as `classify_and_structure` but also feeds existing similar items
/// to the LLM so it can avoid duplicates:
///   - notes: suggest append to existing one with same topic
///   - docs:  prefer append/section to an existing page
///   - todos: skip / merge near-duplicate action
///
/// `context_items`: compact list of existing items as `(kind, title, tags, short_preview)`.
pub async fn classify_and_structure_with_context(
    llm: &dyn LlmClient,
    raw: &str,
    now: DateTime<Utc>,
    context_items: &[(String, String, String, String)],
) -> Result<Vec<Classified>, LlmError> {
    let context_block = if context_items.is_empty() {
        String::new()
    } else {
        let list = context_items
            .iter()
            .map(|(kind, title, tags, preview)| {
                let prev = if preview.chars().count() > 120 {
                    let t: String = preview.chars().take(120).collect();
                    format!("{t}…")
                } else {
                    preview.clone()
                };
                let tags_part = if tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{tags}]")
                };
                format!("  - [{kind}]{tags_part} \"{title}\" — {prev}")
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "\n\nExisting items in user's knowledge base (consider these to avoid duplicates):\n\
             {list}\n\
             Rules when a matching item exists:\n\
             - note: если новая информация пересекается — use type=note анно тот же title, но не создавай дубль. Пользователь потом решит мержить\n\
             - doc:  если тема уже есть в проекте — operation=append или section, НЕ create\n\
             - todo: если действие уже в списке — используй тот же title, пользователь увидит дубль и решит\n\
             - всё равно возвращай полноценный items[], просто с более точными title/operation"
        )
    };
    let system = format!(
        "{CLASSIFY_SYSTEM}\n\nCurrent UTC time: {}{}",
        now.to_rfc3339(),
        context_block
    );
    let text = llm.complete_text(&system, raw).await?;
    let json = extract_json(&text)
        .ok_or_else(|| LlmError::BadResponse("no json object".into()))?;

    let v: Value = serde_json::from_str(json).map_err(|e| {
        LlmError::BadResponse(format!("{e}; raw response: {}", truncate_for_log(&text)))
    })?;

    // Accept two shapes for backward-compat:
    //   new: {"items": [{"type": ..., "data": ...}, ...]}
    //   old: {"type": ..., "data": ...}  (single item)
    let items_values: Vec<&Value> = if let Some(arr) = v.get("items").and_then(|x| x.as_array()) {
        arr.iter().collect()
    } else {
        vec![&v]
    };

    if items_values.is_empty() {
        return Err(LlmError::BadResponse("empty items".into()));
    }

    let mut out = Vec::with_capacity(items_values.len());
    for item in items_values {
        let kind = item
            .get("type")
            .and_then(|x| x.as_str())
            .ok_or_else(|| LlmError::BadResponse("item: missing type".into()))?;
        let data = item
            .get("data")
            .ok_or_else(|| LlmError::BadResponse("item: missing data".into()))?;
        let classified = match kind {
            "note" => Classified::Note(parse_note_data(data)?),
            "todo" => Classified::Todo(parse_todo_data(data)?),
            "reminder" => Classified::Reminder(parse_reminder_data(data, now)?),
            "question" => Classified::Question(parse_question_data(data, raw)?),
            "doc" => Classified::Doc(parse_doc_data(data)?),
            other => return Err(LlmError::BadResponse(format!("unknown type: {other}"))),
        };
        out.push(classified);
    }
    Ok(out)
}

fn truncate_for_log(s: &str) -> String {
    if s.chars().count() > 200 {
        let t: String = s.chars().take(200).collect();
        format!("{t}…")
    } else {
        s.to_string()
    }
}

fn parse_note_data(v: &Value) -> Result<StructuredNote, LlmError> {
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .ok_or_else(|| LlmError::BadResponse("note: missing title".into()))?
        .trim()
        .to_string();
    let content = v
        .get("content")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let tags = v
        .get("tags")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    Ok(StructuredNote {
        title,
        content,
        tags,
    })
}

fn parse_todo_data(v: &Value) -> Result<StructuredTodo, LlmError> {
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .ok_or_else(|| LlmError::BadResponse("todo: missing title".into()))?
        .trim()
        .to_string();
    let due_at = match v.get("due_at") {
        Some(Value::String(s)) => Some(
            DateTime::parse_from_rfc3339(s)
                .map_err(|e| LlmError::BadResponse(format!("due_at: {e}")))?
                .with_timezone(&Utc),
        ),
        _ => None,
    };
    Ok(StructuredTodo { title, due_at })
}

fn parse_reminder_data(v: &Value, now: DateTime<Utc>) -> Result<StructuredReminder, LlmError> {
    let text = v
        .get("text")
        .and_then(|x| x.as_str())
        .ok_or_else(|| LlmError::BadResponse("reminder: missing text".into()))?
        .trim()
        .to_string();
    let remind_at = match v.get("remind_at").and_then(|x| x.as_str()) {
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map_err(|e| LlmError::BadResponse(format!("remind_at: {e}")))?
            .with_timezone(&Utc),
        None => now + Duration::hours(1),
    };
    let recurrence = v
        .get("recurrence")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(StructuredReminder {
        text,
        remind_at,
        recurrence,
    })
}

fn parse_doc_data(v: &Value) -> Result<StructuredDoc, LlmError> {
    let project = v
        .get("project")
        .and_then(|x| x.as_str())
        .ok_or_else(|| LlmError::BadResponse("doc: missing project".into()))?
        .trim()
        .to_string();
    let page = v
        .get("page")
        .and_then(|x| x.as_str())
        .ok_or_else(|| LlmError::BadResponse("doc: missing page".into()))?
        .trim()
        .to_string();
    let content = v
        .get("content")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let tags = v
        .get("tags")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let op_str = v
        .get("operation")
        .and_then(|x| x.as_str())
        .unwrap_or("append");
    let operation = DocOperation::parse(op_str).unwrap_or(DocOperation::Append);
    let section_title = v
        .get("section_title")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(StructuredDoc {
        project,
        page,
        content,
        tags,
        operation,
        section_title,
    })
}

fn parse_question_data(v: &Value, raw: &str) -> Result<StructuredQuestion, LlmError> {
    let keywords = v
        .get("keywords")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let scope = v
        .get("scope")
        .and_then(|x| x.as_str())
        .unwrap_or("all")
        .to_string();
    let status = v
        .get("status")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let time_window = v
        .get("time_window")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    Ok(StructuredQuestion {
        keywords,
        scope,
        query: raw.to_string(),
        status,
        time_window,
    })
}

// ------------------------- legacy note structuring (for direct CRUD API) -------------------------

const STRUCTURE_NOTE_SYSTEM: &str = "You convert raw user thoughts into a well-structured markdown note. \
Respond with a single JSON object, no prose, matching:\n\
{\"title\": string, \"content\": string, \"tags\": string[]}\n\
Rules:\n\
- title: short and concrete, no trailing punctuation.\n\
- content: clean markdown, fix grammar, structure with headings/lists where it helps, preserve all user information and intent.\n\
- tags: 1-5 short lowercase topic tags, no spaces.\n\
- Write in the same language as the input.";

pub async fn structure_note(llm: &dyn LlmClient, raw: &str) -> StructuredNote {
    match try_structure_note(llm, raw).await {
        Ok(s) => s,
        Err(e) => {
            if !matches!(e, LlmError::NotConfigured) {
                tracing::warn!(error = %e, "llm structure_note failed, falling back");
            }
            fallback_note(raw)
        }
    }
}

async fn try_structure_note(llm: &dyn LlmClient, raw: &str) -> Result<StructuredNote, LlmError> {
    let text = llm.complete_text(STRUCTURE_NOTE_SYSTEM, raw).await?;
    let json = extract_json(&text)
        .ok_or_else(|| LlmError::BadResponse("no json object".into()))?;

    #[derive(Deserialize)]
    struct Parsed {
        title: String,
        content: String,
        #[serde(default)]
        tags: Vec<String>,
    }
    let p: Parsed =
        serde_json::from_str(json).map_err(|e| LlmError::BadResponse(e.to_string()))?;
    Ok(StructuredNote {
        title: p.title.trim().to_string(),
        content: p.content,
        tags: p.tags,
    })
}

fn fallback_note(raw: &str) -> StructuredNote {
    let title = raw
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "untitled".to_string());
    let title = title.chars().take(80).collect::<String>();
    StructuredNote {
        title,
        content: raw.to_string(),
        tags: vec![],
    }
}

// ------------------------- todo parsing -------------------------

pub async fn parse_todo(llm: &dyn LlmClient, raw: &str, now: DateTime<Utc>) -> StructuredTodo {
    match try_parse_todo(llm, raw, now).await {
        Ok(t) => t,
        Err(e) => {
            if !matches!(e, LlmError::NotConfigured) {
                tracing::warn!(error = %e, "llm parse_todo failed, falling back");
            }
            StructuredTodo {
                title: raw.trim().to_string(),
                due_at: None,
            }
        }
    }
}

async fn try_parse_todo(
    llm: &dyn LlmClient,
    raw: &str,
    now: DateTime<Utc>,
) -> Result<StructuredTodo, LlmError> {
    let system = format!(
        "You parse user text into a todo item. Current UTC time: {now}.\n\
Respond with a single JSON object, no prose:\n\
{{\"title\": string, \"due_at\": string|null}}\n\
- title: clear imperative action (\"buy milk\", \"call mom\"), same language as input.\n\
- due_at: RFC3339 in UTC if a time/date is mentioned, else null. Do not invent times.\n\
- Parse relative phrases (через час, завтра в 10, tomorrow 5pm) correctly.",
        now = now.to_rfc3339()
    );
    let text = llm.complete_text(&system, raw).await?;
    let json = extract_json(&text)
        .ok_or_else(|| LlmError::BadResponse("no json object".into()))?;
    let v: Value =
        serde_json::from_str(json).map_err(|e| LlmError::BadResponse(e.to_string()))?;
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .ok_or_else(|| LlmError::BadResponse("missing title".into()))?
        .trim()
        .to_string();
    let due_at = match v.get("due_at") {
        Some(Value::String(s)) => Some(
            DateTime::parse_from_rfc3339(s)
                .map_err(|e| LlmError::BadResponse(format!("due_at: {e}")))?
                .with_timezone(&Utc),
        ),
        _ => None,
    };
    Ok(StructuredTodo { title, due_at })
}

// ------------------------- reminder parsing -------------------------

pub async fn parse_reminder(
    llm: &dyn LlmClient,
    raw: &str,
    now: DateTime<Utc>,
) -> StructuredReminder {
    match try_parse_reminder(llm, raw, now).await {
        Ok(r) => r,
        Err(e) => {
            if !matches!(e, LlmError::NotConfigured) {
                tracing::warn!(error = %e, "llm parse_reminder failed, falling back");
            }
            StructuredReminder {
                text: raw.trim().to_string(),
                remind_at: now + Duration::hours(1),
                recurrence: None,
            }
        }
    }
}

async fn try_parse_reminder(
    llm: &dyn LlmClient,
    raw: &str,
    now: DateTime<Utc>,
) -> Result<StructuredReminder, LlmError> {
    let system = format!(
        "You parse user text into a reminder. Current UTC time: {now}.\n\
Respond with a single JSON object, no prose:\n\
{{\"text\": string, \"remind_at\": string}}\n\
- text: what to remind about, same language as input.\n\
- remind_at: RFC3339 in UTC. If no time is given, set it to one hour from now.\n\
- Parse relative phrases correctly (через 10 минут, через час, завтра в 9, in 30 minutes).",
        now = now.to_rfc3339()
    );
    let text = llm.complete_text(&system, raw).await?;
    let json = extract_json(&text)
        .ok_or_else(|| LlmError::BadResponse("no json object".into()))?;
    let v: Value =
        serde_json::from_str(json).map_err(|e| LlmError::BadResponse(e.to_string()))?;
    let text_field = v
        .get("text")
        .and_then(|x| x.as_str())
        .ok_or_else(|| LlmError::BadResponse("missing text".into()))?
        .trim()
        .to_string();
    let remind_at_s = v
        .get("remind_at")
        .and_then(|x| x.as_str())
        .ok_or_else(|| LlmError::BadResponse("missing remind_at".into()))?;
    let remind_at = DateTime::parse_from_rfc3339(remind_at_s)
        .map_err(|e| LlmError::BadResponse(format!("remind_at: {e}")))?
        .with_timezone(&Utc);
    Ok(StructuredReminder {
        text: text_field,
        remind_at,
        recurrence: None,
    })
}

// ------------------------- search keyword extraction -------------------------

pub async fn extract_search_keywords(llm: &dyn LlmClient, query: &str) -> Vec<String> {
    match try_extract_keywords(llm, query).await {
        Ok(kw) if !kw.is_empty() => kw,
        Ok(_) => fallback_keywords(query),
        Err(e) => {
            if !matches!(e, LlmError::NotConfigured) {
                tracing::warn!(error = %e, "llm extract_keywords failed, falling back");
            }
            fallback_keywords(query)
        }
    }
}

async fn try_extract_keywords(llm: &dyn LlmClient, query: &str) -> Result<Vec<String>, LlmError> {
    const SYSTEM: &str = "Extract 1-5 short search keywords from a user search query. \
Respond with JSON only: {\"keywords\": string[]}. Keywords in the same language as the query.";
    let text = llm.complete_text(SYSTEM, query).await?;
    let json = extract_json(&text)
        .ok_or_else(|| LlmError::BadResponse("no json".into()))?;
    #[derive(Deserialize)]
    struct Parsed {
        #[serde(default)]
        keywords: Vec<String>,
    }
    let p: Parsed =
        serde_json::from_str(json).map_err(|e| LlmError::BadResponse(e.to_string()))?;
    Ok(p.keywords
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

fn fallback_keywords(query: &str) -> Vec<String> {
    let mut out: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .map(|s| s.to_lowercase())
        .filter(|s| s.chars().count() >= 3)
        .collect();
    out.sort();
    out.dedup();
    out.into_iter().take(5).collect()
}

// ------------------------- find best match for append -------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct AppendSuggestion {
    pub target_id: String,
    pub target_title: String,
    pub score: f32,
    pub reason: String,
}

use crate::ports::SimilarNote;

/// Given a structured note and similar candidates from Qdrant, asks LLM
/// whether we should append to one of them. Returns `None` → create new.
pub async fn find_best_match(
    llm: &dyn LlmClient,
    structured: &StructuredNote,
    candidates: &[SimilarNote],
) -> Option<AppendSuggestion> {
    if candidates.is_empty() {
        return None;
    }
    match try_find_match(llm, structured, candidates).await {
        Ok(s) => s,
        Err(e) => {
            if !matches!(e, LlmError::NotConfigured) {
                tracing::warn!(error = %e, "llm find_best_match failed");
            }
            // fallback: if top candidate score > 0.85, suggest it
            candidates
                .first()
                .filter(|c| c.score > 0.85)
                .map(|c| AppendSuggestion {
                    target_id: c.id.clone(),
                    target_title: c.title.clone(),
                    score: c.score,
                    reason: "высокое совпадение по вектору".into(),
                })
        }
    }
}

async fn try_find_match(
    llm: &dyn LlmClient,
    structured: &StructuredNote,
    candidates: &[SimilarNote],
) -> Result<Option<AppendSuggestion>, LlmError> {
    let candidates_str: String = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. [id={}] \"{}\" (score: {:.2})", i + 1, c.id, c.title, c.score))
        .collect::<Vec<_>>()
        .join("\n");
    let system = "You decide whether a new note should be appended to an existing one.\n\
Given a new note and a list of similar existing notes, respond with JSON only:\n\
{\"append\": true/false, \"target_id\": string|null, \"reason\": string}\n\
- append=true only if the new note clearly extends the same topic.\n\
- If unsure, set append=false.\n\
- reason: short explanation in the same language as the note.";
    let user_msg = format!(
        "New note:\n  title: {}\n  tags: {:?}\n\nExisting similar notes:\n{}",
        structured.title, structured.tags, candidates_str
    );
    let text = llm.complete_text(system, &user_msg).await?;
    let json = extract_json(&text)
        .ok_or_else(|| LlmError::BadResponse("no json".into()))?;
    let v: Value = serde_json::from_str(json)
        .map_err(|e| LlmError::BadResponse(e.to_string()))?;
    let should_append = v.get("append").and_then(|x| x.as_bool()).unwrap_or(false);
    if !should_append {
        return Ok(None);
    }
    let target_id = v
        .get("target_id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| LlmError::BadResponse("missing target_id".into()))?
        .to_string();
    let reason = v
        .get("reason")
        .and_then(|x| x.as_str())
        .unwrap_or("тема совпадает")
        .to_string();
    let candidate = candidates.iter().find(|c| c.id == target_id);
    match candidate {
        Some(c) => Ok(Some(AppendSuggestion {
            target_id: c.id.clone(),
            target_title: c.title.clone(),
            score: c.score,
            reason,
        })),
        None => Ok(None), // LLM returned unknown id
    }
}

// ------------------------- helpers -------------------------

/// Extracts the first balanced `{...}` block from an LLM response.
///
/// Handles:
/// - Markdown code fences (```json ... ```)
/// - Reasoning prefix ("Thinking Process:...", `<think>...</think>`)
/// - Multiple `{...}` blocks — picks the one that looks like a valid answer
///   (has `items` / `type` / `keywords` / `text` / `title` keys)
/// - Nested braces (counts them, skips string contents)
fn extract_json(text: &str) -> Option<&str> {
    let cleaned = strip_reasoning_preamble(text);
    let stripped = cleaned
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```JSON")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // Collect ALL balanced top-level `{...}` blocks.
    let blocks = find_all_balanced_blocks(stripped);
    if blocks.is_empty() {
        return None;
    }

    // Prefer the LAST block that parses and has an expected key.
    // Reasoning models write thinking first and the actual JSON last.
    const EXPECTED_KEYS: &[&str] =
        &["items", "type", "keywords", "text", "title", "data", "lessons"];

    for (start, end) in blocks.iter().rev() {
        let slice = &stripped[*start..=*end];
        if let Ok(v) = serde_json::from_str::<Value>(slice) {
            if v.is_object()
                && EXPECTED_KEYS
                    .iter()
                    .any(|k| v.get(k).is_some())
            {
                return Some(slice);
            }
        }
    }

    // Fallback: last block that at least parses as JSON.
    for (start, end) in blocks.iter().rev() {
        let slice = &stripped[*start..=*end];
        if serde_json::from_str::<Value>(slice).is_ok() {
            return Some(slice);
        }
    }

    // Last resort: first block (whatever it is — caller will produce
    // a clear parse error with the raw payload).
    let (s, e) = blocks[0];
    Some(&stripped[s..=e])
}

/// Strip common chain-of-thought prefixes produced by reasoning models.
/// Returns a slice of the input starting after the preamble (if any).
fn strip_reasoning_preamble(text: &str) -> &str {
    // DeepSeek / Qwen-style <think>...</think>
    if let Some(pos) = text.find("</think>") {
        return &text[pos + "</think>".len()..];
    }
    // Some models use <|thinking|>...<|/thinking|>
    if let Some(pos) = text.find("<|/thinking|>") {
        return &text[pos + "<|/thinking|>".len()..];
    }
    // "Thinking Process:" / "<thinking>" — cut until the first empty line if we
    // can, otherwise keep original and rely on block scanner below.
    for marker in ["Thinking Process:", "<thinking>", "Chain of Thought:"] {
        if text.trim_start().starts_with(marker) {
            // Try to find a delimiter that looks like end-of-thinking.
            // Heuristics: "Response:" / "Final Answer:" / "JSON:" / "\n\n{"
            for end_marker in [
                "\nResponse:",
                "\nFinal Answer:",
                "\nFINAL ANSWER:",
                "\nJSON:",
                "\nAnswer:",
            ] {
                if let Some(pos) = text.find(end_marker) {
                    return &text[pos + end_marker.len()..];
                }
            }
            // Fallback: return original; the block scanner will pick the last JSON.
            break;
        }
    }
    text
}

/// Scan text for top-level balanced `{...}` blocks, respecting strings/escapes.
/// Returns `(start, end)` byte indices (inclusive) for each complete block.
fn find_all_balanced_blocks(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut blocks = Vec::new();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut cur_start: Option<usize> = None;

    for (i, &b) in bytes.iter().enumerate() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if in_string {
            match b {
                b'\\' => escape_next = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => {
                if depth == 0 {
                    cur_start = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = cur_start.take() {
                        blocks.push((s, i));
                    }
                }
            }
            _ => {}
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_extraction_picks_outermost() {
        let s = "blah blah {\"a\": {\"b\": 1}} trailing";
        assert_eq!(extract_json(s).unwrap(), "{\"a\": {\"b\": 1}}");
    }

    #[test]
    fn fallback_note_uses_first_line() {
        let n = fallback_note("  \n## Hello world\nbody");
        assert_eq!(n.title, "Hello world");
    }

    #[test]
    fn fallback_keywords_are_lowercase_and_deduped() {
        let k = fallback_keywords("Rust rust Obsidian 42");
        assert!(k.contains(&"rust".to_string()));
        assert!(k.contains(&"obsidian".to_string()));
    }

    #[test]
    fn extract_json_skips_reasoning_and_picks_answer() {
        // Corporate / reasoning model response — chain-of-thought then real JSON.
        let raw = "Thinking Process:\n\
                   1. **Analyze**: user wants to create a note.\n\
                   2. An example might be {wrong: value} but that's not the answer.\n\
                   3. Final output follows.\n\
                   \n\
                   {\"items\":[{\"type\":\"note\",\"data\":{\"title\":\"test\",\"content\":\"\",\"tags\":[]}}]}";
        let got = extract_json(raw).expect("should pick real JSON");
        let v: Value = serde_json::from_str(got).unwrap();
        assert!(v.get("items").is_some());
    }

    #[test]
    fn extract_json_handles_think_tags() {
        let raw = "<think>let me think about this</think>{\"type\":\"todo\",\"data\":{\"title\":\"buy milk\",\"due_at\":null}}";
        let got = extract_json(raw).expect("should extract after </think>");
        let v: Value = serde_json::from_str(got).unwrap();
        assert_eq!(v["type"], "todo");
    }

    #[test]
    fn extract_json_prefers_last_valid_with_expected_keys() {
        // Two valid blocks: one with junk, one with our expected shape.
        let raw = "analysis: {\"x\":1, \"y\":2}\nresult:\n{\"type\":\"note\",\"data\":{}}";
        let got = extract_json(raw).unwrap();
        let v: Value = serde_json::from_str(got).unwrap();
        assert_eq!(v["type"], "note");
    }
}
