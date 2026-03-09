use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use agentflow_core::protocol::ws::{WsCommand, WsEvent};

use crate::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut event_rx = state.event_tx.subscribe();

    info!("WebSocket client connected");

    // Task to forward backend events to the client
    let send_task = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    let json = match serde_json::to_string(&event) {
                        Ok(j) => j,
                        Err(e) => {
                            error!("Failed to serialize WsEvent: {}", e);
                            continue;
                        }
                    };
                    if sender.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Event receiver lagged by {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Process incoming commands from client
    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                error!("WebSocket receive error: {}", e);
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                info!("Received WS message ({} bytes)", text.len());
                match serde_json::from_str::<WsCommand>(&text) {
                    Ok(cmd) => {
                        if let Err(e) = handle_command(cmd, &state).await {
                            error!("Command handling error: {}", e);
                            let err_event = WsEvent::Error {
                                workflow_id: None,
                                code: "command_error".into(),
                                message: e.to_string(),
                            };
                            let _ = state.event_tx.send(err_event);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse WsCommand: {}", e);
                        let _ = state.event_tx.send(WsEvent::Error {
                            workflow_id: None,
                            code: "parse_error".into(),
                            message: format!("Invalid command: {}", e),
                        });
                    }
                }
            }
            Message::Close(_) => {
                info!("WebSocket client disconnected");
                break;
            }
            Message::Ping(_) => {
                // Axum handles pong automatically
            }
            _ => {}
        }
    }

    send_task.abort();
    info!("WebSocket connection closed");
}

async fn handle_command(cmd: WsCommand, state: &Arc<AppState>) -> anyhow::Result<()> {
    match cmd {
        WsCommand::StartWorkflow { workflow_id, graph } => {
            info!("Starting workflow: {}", workflow_id);
            state
                .workflow_engine
                .start_workflow(workflow_id, graph)
                .await?;
        }
        WsCommand::StopWorkflow { workflow_id } => {
            info!("Stopping workflow: {}", workflow_id);
            state.workflow_engine.stop_workflow(&workflow_id);
        }
        WsCommand::UpdateGraph { workflow_id, graph } => {
            info!("Updating graph for workflow: {}", workflow_id);
            let graph_json = serde_json::to_string(&graph)?;
            state
                .memory_manager
                .save_workflow(&workflow_id, &graph.name, &graph_json)
                .await?;
        }
        WsCommand::HitlResume {
            workflow_id,
            node_id: _node_id,
            decision,
        } => {
            info!("HITL resume for workflow {}: {:?}", workflow_id, decision);
            state.hitl_manager.resolve(&workflow_id, decision)?;
        }
        WsCommand::QueryGlobalMemory { workflow_id, query } => {
            info!(
                "Querying global memory for workflow {}: {}",
                workflow_id, query
            );
            let results = state
                .memory_manager
                .search_global(&workflow_id, &query)
                .await?;
            let _ = state.event_tx.send(WsEvent::GlobalMemoryQueryResult {
                workflow_id,
                query,
                results,
            });
        }
    }
    Ok(())
}
