use agentflow_core::protocol::ws::{HitlDecision, WorkflowId};
use agentflow_core::CoreError;
use dashmap::DashMap;
use tokio::sync::oneshot;

pub struct HitlManager {
    pending: DashMap<WorkflowId, oneshot::Sender<HitlDecision>>,
}

impl HitlManager {
    pub fn new() -> Self {
        Self {
            pending: DashMap::new(),
        }
    }

    /// Register a pending HITL decision. Returns receiver to await on.
    pub fn register(&self, workflow_id: WorkflowId) -> oneshot::Receiver<HitlDecision> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(workflow_id, tx);
        rx
    }

    /// Send the decision to the waiting workflow.
    pub fn resolve(&self, workflow_id: &str, decision: HitlDecision) -> Result<(), CoreError> {
        match self.pending.remove(workflow_id) {
            Some((_, tx)) => {
                tx.send(decision)
                    .map_err(|_| CoreError::HitlChannelClosed)?;
                Ok(())
            }
            None => Err(CoreError::WorkflowNotFound(workflow_id.to_string())),
        }
    }

    pub fn is_pending(&self, workflow_id: &str) -> bool {
        self.pending.contains_key(workflow_id)
    }
}

impl Default for HitlManager {
    fn default() -> Self {
        Self::new()
    }
}
