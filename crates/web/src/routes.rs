use axum::routing::get;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::app_state::AppState;
use crate::{doc_handlers, handlers, learning_handlers};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        // notes
        .route(
            "/api/notes",
            get(handlers::list_notes).post(handlers::create_note),
        )
        .route("/api/notes/search", get(handlers::search_notes))
        .route(
            "/api/notes/{id}",
            get(handlers::get_note)
                .put(handlers::update_note)
                .delete(handlers::delete_note),
        )
        // todos
        .route(
            "/api/todos",
            get(handlers::list_todos).post(handlers::create_todo),
        )
        .route("/api/todos/{id}", axum::routing::delete(handlers::delete_todo))
        .route("/api/todos/{id}/complete", axum::routing::post(handlers::complete_todo))
        // reminders
        .route(
            "/api/reminders",
            get(handlers::list_reminders).post(handlers::create_reminder),
        )
        .route(
            "/api/reminders/{id}",
            axum::routing::delete(handlers::delete_reminder),
        )
        // iCalendar feed — subscribe в Apple/Google/Outlook Calendar по URL
        .route("/api/ical/feed.ics", get(handlers::ical_feed))
        // llm-powered universal ingest + smart search
        .route("/api/ingest", axum::routing::post(handlers::ingest))
        .route(
            "/api/ingest/note/confirm",
            axum::routing::post(handlers::confirm_ingest_note),
        )
        .route("/api/ask", get(handlers::ask))
        // graph (semantic + wiki-link connections)
        .route("/api/graph/{id}", get(handlers::graph))
        // learning / lessons
        .route(
            "/api/learning/tracks",
            get(learning_handlers::list_tracks).post(learning_handlers::create_track),
        )
        .route(
            "/api/learning/tracks/{id}",
            get(learning_handlers::get_track).delete(learning_handlers::delete_track),
        )
        .route(
            "/api/learning/tracks/{id}/lessons",
            get(learning_handlers::list_lessons),
        )
        .route(
            "/api/learning/tracks/{id}/next",
            axum::routing::post(learning_handlers::deliver_next),
        )
        .route(
            "/api/learning/lessons/{id}/learned",
            axum::routing::post(learning_handlers::mark_learned),
        )
        .route(
            "/api/learning/lessons/{id}/skip",
            axum::routing::post(learning_handlers::skip_lesson),
        )
        // documentation
        .route(
            "/api/doc/projects",
            get(doc_handlers::list_projects).post(doc_handlers::create_project),
        )
        .route(
            "/api/doc/projects/{id}",
            get(doc_handlers::get_project).delete(doc_handlers::delete_project),
        )
        .route(
            "/api/doc/projects/{id}/pages",
            get(doc_handlers::list_pages),
        )
        .route(
            "/api/doc/pages",
            axum::routing::post(doc_handlers::create_page),
        )
        .route(
            "/api/doc/pages/{id}",
            get(doc_handlers::get_page).delete(doc_handlers::delete_page),
        )
        .route(
            "/api/doc/pages/{id}/history",
            get(doc_handlers::list_page_history),
        )
        .route(
            "/api/doc/pages/{id}/history/{version}",
            get(doc_handlers::get_page_version),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
