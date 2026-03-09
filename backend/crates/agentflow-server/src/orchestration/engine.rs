use anyhow::{anyhow, Result};
use chrono::Utc;
use dashmap::DashMap;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info};
use uuid::Uuid;

use agentflow_agents::ExecutorPool;
use agentflow_core::{
    graph::{AgentNode, AgentNodeState, WorkflowGraph},
    protocol::ws::{HitlDecision, WorkflowId, WsEvent},
    state_machine::WorkflowStateMachine,
};
use agentflow_memory::MemoryManager;

use super::hitl::HitlManager;

const GROUP_CHAT_MESSAGE_KEY: &str = "group_chat_message";

async fn persist_hitl_chat_message(
    memory_manager: &MemoryManager,
    workflow_id: &str,
    node_id: &str,
    description: String,
    context: serde_json::Value,
    status: &str,
    reason: Option<String>,
) {
    let value = json!({
        "type": "hitl",
        "id": Uuid::new_v4().to_string(),
        "workflow_id": workflow_id,
        "node_id": node_id,
        "description": description,
        "context": context,
        "status": status,
        "reason": reason,
        "timestamp": Utc::now().timestamp_millis(),
    });

    let _ = memory_manager
        .write_global(workflow_id, node_id, GROUP_CHAT_MESSAGE_KEY, value)
        .await;
}

pub struct WorkflowEngine {
    state_machines: Arc<DashMap<WorkflowId, WorkflowStateMachine>>,
    memory_manager: Arc<MemoryManager>,
    executor_pool: Arc<ExecutorPool>,
    hitl_manager: Arc<HitlManager>,
    event_tx: broadcast::Sender<WsEvent>,
}

impl WorkflowEngine {
    pub fn new(
        memory_manager: Arc<MemoryManager>,
        executor_pool: Arc<ExecutorPool>,
        hitl_manager: Arc<HitlManager>,
        event_tx: broadcast::Sender<WsEvent>,
    ) -> Self {
        Self {
            state_machines: Arc::new(DashMap::new()),
            memory_manager,
            executor_pool,
            hitl_manager,
            event_tx,
        }
    }

    pub async fn start_workflow(
        &self,
        workflow_id: WorkflowId,
        graph: WorkflowGraph,
    ) -> Result<()> {
        self.start_workflow_with_input(workflow_id, graph, None)
            .await
    }

    pub async fn start_workflow_with_input(
        &self,
        workflow_id: WorkflowId,
        graph: WorkflowGraph,
        initial_context: Option<String>,
    ) -> Result<()> {
        graph.validate()?;

        if self.state_machines.contains_key(&workflow_id) {
            return Err(anyhow!("Workflow {} is already running", workflow_id));
        }

        let sm = WorkflowStateMachine::new(workflow_id.clone());
        self.state_machines.insert(workflow_id.clone(), sm);

        let initial_context = initial_context
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let memory_manager = self.memory_manager.clone();
        let executor_pool = self.executor_pool.clone();
        let hitl_manager = self.hitl_manager.clone();
        let event_tx = self.event_tx.clone();
        let engine_event_tx = self.event_tx.clone();
        let state_machines = self.state_machines.clone();

        if let Some(context) = initial_context.as_ref() {
            self.memory_manager
                .write_global(
                    &workflow_id,
                    "workflow",
                    "workflow_run_input",
                    json!({ "text": context }),
                )
                .await
                .ok();
        }

        tokio::spawn(async move {
            let result = Self::run_workflow(
                workflow_id.clone(),
                graph,
                initial_context.unwrap_or_default(),
                memory_manager,
                executor_pool,
                hitl_manager,
                event_tx,
            )
            .await;

            state_machines.remove(&workflow_id);

            if let Err(e) = result {
                error!("Workflow {} failed: {}", workflow_id, e);
                let _ = engine_event_tx.send(WsEvent::WorkflowAborted {
                    workflow_id,
                    reason: e.to_string(),
                });
            }
        });

        Ok(())
    }

