//! WebSocket bridge for PTY sessions: `/ws/pty/:session_id`
//!
//! Protocol (after WS upgrade):
//!   Client → Server  binary frame  →  raw bytes sent to PTY stdin
//!   Client → Server  text  frame   →  "resize:{cols},{rows}"  (terminal resize)
//!   Server → Client  binary frame  →  raw PTY output (ANSI/control codes)
//!   Server closes                  →  PTY session ended or not found

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    response::Response,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use tracing::{debug, info, warn};

use crate::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws/pty/:session_id", get(pty_ws_handler))
        .with_state(state)
}

async fn pty_ws_handler(
    Path(session_id): Path<String>,
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_pty_socket(socket, session_id, state))
}

async fn handle_pty_socket(socket: WebSocket, session_id: String, state: Arc<AppState>) {
    let handle = match state.pty_sessions.get(&session_id) {
        Some(h) => h.value().clone(),
        None => {
            warn!("PTY WS: session {} not found", session_id);
            return;
        }
    };

    info!("PTY WS: client connected to session {}", session_id);

    let mut output_rx = handle.subscribe_output();
    let mut exit_rx = handle.subscribe_exit();
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Task A: PTY output → WebSocket (binary frames)
    let tx_task = tokio::spawn(async move {
        if *exit_rx.borrow() {
            let _ = ws_tx.send(Message::Close(None)).await;
            return;
        }

        loop {
            tokio::select! {
                recv = output_rx.recv() => {
                    match recv {
                        Ok(bytes) => {
                            if ws_tx.send(Message::Binary(bytes.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            debug!("PTY WS output lagged by {} messages", n);
                        }
                    }
                }
                changed = exit_rx.changed() => {
                    match changed {
                        Ok(()) if *exit_rx.borrow() => {
                            let _ = ws_tx.send(Message::Close(None)).await;
                            break;
                        }
                        Ok(()) => {}
                        Err(_) => break,
                    }
                }
            }
        }
    });

    // Task B: WebSocket → PTY input / resize
    let handle_for_input = handle.clone();
    let rx_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Binary(bytes) => {
                    if handle_for_input.write_input(&bytes).is_err() {
                        break;
                    }
                }
                Message::Text(text) => {
                    // resize:{cols},{rows}
                    if let Some(dims) = text.strip_prefix("resize:") {
                        let parts: Vec<&str> = dims.splitn(2, ',').collect();
                        if parts.len() == 2 {
                            let cols = parts[0].parse::<u16>().unwrap_or(220);
                            let rows = parts[1].parse::<u16>().unwrap_or(50);
                            let _ = handle_for_input.resize(cols, rows);
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either side to close
    tokio::select! {
        _ = tx_task => {},
        _ = rx_task => {},
    }

    if *handle.subscribe_exit().borrow() {
        state.pty_sessions.remove(&session_id);
    }

    info!("PTY WS: session {} disconnected", session_id);
}
