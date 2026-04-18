use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod bootstrap;
mod config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,cozby_brain=debug")))
        .with(fmt::layer().with_target(true).with_level(true))
        .init();

    let (app, cfg) = bootstrap::build_app().await?;
    let listener = tokio::net::TcpListener::bind(&cfg.http_addr).await?;
    tracing::info!("listening on http://{}", cfg.http_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
