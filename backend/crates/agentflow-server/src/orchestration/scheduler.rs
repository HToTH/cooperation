use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::error;

use agentflow_core::{
    graph::{AgentNodeState, WorkflowGraph},
    protocol::{
        messages::{
            ContextObject, ExpectedFormat, TaskDispatchPayload, TaskResultPayload,
            TaskStatus,
        },
        ws::WsEvent,
    },
};
use agentflow_memory::MemoryManager;
use agentflow_agents::ExecutorPool;

pub struct Scheduler {
    memory_manager: Arc<MemoryManager>,
    executor_pool: Arc<ExecutorPool>,
    event_tx: broadcast::Sender<WsEvent>,
}

impl Scheduler {
    pub fn new(
        memory_manager: Arc<MemoryManager>,
        executor_pool: Arc<ExecutorPool>,
        event_tx: broadcast::Sender<WsEvent>,
    ) -> Self {
        Self { memory_manager, executor_pool, event_tx }
    }

    pub async fn dispatch_workers(
        &self,
        workflow_id: &str,
        graph: &WorkflowGraph,
        leader_task: &str,
    ) -> Vec<TaskResultPayload> {
        let workers = graph.get_workers();
        let executor = self.executor_pool.executor();
        let mut handles = Vec::new();

        for worker in workers {
            let worker = worker.clone();
            let workflow_id = workflow_id.to_string();
            let executor = executor.clone();
            let memory_manager = self.memory_manager.clone();
            let event_tx = self.event_tx.clone();
            let leader_task = leader_task.to_string();

            let handle = tokio::spawn(async move {
                // Signal node is running
                let _ = event_tx.send(WsEvent::NodeStateChanged {
                    workflow_id: workflow_id.clone(),
                    node_id: worker.id.clone(),
                    state: AgentNodeState::Running,
                });

                // Get or create context pool for this worker
                let pool_id = worker.context_pool_id.clone();
                memory_manager.create_context_pool_with_prompt(
                    pool_id.clone(),
                    worker.id.clone(),
                    worker.model_config.system_prompt.clone(),
                );

                let dispatch = TaskDispatchPayload {
                    task_intent: leader_task.clone(),
                    context: ContextObject(serde_json::json!({ "leader_instruction": leader_task })),
                    expected_format: ExpectedFormat {
                        schema_type: "object".into(),
                        properties: None,
                        required: None,
                    },
                    context_pool_id: pool_id.clone(),
                };

                let result = {
                    let mut pool_ref = match memory_manager.get_context_pool_mut(&pool_id) {
                        Some(p) => p,
                        None => {
                            return TaskResultPayload {
                                status: TaskStatus::Failed,
                                result: serde_json::json!({}),
                                error: Some("Context pool not found".into()),
                                execution_metadata: agentflow_core::protocol::messages::ExecutionMetadata {
                                    tokens_used: 0,
                                    context_pool_id: pool_id,
                                    duration_ms: 0,
                                },
                            };
                        }
                    };

                    let task_msg = serde_json::to_string_pretty(&dispatch).unwrap_or_default();
                    match executor.run_single_turn(&worker, &mut pool_ref, &task_msg).await {
                        Ok(r) => r,
                        Err(e) => {
                            error!("Worker {} failed: {}", worker.id, e);
                            TaskResultPayload {
                                status: TaskStatus::Failed,
                                result: serde_json::json!({}),
                                error: Some(e.to_string()),
                                execution_metadata: agentflow_core::protocol::messages::ExecutionMetadata {
                                    tokens_used: 0,
                                    context_pool_id: pool_id,
                                    duration_ms: 0,
                                },
                            }
                        }
                    }
                };

                let node_state = if result.status == TaskStatus::Completed {
                    AgentNodeState::Completed
                } else {
                    AgentNodeState::Failed
                };

                let _ = event_tx.send(WsEvent::NodeStateChanged {
                    workflow_id: workflow_id.clone(),
                    node_id: worker.id.clone(),
                    state: node_state,
                });

                result
            });

            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => {
                    error!("Worker task panicked: {}", e);
                    results.push(TaskResultPayload {
                        status: TaskStatus::Failed,
                        result: serde_json::json!({}),
                        error: Some(format!("Task panic: {}", e)),
                        execution_metadata: agentflow_core::protocol::messages::ExecutionMetadata {
                            tokens_used: 0,
                            context_pool_id: "unknown".into(),
                            duration_ms: 0,
                        },
                    });
                }
            }
        }

        results
    }
}
