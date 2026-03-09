use anyhow::Result;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;

use agentflow_agents::executor::ExecutorConfig;
use agentflow_agents::native::pty_broker::PtyHandle;
use agentflow_agents::ExecutorPool;
use agentflow_core::protocol::ws::WsEvent;
use agentflow_memory::{GlobalStore, MemoryManager};

use crate::orchestration::engine::WorkflowEngine;
use crate::orchestration::hitl::HitlManager;

#[derive(Debug, Clone)]
pub struct GroupNativeSession {
    pub kind: String,
    pub handle: String,
}

pub struct AppState {
    pub workflow_engine: Arc<WorkflowEngine>,
    pub memory_manager: Arc<MemoryManager>,
    pub hitl_manager: Arc<HitlManager>,
    pub executor_pool: Arc<ExecutorPool>,
    pub event_tx: broadcast::Sender<WsEvent>,
    /// Chat history per agent: key = "{workflow_id}_{agent_id}" → conversation turns
    pub chat_histories: Arc<DashMap<String, Vec<Value>>>,
    /// Persistent native group-chat session handles: key = "group_{workflow_id}_{agent_id}"
    pub group_native_sessions: Arc<DashMap<String, GroupNativeSession>>,
    /// Active PTY sessions: session_id → PtyHandle
    pub pty_sessions: Arc<DashMap<String, PtyHandle>>,
}

impl AppState {
    pub async fn new(database_url: &str) -> Result<Self> {
        let global_store = GlobalStore::new(database_url).await?;
        let memory_manager = Arc::new(MemoryManager::new(global_store));

        let executor_config = ExecutorConfig::from_env();
        let executor_pool = Arc::new(ExecutorPool::new(executor_config));

        let hitl_manager = Arc::new(HitlManager::new());

        let (event_tx, _) = broadcast::channel(1024);

        let workflow_engine = Arc::new(WorkflowEngine::new(
            memory_manager.clone(),
            executor_pool.clone(),
            hitl_manager.clone(),
            event_tx.clone(),
        ));

        Ok(Self {
            workflow_engine,
            memory_manager,
            hitl_manager,
            executor_pool,
            event_tx,
            chat_histories: Arc::new(DashMap::new()),
            group_native_sessions: Arc::new(DashMap::new()),
            pty_sessions: Arc::new(DashMap::new()),
        })
    }
}
