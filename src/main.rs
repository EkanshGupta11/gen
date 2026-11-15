mod api;
mod contracts;
mod orchestrator;
mod provider;
mod store;
mod verification;
mod worker;

use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use api::{AppState, router};
use orchestrator::Orchestrator;
use store::Store;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            env::var("RUST_LOG").unwrap_or_else(|_| "neurogenesis_ai=info,tower_http=info".into()),
        )
        .with_target(false)
        .init();

    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = env::var("PORT").unwrap_or_else(|_| "8787".into()).parse()?;
    let max_request_bytes: usize = env::var("MAX_REQUEST_BYTES")
        .unwrap_or_else(|_| "262144".into())
        .parse()?;
    if max_request_bytes == 0 {
        return Err("MAX_REQUEST_BYTES must be greater than zero".into());
    }
    let worker_timeout = Duration::from_secs(
        env::var("WORKER_TIMEOUT_SECONDS")
            .unwrap_or_else(|_| "30".into())
            .parse()?,
    );
    if worker_timeout.is_zero() {
        return Err("WORKER_TIMEOUT_SECONDS must be greater than zero".into());
    }
    let python = env::var_os("PYTHON_EXECUTABLE").unwrap_or_else(|| "python".into());
    let worker_script = env::var_os("SCIENTIFIC_WORKER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("engine/app.py"));

    let database = env::var("DATABASE_PATH").unwrap_or_else(|_| "data/neurogenesis.sqlite3".into());
    let (store, backup) = Store::initialize(database)?;
    if let Some(path) = backup {
        tracing::info!(path=%path.display(), "Created pre-migration SQLite backup");
    }
    let max_concurrency: usize = env::var("SPECIALIST_CONCURRENCY")
        .unwrap_or_else(|_| "3".into())
        .parse::<usize>()
        .unwrap_or(3)
        .clamp(1, 5);
    let provider = provider::from_environment()?;
    let orchestrator = std::sync::Arc::new(Orchestrator {
        store,
        provider: provider.provider,
        python: python.clone(),
        worker_script: worker_script.clone(),
        worker_timeout,
        max_concurrency,
    });
    let state = AppState {
        python,
        worker_script,
        worker_timeout,
        max_request_bytes,
        orchestrator,
    };
    let app = router(state);

    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "NeuroGenesis AI API listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("NeuroGenesis AI API stopped gracefully");
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install Ctrl+C handler");
    }
}
