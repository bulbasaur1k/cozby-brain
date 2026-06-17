//! Наблюдаемость: снапшот состояния (`/api/status`) и живой поток событий
//! активности (`/api/events`, SSE).

use std::collections::HashMap;
use std::convert::Infallible;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::app_state::AppState;

/// Снапшот состояния сервисов: MCP-серверы, LLM, vector/embedding/storage.
pub async fn status(State(state): State<AppState>) -> impl IntoResponse {
    // tool counts per server (имя tool'а: mcp__<server>__<tool>)
    let tools = state.mcp.list_all_tools().await;
    let tool_count = tools.len();
    let mut per_server: HashMap<String, usize> = HashMap::new();
    for (full, _) in &tools {
        if let Some(rest) = full.strip_prefix("mcp__") {
            if let Some((server, _)) = rest.split_once("__") {
                *per_server.entry(server.to_string()).or_default() += 1;
            }
        }
    }

    let mut servers = Vec::new();
    for name in state.mcp.list_servers().await {
        let connected = state.mcp.get_server_status(&name).await.unwrap_or(false);
        let count = per_server.get(&name).copied().unwrap_or(0);
        servers.push(json!({ "name": name, "connected": connected, "tools": count }));
    }

    let info = &state.status_info;
    Json(json!({
        "status": "ok",
        "data": {
            "llm": {
                "name": info.llm_name,
                "model": info.llm_model,
                "configured": info.llm_configured,
            },
            "embedding": { "configured": info.embedding_configured },
            "vector": { "configured": info.vector_configured },
            "storage": { "configured": info.storage_configured },
            "mcp": {
                "servers": servers,
                "tool_count": tool_count,
                "agent_enabled": crate::handlers::agent_enabled(),
            },
        }
    }))
}

/// SSE-поток событий активности. Каждое событие — JSON `ActivityEvent`.
pub async fn events(State(state): State<AppState>) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.activity.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|res| match res {
        Ok(ev) => Some(Ok(Event::default()
            .json_data(&ev)
            .unwrap_or_else(|_| Event::default().comment("serialize error")))),
        // отставший подписчик (lag) — пропускаем потерянные события
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
