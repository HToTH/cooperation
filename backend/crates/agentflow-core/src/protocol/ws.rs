use super::messages::{AgentMessage, TaskResultPayload};
use crate::graph::{AgentNodeState, WorkflowGraph};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type WorkflowId = String;
pub type TaskId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HitlDecision {
    Approved,
    Rejected { reason: String },
}

/// Commands from frontend -> backend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum WsCommand {
    StartWorkflow {
        workflow_id: WorkflowId,
        graph: WorkflowGraph,
    },
    StopWorkflow {
        workflow_id: WorkflowId,
    },
    UpdateGraph {
        workflow_id: WorkflowId,
        graph: WorkflowGraph,
    },
    HitlResume {
        workflow_id: WorkflowId,
        #[serde(default)]
        node_id: Option<String>,
        decision: HitlDecision,
    },
    QueryGlobalMemory {
        workflow_id: WorkflowId,
        query: String,
    },
}

/// Events from backend -> frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum WsEvent {
    WorkflowStateChanged {
        workflow_id: WorkflowId,
        state: String,
    },
    NodeStateChanged {
        workflow_id: WorkflowId,
        node_id: String,
        state: AgentNodeState,
    },
    AgentMessageSent {
        workflow_id: WorkflowId,
        message: AgentMessage,
    },
    HitlPaused {
        workflow_id: WorkflowId,
        node_id: String,
        context: Value,
        description: String,
    },
    WorkflowCompleted {
        workflow_id: WorkflowId,
        summary: String,
        results: Vec<TaskResultPayload>,
    },
    WorkflowAborted {
        workflow_id: WorkflowId,
        reason: String,
    },
    GlobalMemoryQueryResult {
        workflow_id: WorkflowId,
        query: String,
        results: Vec<Value>,
    },
    Error {
        workflow_id: Option<WorkflowId>,
        code: String,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_command_serialization() {
        let cmd = WsCommand::HitlResume {
            workflow_id: "wf_001".into(),
            node_id: None,
            decision: HitlDecision::Approved,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let decoded: WsCommand = serde_json::from_str(&json).unwrap();
        match decoded {
            WsCommand::HitlResume {
                workflow_id,
                node_id,
                decision,
            } => {
                assert_eq!(workflow_id, "wf_001");
                assert_eq!(node_id, None);
                assert_eq!(decision, HitlDecision::Approved);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_ws_event_serialization() {
        let event = WsEvent::WorkflowStateChanged {
            workflow_id: "wf_001".into(),
            state: "WorkersRunning".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: WsEvent = serde_json::from_str(&json).unwrap();
        match decoded {
            WsEvent::WorkflowStateChanged { workflow_id, state } => {
                assert_eq!(workflow_id, "wf_001");
                assert_eq!(state, "WorkersRunning");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_hitl_decision_rejected() {
        let decision = HitlDecision::Rejected {
            reason: "Too risky".into(),
        };
        let json = serde_json::to_string(&decision).unwrap();
        let decoded: HitlDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, decision);
    }
}
