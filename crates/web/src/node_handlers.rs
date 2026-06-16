//! HTTP handlers for Nodes — composite entities («узлы»).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use ractor::call;
use serde::Deserialize;
use serde_json::json;

use crate::app_state::AppState;
use crate::handlers::index_async;
use actors::node_actor::NodeMsg;
use application::ports::KIND_NODE;

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(error = %e, "node actor call failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    )
}

#[derive(Deserialize)]
pub struct CreateNodeDto {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Deserialize)]
pub struct ListNodesQuery {
    #[serde(default)]
    pub kind: Option<String>,
}

pub async fn create_node(
    State(state): State<AppState>,
    Json(dto): Json<CreateNodeDto>,
) -> impl IntoResponse {
    match call!(state.node_actor, NodeMsg::Create, dto.kind, dto.title, dto.tags) {
        Ok(Ok(node)) => {
            // index node for RAG/match (content = title initially; summary added later)
            index_async(
                state.clone(),
                node.id.clone(),
                KIND_NODE,
                node.title.clone(),
                node.title.clone(),
                node.tags.clone(),
            );
            (StatusCode::CREATED, Json(json!({ "status": "ok", "data": node }))).into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn list_nodes(
    State(state): State<AppState>,
    Query(q): Query<ListNodesQuery>,
) -> impl IntoResponse {
    let result = match q.kind {
        Some(kind) => call!(state.node_actor, NodeMsg::ListByKind, kind),
        None => call!(state.node_actor, NodeMsg::List),
    };
    match result {
        Ok(list) => {
            (StatusCode::OK, Json(json!({ "status": "ok", "data": list }))).into_response()
        }
        Err(e) => internal(e).into_response(),
    }
}

pub async fn get_node(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let node = match call!(state.node_actor, NodeMsg::Get, id.clone()) {
        Ok(Some(n)) => n,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(json!({ "error": "node not found" })))
                .into_response()
        }
        Err(e) => return internal(e).into_response(),
    };
    let members = match call!(state.node_actor, NodeMsg::ListMembers, id) {
        Ok(m) => m,
        Err(e) => return internal(e).into_response(),
    };
    (
        StatusCode::OK,
        Json(json!({ "status": "ok", "data": { "node": node, "members": members } })),
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct UpdateNodeDto {
    pub summary: Option<String>,
    pub status: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn update_node(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(dto): Json<UpdateNodeDto>,
) -> impl IntoResponse {
    let mut last: Option<domain::entities::Node> = None;
    if let Some(summary) = dto.summary {
        match call!(state.node_actor, NodeMsg::UpdateSummary, id.clone(), summary) {
            Ok(Ok(n)) => last = Some(n),
            Ok(Err(e)) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
            Err(e) => return internal(e).into_response(),
        }
    }
    if let Some(status) = dto.status {
        match call!(state.node_actor, NodeMsg::SetStatus, id.clone(), status) {
            Ok(Ok(n)) => last = Some(n),
            Ok(Err(e)) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
            Err(e) => return internal(e).into_response(),
        }
    }
    if let Some(metadata) = dto.metadata {
        match call!(state.node_actor, NodeMsg::SetMetadata, id.clone(), metadata) {
            Ok(Ok(n)) => last = Some(n),
            Ok(Err(e)) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
            Err(e) => return internal(e).into_response(),
        }
    }
    match last {
        Some(node) => {
            // re-index with fresh summary
            index_async(
                state.clone(),
                node.id.clone(),
                KIND_NODE,
                node.title.clone(),
                if node.summary.is_empty() { node.title.clone() } else { node.summary.clone() },
                node.tags.clone(),
            );
            (StatusCode::OK, Json(json!({ "status": "ok", "data": node }))).into_response()
        }
        None => (StatusCode::OK, Json(json!({ "status": "ok", "data": null }))).into_response(),
    }
}

pub async fn delete_node(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // node_members are cascade-deleted by FK; vector entry removed fire-and-forget.
    match call!(state.node_actor, NodeMsg::Delete, id.clone()) {
        Ok(Ok(())) => {
            crate::handlers::unindex_async(state.clone(), id);
            (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct AddMemberDto {
    /// "note" | "todo" | "reminder" | "doc_page" | "link"
    pub kind: String,
    /// entity id, or URL for kind="link"
    pub member_id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub label: String,
}

pub async fn add_member(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(dto): Json<AddMemberDto>,
) -> impl IntoResponse {
    match call!(
        state.node_actor,
        NodeMsg::AddMember,
        id,
        dto.kind,
        dto.member_id,
        dto.role,
        dto.label
    ) {
        Ok(Ok(m)) => {
            (StatusCode::CREATED, Json(json!({ "status": "ok", "data": m }))).into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn remove_member(
    State(state): State<AppState>,
    Path((_node_id, member_pk)): Path<(String, String)>,
) -> impl IntoResponse {
    match call!(state.node_actor, NodeMsg::RemoveMember, member_pk) {
        Ok(Ok(())) => (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}
