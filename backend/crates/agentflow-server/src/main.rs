use anyhow::Result;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod app_state;
mod attachments;
mod orchestration;
mod router;

pub use app_state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agentflow_server=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:agentflow.db".to_string());

    let state = AppState::new(&database_url).await?;
    let state = Arc::new(state);

    let app = router::create_router(state);

    let addr: std::net::SocketAddr = "0.0.0.0:8080".parse()?;
    info!("cooperation server listening on {}", addr);

    // Use TcpSocket to avoid Windows SO_REUSEADDR dual-bind issue (os error 10048)
    let socket = tokio::net::TcpSocket::new_v4()?;
    socket.bind(addr)?;
    let listener = socket.listen(1024)?;
    axum::serve(listener, app).await?;

    Ok(())
}
