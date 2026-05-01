use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use ractor::{cast, Actor};
use sqlx::postgres::PgPoolOptions;

use web::app_state::AppState;
use application::ports::{
    AttachmentStore, EmbeddingClient, LessonSplitter, LlmClient, Notifier, VectorStore,
};
use actors::doc_actor::DocActor;
use actors::learning_actor::{LearningActor, LearningMsg};
use actors::note_actor::{NoteActor, NoteMsg};
use actors::reminder_actor::{ReminderActor, ReminderMsg};
use actors::todo_actor::TodoActor;
use crate::config::AppConfig;
use learning::llm_splitter::LlmLessonSplitter;
use llm::noop::NoopLlmClient;
use llm::openai_compat::OpenAICompatClient;
use notifications::composite::CompositeNotifier;
use notifications::desktop_notifier::DesktopNotifier;
use notifications::log_notifier::LogNotifier;
use notifications::stdout_notifier::StdoutNotifier;
use persistence::doc_repo::{
    PgDocPageHistoryRepository, PgDocPageRepository, PgProjectRepository,
};
use persistence::learning_repo::{PgLearningTrackRepository, PgLessonRepository};
use persistence::note_repo::PgNoteRepository;
use persistence::reminder_repo::PgReminderRepository;
use persistence::todo_repo::PgTodoRepository;
use storage::noop::NoopAttachmentStore;
use storage::s3_store::S3AttachmentStore;
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

    // --- attachment store (MinIO/S3) ---
    let attachments: Arc<dyn AttachmentStore> = match cfg.s3() {
        Some((endpoint, region, access, secret, bucket)) => {
            tracing::info!(%endpoint, %region, %bucket, "s3 attachment store configured");
            match S3AttachmentStore::new(endpoint, region, &access, &secret, bucket) {
                Ok(store) => {
                    if let Err(e) = store.ensure_bucket().await {
                        tracing::warn!(error = %e, "s3 bucket ensure failed; continuing");
                    }
                    Arc::new(store) as Arc<dyn AttachmentStore>
                }
                Err(e) => {
                    tracing::warn!(error = %e, "s3 init failed; using noop");
                    Arc::new(NoopAttachmentStore) as Arc<dyn AttachmentStore>
                }
            }
        }
        None => {
            tracing::info!("s3 not configured — attachments disabled");
            Arc::new(NoopAttachmentStore) as Arc<dyn AttachmentStore>
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
    // Desktop = native OS notification (macOS Notification Center, Linux libnotify).
    // macOS Glass sound plays automatically on show.
    let notifier: Arc<dyn Notifier> = Arc::new(CompositeNotifier::new(vec![
        Arc::new(LogNotifier),
        Arc::new(StdoutNotifier),
        Arc::new(DesktopNotifier::new()),
    ]));

    // --- repositories ---
    let note_repo = Arc::new(PgNoteRepository::new(pool.clone()));
    let todo_repo = Arc::new(PgTodoRepository::new(pool.clone()));
    let reminder_repo = Arc::new(PgReminderRepository::new(pool.clone()));
    let track_repo = Arc::new(PgLearningTrackRepository::new(pool.clone()));
    let lesson_repo = Arc::new(PgLessonRepository::new(pool.clone()));
    let project_repo = Arc::new(PgProjectRepository::new(pool.clone()));
    let doc_page_repo = Arc::new(PgDocPageRepository::new(pool.clone()));
    let doc_history_repo = Arc::new(PgDocPageHistoryRepository::new(pool.clone()));

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

    let (doc_actor, _d) = Actor::spawn(
        Some("doc_actor".to_string()),
        DocActor {
            project_repo,
            page_repo: doc_page_repo,
            history_repo: doc_history_repo,
        },
        (),
    )
    .await?;
    tracing::info!("doc actor spawned");

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
        doc_actor,
        llm,
        embedding,
        vector,
        attachments,
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
                    now,
                    None::<String>
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
