pub mod http;
pub mod pty_ws;
pub mod websocket;

use axum::Router;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::AppState;

pub fn create_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .merge(http::router(state.clone()))
        .merge(websocket::router(state.clone()))
        .merge(pty_ws::router(state))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}
