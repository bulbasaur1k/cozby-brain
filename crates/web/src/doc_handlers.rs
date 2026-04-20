//! HTTP handlers for the Documentation module.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use ractor::call;
use serde::Deserialize;
use serde_json::json;

use crate::app_state::AppState;
use actors::doc_actor::{DocMsg, DocOp};

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(error = %e, "doc actor call failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    )
}

// ---------------- Projects ----------------

#[derive(Deserialize)]
pub struct CreateProjectDto {
    #[serde(default)]
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub async fn create_project(
    State(state): State<AppState>,
    Json(dto): Json<CreateProjectDto>,
) -> impl IntoResponse {
    match call!(
        state.doc_actor,
        DocMsg::CreateProject,
        dto.slug,
        dto.title,
        dto.description,
        dto.tags
    ) {
        Ok(Ok(p)) => (
            StatusCode::CREATED,
            Json(json!({ "status": "ok", "data": p })),
        )
            .into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn list_projects(State(state): State<AppState>) -> impl IntoResponse {
    match call!(state.doc_actor, DocMsg::ListProjects) {
        Ok(list) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": list })),
        )
            .into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Try id first, else treat as slug via ResolveProject
    if let Ok(Some(p)) = call!(state.doc_actor, DocMsg::GetProject, id.clone()) {
        return (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": p })),
        )
            .into_response();
    }
    match call!(state.doc_actor, DocMsg::ResolveProject, id) {
        Ok(Some(p)) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": p })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "project not found" })),
        )
            .into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn delete_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match call!(state.doc_actor, DocMsg::DeleteProject, id) {
        Ok(Ok(())) => (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

// ---------------- Pages ----------------

#[derive(Deserialize)]
pub struct CreatePageDto {
    pub project: String, // slug or id or title
    pub page: String,    // title (slug auto-derived)
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Operation: "create" | "append" | "replace" | "section". Default: "create".
    #[serde(default = "default_op")]
    pub operation: String,
    #[serde(default)]
    pub section_title: Option<String>,
}

fn default_op() -> String {
    "create".into()
}

pub async fn create_page(
    State(state): State<AppState>,
    Json(dto): Json<CreatePageDto>,
) -> impl IntoResponse {
    let op = match dto.operation.as_str() {
        "append" => DocOp::Append,
        "section" => DocOp::Section,
        "replace" => DocOp::Replace,
        _ => DocOp::Create,
    };
    let author = "api".to_string();
    match call!(
        state.doc_actor,
        DocMsg::IngestDoc,
        dto.project,
        dto.page,
        dto.content,
        dto.tags,
        op,
        dto.section_title,
        author
    ) {
        Ok(Ok(page)) => {
            crate::handlers::index_async(
                state.clone(),
                page.id.clone(),
                application::ports::KIND_DOC_PAGE,
                page.title.clone(),
                page.content.clone(),
                page.tags.clone(),
            );
            (
                StatusCode::CREATED,
                Json(json!({ "status": "ok", "data": page })),
            )
                .into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn list_pages(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    // Try direct id, else resolve as slug
    let project_id = match call!(state.doc_actor, DocMsg::GetProject, project_id.clone()) {
        Ok(Some(p)) => p.id,
        _ => match call!(state.doc_actor, DocMsg::ResolveProject, project_id) {
            Ok(Some(p)) => p.id,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "project not found" })),
                )
                    .into_response();
            }
            Err(e) => return internal(e).into_response(),
        },
    };
    match call!(state.doc_actor, DocMsg::ListPages, project_id) {
        Ok(pages) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": pages })),
        )
            .into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn get_page(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match call!(state.doc_actor, DocMsg::GetPage, id) {
        Ok(Some(p)) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": p })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "page not found" })),
        )
            .into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn delete_page(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match call!(state.doc_actor, DocMsg::DeletePage, id.clone()) {
        Ok(Ok(())) => {
            crate::handlers::unindex_async(state.clone(), id);
            (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

// ---------------- History ----------------

pub async fn list_page_history(
    State(state): State<AppState>,
    Path(page_id): Path<String>,
) -> impl IntoResponse {
    match call!(state.doc_actor, DocMsg::ListPageHistory, page_id) {
        Ok(list) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": list })),
        )
            .into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn get_page_version(
    State(state): State<AppState>,
    Path((page_id, version)): Path<(String, i32)>,
) -> impl IntoResponse {
    match call!(state.doc_actor, DocMsg::GetPageVersion, page_id, version) {
        Ok(Some(v)) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": v })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "version not found" })),
        )
            .into_response(),
        Err(e) => internal(e).into_response(),
    }
}