    /// Topology-driven DAG execution.
    ///
    /// - Root nodes (no incoming edges) start immediately in parallel.
    /// - When a node finishes, its output is collected.
    /// - Each successor runs once all its predecessors have finished —
    ///   it receives their combined outputs as context.
    /// - Nodes with role == "human_in_loop" pause for HITL approval instead
    ///   of running an LLM.
    async fn run_workflow(
        workflow_id: WorkflowId,
        graph: WorkflowGraph,
        initial_context: String,
        memory_manager: Arc<MemoryManager>,
        executor_pool: Arc<ExecutorPool>,
        hitl_manager: Arc<HitlManager>,
        event_tx: broadcast::Sender<WsEvent>,
    ) -> Result<()> {
        let emit = |event: WsEvent| {
            let _ = event_tx.send(event);
        };

        emit(WsEvent::WorkflowStateChanged {
            workflow_id: workflow_id.clone(),
            state: "Running".into(),
        });

        // ── Build in-degree map ───────────────────────────────────────────────
        let mut in_degree: HashMap<String, usize> =
            graph.nodes.keys().map(|id| (id.clone(), 0)).collect();
        for edge in &graph.edges {
            *in_degree.entry(edge.target.clone()).or_insert(0) += 1;
        }

        // ── Shared outputs map: node_id → text output ─────────────────────────
        let outputs: Arc<DashMap<String, String>> = Arc::new(DashMap::new());

        // ── Completion channel: (node_id, output_text, aborted_reason) ────────
        let (done_tx, mut done_rx) = mpsc::channel::<(String, String, Option<String>)>(64);

        // ── Launch root nodes ─────────────────────────────────────────────────
        let mut running = 0usize;
        for node in graph.get_root_nodes() {
            running += 1;
            spawn_node(
                node.clone(),
                initial_context.clone(),
                workflow_id.clone(),
                executor_pool.clone(),
                memory_manager.clone(),
                hitl_manager.clone(),
                event_tx.clone(),
                done_tx.clone(),
            );
        }

        if running == 0 {
            emit(WsEvent::WorkflowAborted {
                workflow_id,
                reason: "Workflow has no nodes to execute".into(),
            });
            return Ok(());
        }

        let mut final_output = String::new();

        // ── Process completions ───────────────────────────────────────────────
        while running > 0 {
            let (node_id, output, abort_reason) = match done_rx.recv().await {
                Some(v) => v,
                None => break,
            };
            running -= 1;

            if let Some(reason) = abort_reason {
                emit(WsEvent::WorkflowAborted {
                    workflow_id: workflow_id.clone(),
                    reason,
                });
                return Ok(());
            }

            outputs.insert(node_id.clone(), output.clone());
            final_output = output;

            // Check successors — start any that are now unblocked
            for successor_id in graph.get_successor_ids(&node_id) {
                let deg = in_degree.entry(successor_id.clone()).or_insert(0);
                if *deg > 0 {
                    *deg -= 1;
                }

                if *deg == 0 {
                    // Collect all predecessor outputs as context
                    let pred_ids = graph.get_predecessor_ids(&successor_id);
                    let context = pred_ids
                        .iter()
                        .filter_map(|pid| {
                            let label = graph
                                .get_node(pid)
                                .map(|n| n.label.as_str())
                                .unwrap_or(pid.as_str());
                            outputs.get(pid).map(|o| {
                                format!("## Output from {}\n\n{}", label, o.value().clone())
                            })
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n---\n\n");

                    if let Some(successor) = graph.get_node(&successor_id) {
                        running += 1;
                        spawn_node(
                            successor.clone(),
                            context,
                            workflow_id.clone(),
                            executor_pool.clone(),
                            memory_manager.clone(),
                            hitl_manager.clone(),
                            event_tx.clone(),
                            done_tx.clone(),
                        );
                    }
                }
            }
        }

        // ── Write final output to global memory ───────────────────────────────
        memory_manager
            .write_global(
                &workflow_id,
                "workflow",
                "final_output",
                json!({
                    "text": final_output,
                }),
            )
            .await
            .ok();

        emit(WsEvent::WorkflowStateChanged {
            workflow_id: workflow_id.clone(),
            state: "Completed".into(),
        });
        emit(WsEvent::WorkflowCompleted {
            workflow_id,
            summary: final_output,
            results: vec![],
        });

        Ok(())
    }

    pub fn stop_workflow(&self, workflow_id: &str) {
        self.state_machines.remove(workflow_id);
    }
}

/// Spawn a single node execution as a tokio task.
/// Sends `(node_id, output, abort_reason)` on `done_tx` when done.
fn spawn_node(
    node: AgentNode,
    input_context: String,
    workflow_id: WorkflowId,
    executor_pool: Arc<ExecutorPool>,
    memory_manager: Arc<MemoryManager>,
    hitl_manager: Arc<HitlManager>,
    event_tx: broadcast::Sender<WsEvent>,
    done_tx: mpsc::Sender<(String, String, Option<String>)>,
) {
    tokio::spawn(async move {
        let emit = |event: WsEvent| {
            let _ = event_tx.send(event);
        };
        let node_id = node.id.clone();

        emit(WsEvent::NodeStateChanged {
            workflow_id: workflow_id.clone(),
            node_id: node_id.clone(),
            state: AgentNodeState::Running,
        });

        // ── HITL pause ───────────────────────────────────────────────────────
        if node.role == "human_in_loop" {
            let hitl_context =
                json!({ "node": node.label.clone(), "input": input_context.clone() });
            let hitl_description = format!("Human review required: {}", node.label);

            emit(WsEvent::NodeStateChanged {
                workflow_id: workflow_id.clone(),
                node_id: node_id.clone(),
                state: AgentNodeState::Paused,
            });
            emit(WsEvent::HitlPaused {
                workflow_id: workflow_id.clone(),
                node_id: node_id.clone(),
                context: hitl_context.clone(),
                description: hitl_description.clone(),
            });
            persist_hitl_chat_message(
                &memory_manager,
                &workflow_id,
                &node_id,
                hitl_description.clone(),
                hitl_context.clone(),
                "pending",
                None,
            )
            .await;

            let rx = hitl_manager.register(workflow_id.clone());
            match rx.await {
                Ok(HitlDecision::Approved) => {
                    info!(
                        "HITL approved for node {} in workflow {}",
                        node_id, workflow_id
                    );
                    emit(WsEvent::NodeStateChanged {
                        workflow_id: workflow_id.clone(),
                        node_id: node_id.clone(),
                        state: AgentNodeState::Completed,
                    });
                    persist_hitl_chat_message(
                        &memory_manager,
                        &workflow_id,
                        &node_id,
                        hitl_description.clone(),
                        hitl_context.clone(),
                        "approved",
                        None,
                    )
                    .await;
                    let _ = done_tx.send((node_id, "Approved".into(), None)).await;
                }
                Ok(HitlDecision::Rejected { reason }) => {
                    emit(WsEvent::NodeStateChanged {
                        workflow_id: workflow_id.clone(),
                        node_id: node_id.clone(),
                        state: AgentNodeState::Failed,
                    });
                    persist_hitl_chat_message(
                        &memory_manager,
                        &workflow_id,
                        &node_id,
                        hitl_description.clone(),
                        hitl_context.clone(),
                        "rejected",
                        Some(reason.clone()),
                    )
                    .await;
                    let _ = done_tx
                        .send((
                            node_id,
                            String::new(),
                            Some(format!("HITL rejected by {}: {}", node.label, reason)),
                        ))
                        .await;
                }
                Err(_) => {
                    let _ = done_tx
                        .send((node_id, String::new(), Some("HITL channel closed".into())))
                        .await;
                }
            }
            return;
        }

        // ── Normal agent execution ────────────────────────────────────────────
        memory_manager.create_context_pool_with_prompt(
            node.context_pool_id.clone(),
            node.id.clone(),
            node.model_config.system_prompt.clone(),
        );

        let task_prompt = if input_context.is_empty() {
            format!("Begin your task. Your role is: {}", node.role)
        } else {
            format!(
                "Your role: {}\n\n{}\n\nBased on the above context, complete your task.",
                node.role, input_context
            )
        };

        let executor = executor_pool.executor();
        match executor
            .run_task(&node, &task_prompt, &memory_manager)
            .await
        {
            Ok(output) => {
                info!("Node {} completed in workflow {}", node_id, workflow_id);
                emit(WsEvent::NodeStateChanged {
                    workflow_id: workflow_id.clone(),
                    node_id: node_id.clone(),
                    state: AgentNodeState::Completed,
                });
                let _ = done_tx.send((node_id, output, None)).await;
            }
            Err(e) => {
                error!("Node {} failed in workflow {}: {}", node_id, workflow_id, e);
                emit(WsEvent::NodeStateChanged {
                    workflow_id: workflow_id.clone(),
                    node_id: node_id.clone(),
                    state: AgentNodeState::Failed,
                });
                let _ = done_tx
                    .send((node_id, String::new(), Some(e.to_string())))
                    .await;
            }
        }
    });
}
