use agentflow_core::graph::node::NodeId;
use anyhow::Result;
use chrono::Utc;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

use crate::context_pool::ContextPool;
use crate::global_store::{GlobalStore, MemoryEntry};

pub struct MemoryManager {
    context_pools: Arc<DashMap<String, ContextPool>>,
    global_store: Arc<GlobalStore>,
}

impl MemoryManager {
    pub fn new(global_store: GlobalStore) -> Self {
        Self {
            context_pools: Arc::new(DashMap::new()),
            global_store: Arc::new(global_store),
        }
    }

    pub fn create_context_pool(&self, pool_id: String, owner_agent_id: NodeId) -> String {
        let pool = ContextPool::new(pool_id.clone(), owner_agent_id);
        self.context_pools.insert(pool_id.clone(), pool);
        pool_id
    }

    pub fn create_context_pool_with_prompt(
        &self,
        pool_id: String,
        owner_agent_id: NodeId,
        system_prompt: String,
    ) -> String {
        let pool =
            ContextPool::new(pool_id.clone(), owner_agent_id).with_system_prompt(system_prompt);
        self.context_pools.insert(pool_id.clone(), pool);
        pool_id
    }

    pub fn get_context_pool(
        &self,
        pool_id: &str,
    ) -> Option<dashmap::mapref::one::Ref<'_, String, ContextPool>> {
        self.context_pools.get(pool_id)
    }

    pub fn get_context_pool_mut(
        &self,
        pool_id: &str,
    ) -> Option<dashmap::mapref::one::RefMut<'_, String, ContextPool>> {
        self.context_pools.get_mut(pool_id)
    }

    pub fn remove_context_pool(&self, pool_id: &str) {
        self.context_pools.remove(pool_id);
    }

    pub async fn write_global(
        &self,
        workflow_id: &str,
        agent_id: &str,
        key: &str,
        value: Value,
    ) -> Result<()> {
        let entry = MemoryEntry {
            id: Uuid::new_v4().to_string(),
            workflow_id: workflow_id.to_string(),
            agent_id: agent_id.to_string(),
            key: key.to_string(),
            value,
            created_at: Utc::now().to_rfc3339(),
        };
        self.global_store.write(entry).await
    }

    pub async fn query_global(&self, workflow_id: &str) -> Result<Vec<Value>> {
        let entries = self.global_store.query_by_workflow(workflow_id).await?;
        Ok(entries.into_iter().map(|e| e.value).collect())
    }

    pub async fn query_global_by_key(&self, workflow_id: &str, key: &str) -> Result<Vec<Value>> {
        let entries = self.global_store.query_by_key(workflow_id, key).await?;
        Ok(entries.into_iter().map(|e| e.value).collect())
    }

    pub async fn search_global(&self, workflow_id: &str, query: &str) -> Result<Vec<Value>> {
        self.global_store.search(workflow_id, query).await
    }

    pub async fn delete_workflow(&self, id: &str) -> Result<()> {
        self.global_store.delete_workflow(id).await
    }

    pub async fn save_workflow(&self, id: &str, name: &str, graph_json: &str) -> Result<()> {
        self.global_store.save_workflow(id, name, graph_json).await
    }

    pub async fn load_workflow(&self, id: &str) -> Result<Option<String>> {
        self.global_store.load_workflow(id).await
    }

    pub async fn list_workflows(&self) -> Result<Vec<(String, String, String)>> {
        self.global_store.list_workflows().await
    }
}
