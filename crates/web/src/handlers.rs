use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use ractor::call;
use serde_json::json;

use chrono::Utc;

use crate::app_state::AppState;
use crate::dto::{
    AskQuery, ConfirmIngestNoteDto, CreateNoteDto, CreateReminderDto, CreateTodoDto,
    IngestRawDto, SearchQuery, StructuredNoteDto, UpdateNoteDto,
};

use application::llm_use_cases;
use application::ports::LlmError;
use domain::services;
use actors::note_actor::NoteMsg;
use actors::reminder_actor::ReminderMsg;
use actors::todo_actor::TodoMsg;

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(error = %e, "actor call failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    )
}

pub async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

// ---------- notes ----------

pub async fn list_notes(State(state): State<AppState>) -> impl IntoResponse {
    match call!(state.note_actor, NoteMsg::List) {
        Ok(notes) => (StatusCode::OK, Json(json!({ "status": "ok", "data": notes }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn create_note(
    State(state): State<AppState>,
    Json(dto): Json<CreateNoteDto>,
) -> impl IntoResponse {
    tracing::debug!(title = %dto.title, tags = ?dto.tags, "create note");
    match call!(state.note_actor, NoteMsg::Create, dto.title, dto.content, dto.tags) {
        Ok(Ok(note)) => (
            StatusCode::CREATED,
            Json(json!({ "status": "ok", "data": note })),
        )
            .into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn get_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match call!(state.note_actor, NoteMsg::Get, id) {
        Ok(Some(note)) => {
            let links = services::extract_wiki_links(&note.content);
            (
                StatusCode::OK,
                Json(json!({ "status": "ok", "data": note, "links": links })),
            )
                .into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn update_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(dto): Json<UpdateNoteDto>,
) -> impl IntoResponse {
    match call!(state.note_actor, NoteMsg::Update, id, dto.title, dto.content, dto.tags) {
        Ok(Ok(note)) => (StatusCode::OK, Json(json!({ "status": "ok", "data": note }))).into_response(),
        Ok(Err(e)) if e.starts_with("not found") => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": e }))).into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn delete_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match call!(state.note_actor, NoteMsg::Delete, id) {
        Ok(Ok(())) => (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn search_notes(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> impl IntoResponse {
    match call!(state.note_actor, NoteMsg::Search, q.q) {
        Ok(notes) => (StatusCode::OK, Json(json!({ "status": "ok", "data": notes }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

// ---------- todos ----------

pub async fn list_todos(State(state): State<AppState>) -> impl IntoResponse {
    match call!(state.todo_actor, TodoMsg::List) {
        Ok(todos) => (StatusCode::OK, Json(json!({ "status": "ok", "data": todos }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn create_todo(
    State(state): State<AppState>,
    Json(dto): Json<CreateTodoDto>,
) -> impl IntoResponse {
    match call!(state.todo_actor, TodoMsg::Create, dto.title, dto.due_at) {
        Ok(Ok(t)) => (StatusCode::CREATED, Json(json!({ "status": "ok", "data": t }))).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn complete_todo(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match call!(state.todo_actor, TodoMsg::Complete, id) {
        Ok(Ok(t)) => (StatusCode::OK, Json(json!({ "status": "ok", "data": t }))).into_response(),
        Ok(Err(e)) if e.starts_with("not found") => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": e }))).into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn delete_todo(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match call!(state.todo_actor, TodoMsg::Delete, id) {
        Ok(Ok(())) => (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

// ---------- reminders ----------

pub async fn list_reminders(State(state): State<AppState>) -> impl IntoResponse {
    match call!(state.reminder_actor, ReminderMsg::List) {
        Ok(r) => (StatusCode::OK, Json(json!({ "status": "ok", "data": r }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn create_reminder(
    State(state): State<AppState>,
    Json(dto): Json<CreateReminderDto>,
) -> impl IntoResponse {
    match call!(state.reminder_actor, ReminderMsg::Create, dto.text, dto.remind_at) {
        Ok(Ok(r)) => (StatusCode::CREATED, Json(json!({ "status": "ok", "data": r }))).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn delete_reminder(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match call!(state.reminder_actor, ReminderMsg::Delete, id) {
        Ok(Ok(())) => (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

// ---------- LLM ingest (universal) ----------

/// Universal LLM-powered ingest. LLM classifies user input into one of:
/// - note: returns structured markdown + optional append-suggestion (2-step)
/// - todo: created immediately, returned
/// - reminder: created immediately, returned
/// - question: returns search results across all entities
///
/// Returns 503 if LLM is not configured — this endpoint is LLM-mandatory.
pub async fn ingest(
    State(state): State<AppState>,
    Json(dto): Json<IngestRawDto>,
) -> impl IntoResponse {
    tracing::debug!(len = dto.raw.len(), llm = state.llm.name(), "universal ingest");

    let classified = match llm_use_cases::classify_and_structure(
        state.llm.as_ref(),
        &dto.raw,
        Utc::now(),
    )
    .await
    {
        Ok(c) => c,
        Err(LlmError::NotConfigured) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "LLM not configured. Set LLM_BASE_URL + LLM_API_KEY + LLM_MODEL, or use direct CRUD endpoints (/api/notes, /api/todos, /api/reminders)."
                })),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "classify_and_structure failed");
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("LLM error: {e}") })),
            )
                .into_response();
        }
    };

    use llm_use_cases::Classified;

    // Backward-compat response for SINGLE item — keep old flat shape:
    // { "type": "todo", "status": "ok", "data": {...} }
    // For multiple items — return { "status": "ok", "items": [{single-item-shape}, ...] }.
    let single = classified.len() == 1;
    let mut item_responses: Vec<serde_json::Value> = Vec::with_capacity(classified.len());

    for item in classified {
        let item_resp = match item {
            Classified::Note(s) => {
                // 2-step flow: return structured + suggestion, do NOT create yet.
                build_note_ingest_response(&state, s).await
            }
            Classified::Todo(t) => {
                match call!(state.todo_actor, TodoMsg::Create, t.title, t.due_at) {
                    Ok(Ok(todo)) => json!({
                        "type": "todo",
                        "status": "ok",
                        "data": todo,
                    }),
                    Ok(Err(e)) => json!({ "type": "todo", "status": "error", "error": e }),
                    Err(e) => json!({ "type": "todo", "status": "error", "error": format!("{e}") }),
                }
            }
            Classified::Reminder(r) => {
                match call!(state.reminder_actor, ReminderMsg::Create, r.text, r.remind_at) {
                    Ok(Ok(rem)) => json!({
                        "type": "reminder",
                        "status": "ok",
                        "data": rem,
                    }),
                    Ok(Err(e)) => json!({ "type": "reminder", "status": "error", "error": e }),
                    Err(e) => json!({ "type": "reminder", "status": "error", "error": format!("{e}") }),
                }
            }
            Classified::Question(q) => {
                let (notes, todos, reminders) = search_by_keywords(&state, &q.keywords).await;
                json!({
                    "type": "question",
                    "status": "ok",
                    "keywords": q.keywords,
                    "scope": q.scope,
                    "data": {
                        "notes": notes,
                        "todos": todos,
                        "reminders": reminders,
                    }
                })
            }
            Classified::Doc(d) => {
                use actors::doc_actor::{DocMsg, DocOp};
                let op = match d.operation {
                    application::llm_use_cases::DocOperation::Create => DocOp::Create,
                    application::llm_use_cases::DocOperation::Append => DocOp::Append,
                    application::llm_use_cases::DocOperation::Replace => DocOp::Replace,
                    application::llm_use_cases::DocOperation::Section => DocOp::Section,
                };
                let author = "llm".to_string();
                match call!(
                    state.doc_actor,
                    DocMsg::IngestDoc,
                    d.project,
                    d.page,
                    d.content,
                    d.tags,
                    op,
                    d.section_title,
                    author
                ) {
                    Ok(Ok(page)) => json!({
                        "type": "doc",
                        "status": "ok",
                        "data": page,
                    }),
                    Ok(Err(e)) => json!({ "type": "doc", "status": "error", "error": e }),
                    Err(e) => json!({ "type": "doc", "status": "error", "error": format!("{e}") }),
                }
            }
        };
        item_responses.push(item_resp);
    }

    if single {
        // keep flat response for single-item case (legacy client compat)
        (StatusCode::OK, Json(item_responses.into_iter().next().unwrap())).into_response()
    } else {
        (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "items": item_responses,
            })),
        )
            .into_response()
    }
}

/// Build the response value for a note ingest: structured + optional suggestion.
/// Does NOT create the note yet — step 2 (`/api/ingest/note/confirm`) does that.
async fn build_note_ingest_response(
    state: &AppState,
    s: application::llm_use_cases::StructuredNote,
) -> serde_json::Value {
    let suggestion = match state.embedding.embed(&s.content).await {
        Ok(vector) => match state.vector.search(vector, 5).await {
            Ok(candidates) if !candidates.is_empty() => {
                tracing::debug!(count = candidates.len(), "found similar notes");
                llm_use_cases::find_best_match(state.llm.as_ref(), &s, &candidates).await
            }
            _ => None,
        },
        Err(e) => {
            if !matches!(e, LlmError::NotConfigured) {
                tracing::warn!(error = %e, "embedding failed, skipping similarity search");
            }
            None
        }
    };

    json!({
        "type": "note",
        "status": "ok",
        "structured": StructuredNoteDto {
            title: s.title,
            content: s.content,
            tags: s.tags,
        },
        "suggestion": suggestion,
    })
}

/// Confirm step after `/api/ingest` returned type=note — user chose create or append.
pub async fn confirm_ingest_note(
    State(state): State<AppState>,
    Json(dto): Json<ConfirmIngestNoteDto>,
) -> impl IntoResponse {
    let result = match dto.action.as_str() {
        "append" => {
            let target_id = match dto.target_id {
                Some(id) => id,
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "target_id required for append" })),
                    )
                        .into_response();
                }
            };
            // Fetch existing note content and merge
            let existing = call!(state.note_actor, NoteMsg::Get, target_id.clone());
            match existing {
                Ok(Some(note)) => {
                    let merged_content =
                        format!("{}\n\n---\n\n{}", note.content, dto.content);
                    let mut merged_tags = note.tags.clone();
                    for t in &dto.tags {
                        if !merged_tags.contains(t) {
                            merged_tags.push(t.clone());
                        }
                    }
                    call!(
                        state.note_actor,
                        NoteMsg::Update,
                        target_id,
                        note.title,
                        merged_content,
                        merged_tags
                    )
                }
                Ok(None) => {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(json!({ "error": "target note not found" })),
                    )
                        .into_response();
                }
                Err(e) => return internal(e).into_response(),
            }
        }
        _ => {
            // "create" (default)
            call!(
                state.note_actor,
                NoteMsg::Create,
                dto.title.clone(),
                dto.content.clone(),
                dto.tags.clone()
            )
        }
    };

    match result {
        Ok(Ok(note)) => {
            // Index in Qdrant (fire-and-forget)
            let emb = state.embedding.clone();
            let vec_store = state.vector.clone();
            let note_id = note.id.clone();
            let note_title = note.title.clone();
            let note_content = note.content.clone();
            let note_tags = note.tags.clone();
            tokio::spawn(async move {
                if let Ok(vector) = emb.embed(&note_content).await {
                    if let Err(e) = vec_store.upsert(&note_id, vector, &note_title, &note_tags).await {
                        tracing::warn!(error = %e, "qdrant upsert failed (fire-and-forget)");
                    }
                }
            });
            (
                StatusCode::CREATED,
                Json(json!({ "status": "ok", "data": note })),
            )
                .into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

// ---------- search helper (reused by /api/ask and ingest question branch) ----------

async fn search_by_keywords(
    state: &AppState,
    keywords: &[String],
) -> (Vec<domain::entities::Note>, Vec<domain::entities::Todo>, Vec<domain::entities::Reminder>) {
    // notes: actor search per keyword + dedup
    let mut notes_map = std::collections::HashMap::new();
    for kw in keywords {
        if let Ok(list) = call!(state.note_actor, NoteMsg::Search, kw.clone()) {
            for n in list {
                notes_map.insert(n.id.clone(), n);
            }
        }
    }
    let mut notes: Vec<_> = notes_map.into_values().collect();
    notes.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    let todos = call!(state.todo_actor, TodoMsg::List)
        .unwrap_or_default()
        .into_iter()
        .filter(|t| match_any(&t.title, keywords))
        .collect::<Vec<_>>();

    let reminders = call!(state.reminder_actor, ReminderMsg::List)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| match_any(&r.text, keywords))
        .collect::<Vec<_>>();

    (notes, todos, reminders)
}

/// Unified smart search across notes, todos and reminders.
/// LLM extracts keywords, then each keyword queries the respective actors.
pub async fn ask(
    State(state): State<AppState>,
    Query(q): Query<AskQuery>,
) -> impl IntoResponse {
    tracing::debug!(query = %q.q, llm = state.llm.name(), "ask / smart search");
    let keywords = llm_use_cases::extract_search_keywords(state.llm.as_ref(), &q.q).await;
    let (notes, todos, reminders) = search_by_keywords(&state, &keywords).await;
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "keywords": keywords,
            "data": {
                "notes": notes,
                "todos": todos,
                "reminders": reminders,
            }
        })),
    )
        .into_response()
}

fn match_any(text: &str, keywords: &[String]) -> bool {
    if keywords.is_empty() {
        return true;
    }
    let lower = text.to_lowercase();
    keywords.iter().any(|k| lower.contains(&k.to_lowercase()))
}

// ---------- graph (connections) ----------

#[derive(Debug, serde::Deserialize)]
pub struct GraphQuery {
    #[serde(default = "default_depth")]
    pub depth: u8,
}
fn default_depth() -> u8 {
    1
}

pub async fn graph(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<GraphQuery>,
) -> impl IntoResponse {
    let root_note = match call!(state.note_actor, NoteMsg::Get, id.clone()) {
        Ok(Some(n)) => n,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(json!({ "error": "note not found" })))
                .into_response();
        }
        Err(e) => return internal(e).into_response(),
    };

    let mut nodes: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    let mut edges: Vec<serde_json::Value> = Vec::new();

    // include root
    nodes.insert(
        root_note.id.clone(),
        json!({
            "id": root_note.id,
            "title": root_note.title,
            "tags": root_note.tags,
        }),
    );

    let depth = q.depth.clamp(1, 3) as usize;
    let mut frontier = vec![root_note.clone()];
    for _ in 0..depth {
        let mut next_frontier = Vec::new();
        for note in &frontier {
            // semantic via embedding
            if let Ok(vector) = state.embedding.embed(&note.content).await {
                if let Ok(hits) = state.vector.search(vector, 5).await {
                    for hit in hits {
                        if hit.id == note.id {
                            continue;
                        }
                        edges.push(json!({
                            "from": note.id,
                            "to": hit.id,
                            "kind": "semantic",
                            "score": hit.score,
                        }));
                        if let std::collections::hash_map::Entry::Vacant(e) = nodes.entry(hit.id.clone()) {
                            e.insert(json!({
                                "id": hit.id,
                                "title": hit.title,
                                "score": hit.score,
                            }));
                            // Try to resolve full note for next depth layer
                            if let Ok(Some(next_note)) =
                                call!(state.note_actor, NoteMsg::Get, hit.id.clone())
                            {
                                next_frontier.push(next_note);
                            }
                        }
                    }
                }
            }

            // wiki-links from content
            let wiki = domain::services::extract_wiki_links(&note.content);
            for target_title in wiki {
                // resolve by fuzzy title match via actor search
                if let Ok(matches) =
                    call!(state.note_actor, NoteMsg::Search, target_title.clone())
                {
                    if let Some(target) = matches.first() {
                        if target.id == note.id {
                            continue;
                        }
                        edges.push(json!({
                            "from": note.id,
                            "to": target.id,
                            "kind": "wiki",
                            "label": target_title,
                        }));
                        if let std::collections::hash_map::Entry::Vacant(e) =
                            nodes.entry(target.id.clone())
                        {
                            e.insert(json!({
                                "id": target.id,
                                "title": target.title,
                                "tags": target.tags,
                            }));
                            next_frontier.push(target.clone());
                        }
                    }
                }
            }
        }
        frontier = next_frontier;
    }

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "root": root_note.id,
            "nodes": nodes.values().collect::<Vec<_>>(),
            "edges": edges,
        })),
    )
        .into_response()
}
