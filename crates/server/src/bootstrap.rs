use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use ractor::{cast, Actor};
use sqlx::postgres::PgPoolOptions;

use web::app_state::AppState;
use application::ports::{EmbeddingClient, LessonSplitter, LlmClient, Notifier, VectorStore};
use actors::learning_actor::{LearningActor, LearningMsg};
use actors::note_actor::{NoteActor, NoteMsg};
use actors::reminder_actor::{ReminderActor, ReminderMsg};
use actors::todo_actor::TodoActor;
use crate::config::AppConfig;
use learning::llm_splitter::LlmLessonSplitter;
use llm::noop::NoopLlmClient;
use llm::openai_compat::OpenAICompatClient;
use notifications::composite::CompositeNotifier;
use notifications::log_notifier::LogNotifier;
use notifications::stdout_notifier::StdoutNotifier;
use persistence::learning_repo::{PgLearningTrackRepository, PgLessonRepository};
use persistence::note_repo::PgNoteRepository;
use persistence::reminder_repo::PgReminderRepository;
use persistence::todo_repo::PgTodoRepository;
use vector::noop::NoopVectorStore;
use vector::qdrant_store::QdrantVectorStore;
use web::routes::create_router;

pub async fn build_app() -> anyhow::Result<(Router, AppConfig)> {
    let cfg = AppConfig::from_env()?;
    tracing::info!(db = %mask_db_url(&cfg.database_url), addr = %cfg.http_addr, "bootstrapping");

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&cfg.database_url)
        .await?;

    sqlx::migrate!().run(&pool).await?;
    tracing::info!("migrations applied");

    // --- LLM + embedding wiring ---
    let (llm, embedding): (Arc<dyn LlmClient>, Arc<dyn EmbeddingClient>) = match cfg.llm() {
        Some((base_url, api_key, model, embedding_model)) => {
            tracing::info!(
                %base_url, %model,
                embed_model = ?embedding_model,
                authed = api_key.is_some(),
                "llm configured"
            );
            let client = Arc::new(OpenAICompatClient::with_embedding(
                base_url,
                api_key,
                model,
                embedding_model,
            ));
            (client.clone(), client)
        }
        None => {
            tracing::info!("llm not configured — using noop (fallback heuristics)");
            let noop = Arc::new(NoopLlmClient);
            (noop.clone(), noop)
        }
    };

    // --- vector store (Qdrant) ---
    let vector: Arc<dyn VectorStore> = match cfg.qdrant_url() {
        Some(url) => {
            let collection = cfg.qdrant_collection();
            tracing::info!(%url, %collection, "qdrant configured");
            let client = qdrant_client::Qdrant::from_url(&url).build()?;
            Arc::new(QdrantVectorStore::new(Arc::new(client), collection))
        }
        None => {
            tracing::info!("qdrant not configured — vector search disabled");
            Arc::new(NoopVectorStore)
        }
    };

    // --- notifier composition ---
    let notifier: Arc<dyn Notifier> = Arc::new(CompositeNotifier::new(vec![
        Arc::new(LogNotifier),
        Arc::new(StdoutNotifier),
    ]));

    // --- repositories ---
    let note_repo = Arc::new(PgNoteRepository::new(pool.clone()));
    let todo_repo = Arc::new(PgTodoRepository::new(pool.clone()));
    let reminder_repo = Arc::new(PgReminderRepository::new(pool.clone()));
    let track_repo = Arc::new(PgLearningTrackRepository::new(pool.clone()));
    let lesson_repo = Arc::new(PgLessonRepository::new(pool.clone()));

    // --- lesson splitter (LLM-powered) ---
    let splitter: Arc<dyn LessonSplitter> = Arc::new(LlmLessonSplitter::new(llm.clone()));

    // --- actors ---
    let (note_actor, _n) =
        Actor::spawn(Some("note_actor".to_string()), NoteActor { repo: note_repo }, ()).await?;
    tracing::info!("note actor spawned");

    let (todo_actor, _t) =
        Actor::spawn(Some("todo_actor".to_string()), TodoActor { repo: todo_repo }, ()).await?;
    tracing::info!("todo actor spawned");

    let (reminder_actor, _r) = Actor::spawn(
        Some("reminder_actor".to_string()),
        ReminderActor {
            repo: reminder_repo,
            notifier,
        },
        (),
    )
    .await?;
    tracing::info!("reminder actor spawned");

    let (learning_actor, _l) = Actor::spawn(
        Some("learning_actor".to_string()),
        LearningActor {
            track_repo,
            lesson_repo,
            splitter,
        },
        (),
    )
    .await?;
    tracing::info!("learning actor spawned");

    spawn_reminder_ticker(reminder_actor.clone(), Duration::from_secs(10));
    spawn_learning_ticker(
        learning_actor.clone(),
        reminder_actor.clone(),
        note_actor.clone(),
        Duration::from_secs(60 * 30), // check tracks every 30 minutes
    );

    let state = AppState {
        note_actor,
        todo_actor,
        reminder_actor,
        learning_actor,
        llm,
        embedding,
        vector,
        db: pool,
    };
    Ok((create_router(state), cfg))
}

fn spawn_reminder_ticker(actor: ractor::ActorRef<ReminderMsg>, period: Duration) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(period);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(e) = cast!(actor, ReminderMsg::CheckDue) {
                tracing::warn!(error = %e, "reminder tick: cast failed");
                break;
            }
        }
    });
}

/// Learning scheduler: every `period`, asks learning actor for due lessons.
/// For each delivered lesson — creates a Reminder ("new lesson") and a Note
/// with the lesson content, so it appears in the user's notebook.
fn spawn_learning_ticker(
    learning: ractor::ActorRef<LearningMsg>,
    reminder: ractor::ActorRef<ReminderMsg>,
    note: ractor::ActorRef<NoteMsg>,
    period: Duration,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(period);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let delivered =
                match ractor::call!(learning, LearningMsg::CheckDue) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(error = %e, "learning tick: CheckDue failed");
                        continue;
                    }
                };
            for lesson in delivered {
                // Create Reminder to notify user about the new lesson
                let reminder_text = format!("Новый урок: {}", lesson.title);
                let now = chrono::Utc::now();
                let _ = ractor::call!(
                    reminder,
                    ReminderMsg::Create,
                    reminder_text,
                    now
                );

                // Create Note with full lesson content + `learning` tag
                let tags = vec!["learning".to_string()];
                let _ = ractor::call!(
                    note,
                    NoteMsg::Create,
                    lesson.title.clone(),
                    lesson.content.clone(),
                    tags
                );

                tracing::info!(
                    lesson_id = %lesson.id,
                    title = %lesson.title,
                    "lesson delivered → reminder + note created"
                );
            }
        }
    });
}

fn mask_db_url(url: &str) -> String {
    if let Some(at) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            return format!("{}://***{}", &url[..scheme_end], &url[at..]);
        }
    }
    url.to_string()
}
