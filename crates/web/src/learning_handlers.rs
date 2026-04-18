//! HTTP handlers for the Learning module.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use ractor::call;
use serde::Deserialize;
use serde_json::json;

use crate::app_state::AppState;
use actors::learning_actor::LearningMsg;

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(error = %e, "learning actor call failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    )
}

#[derive(Debug, Deserialize)]
pub struct CreateTrackDto {
    pub title: String,
    /// Either `raw_text` (pasted content) or `file_path` (server-side path).
    #[serde(default)]
    pub raw_text: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default = "default_pace")]
    pub pace_hours: i32,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_pace() -> i32 {
    24
}

pub async fn create_track(
    State(state): State<AppState>,
    Json(dto): Json<CreateTrackDto>,
) -> impl IntoResponse {
    // Resolve raw text: prefer inline, else read file
    let (raw_text, source_ref) = match (dto.raw_text, dto.file_path) {
        (Some(text), Some(path)) => (text, path),
        (Some(text), None) => (text, "inline".to_string()),
        (None, Some(path)) => match std::fs::read_to_string(&path) {
            Ok(t) => (t, path),
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("read file: {e}") })),
                )
                    .into_response();
            }
        },
        (None, None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "either raw_text or file_path required" })),
            )
                .into_response();
        }
    };

    if dto.title.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "title required" })),
        )
            .into_response();
    }

    match call!(
        state.learning_actor,
        LearningMsg::CreateTrack,
        dto.title,
        source_ref,
        raw_text,
        dto.pace_hours,
        dto.tags
    ) {
        Ok(Ok(track)) => (
            StatusCode::CREATED,
            Json(json!({ "status": "ok", "data": track })),
        )
            .into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn list_tracks(State(state): State<AppState>) -> impl IntoResponse {
    match call!(state.learning_actor, LearningMsg::ListTracks) {
        Ok(tracks) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": tracks })),
        )
            .into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn get_track(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match call!(state.learning_actor, LearningMsg::GetTrack, id) {
        Ok(Some(track)) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": track })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "track not found" })),
        )
            .into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn delete_track(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match call!(state.learning_actor, LearningMsg::DeleteTrack, id) {
        Ok(Ok(())) => (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn list_lessons(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
) -> impl IntoResponse {
    match call!(state.learning_actor, LearningMsg::ListLessons, track_id) {
        Ok(lessons) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": lessons })),
        )
            .into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn deliver_next(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
) -> impl IntoResponse {
    match call!(state.learning_actor, LearningMsg::DeliverNext, track_id) {
        Ok(Ok(Some(lesson))) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": lesson })),
        )
            .into_response(),
        Ok(Ok(None)) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "data": null, "message": "no more pending lessons" })),
        )
            .into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn mark_learned(
    State(state): State<AppState>,
    Path(lesson_id): Path<String>,
) -> impl IntoResponse {
    match call!(state.learning_actor, LearningMsg::MarkLearned, lesson_id) {
        Ok(Ok(())) => (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn skip_lesson(
    State(state): State<AppState>,
    Path(lesson_id): Path<String>,
) -> impl IntoResponse {
    match call!(state.learning_actor, LearningMsg::SkipLesson, lesson_id) {
        Ok(Ok(())) => (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => internal(e).into_response(),
    }
}
