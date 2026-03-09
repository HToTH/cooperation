use anyhow::Result;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub mod app_state;
pub mod attachments;
pub mod orchestration;
pub mod router;

pub use app_state::AppState;

pub fn init_tracing() {
    let _ = tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agentflow_server=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .try_init();
}

pub async fn run_server(database_url: &str, addr: SocketAddr) -> Result<()> {
    // Use TcpSocket to avoid Windows SO_REUSEADDR dual-bind issue (os error 10048)
    let socket = tokio::net::TcpSocket::new_v4()?;
    socket.bind(addr)?;
    let listener = socket.listen(1024)?;
    run_server_with_listener(database_url, listener).await
}

pub async fn run_server_with_listener(database_url: &str, listener: TcpListener) -> Result<()> {
    init_tracing();

    let state = AppState::new(database_url).await?;
    let state = Arc::new(state);
    let app = router::create_router(state);
    let addr = listener.local_addr()?;

    tracing::info!("cooperation server listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}
