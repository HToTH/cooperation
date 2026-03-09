use crate::error::CoreError;
use crate::graph::node::NodeId;
use crate::protocol::messages::TaskResultPayload;
use crate::protocol::ws::TaskId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HitlTrigger {
    pub node_id: NodeId,
    pub description: String,
    pub context_snapshot: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum WorkflowState {
    Idle,
    Planning {
        leader_id: NodeId,
    },
    Dispatching {
        pending_tasks: Vec<TaskId>,
    },
    WorkersRunning {
        active_workers: Vec<NodeId>,
    },
    AwaitingHitl {
        trigger: HitlTrigger,
        next_state: Box<WorkflowState>,
    },
    Aggregating {
        results: Vec<TaskResultPayload>,
    },
    LeaderSynthesis,
    Completed {
        summary: String,
    },
    Aborted {
        reason: String,
    },
}

impl WorkflowState {
    pub fn name(&self) -> &'static str {
        match self {
            WorkflowState::Idle => "Idle",
            WorkflowState::Planning { .. } => "Planning",
            WorkflowState::Dispatching { .. } => "Dispatching",
            WorkflowState::WorkersRunning { .. } => "WorkersRunning",
            WorkflowState::AwaitingHitl { .. } => "AwaitingHitl",
            WorkflowState::Aggregating { .. } => "Aggregating",
            WorkflowState::LeaderSynthesis => "LeaderSynthesis",
            WorkflowState::Completed { .. } => "Completed",
            WorkflowState::Aborted { .. } => "Aborted",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            WorkflowState::Completed { .. } | WorkflowState::Aborted { .. }
        )
    }
}

pub struct WorkflowStateMachine {
    pub workflow_id: String,
    pub state: WorkflowState,
}

impl WorkflowStateMachine {
    pub fn new(workflow_id: String) -> Self {
        Self {
            workflow_id,
            state: WorkflowState::Idle,
        }
    }

    pub fn transition(&mut self, new_state: WorkflowState) -> Result<(), CoreError> {
        let valid = self.is_valid_transition(&self.state, &new_state);
        if !valid {
            return Err(CoreError::InvalidStateTransition {
                from: self.state.name().to_string(),
                to: new_state.name().to_string(),
            });
        }
        self.state = new_state;
        Ok(())
    }

    fn is_valid_transition(&self, from: &WorkflowState, to: &WorkflowState) -> bool {
        use WorkflowState::*;
        matches!(
            (from, to),
            (Idle, Planning { .. })
                | (Planning { .. }, Dispatching { .. })
                | (Dispatching { .. }, WorkersRunning { .. })
                | (WorkersRunning { .. }, AwaitingHitl { .. })
                | (WorkersRunning { .. }, Aggregating { .. })
                | (AwaitingHitl { .. }, WorkersRunning { .. })
                | (AwaitingHitl { .. }, Aborted { .. })
                | (Aggregating { .. }, LeaderSynthesis)
                | (LeaderSynthesis, Completed { .. })
                | (_, Aborted { .. })
        )
    }

    pub fn current_state_name(&self) -> &'static str {
        self.state.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_valid_transitions() {
        let mut sm = WorkflowStateMachine::new("wf_test".into());
        assert_eq!(sm.current_state_name(), "Idle");

        sm.transition(WorkflowState::Planning {
            leader_id: "leader_001".into(),
        })
        .unwrap();
        assert_eq!(sm.current_state_name(), "Planning");

        sm.transition(WorkflowState::Dispatching {
            pending_tasks: vec!["t1".into()],
        })
        .unwrap();
        assert_eq!(sm.current_state_name(), "Dispatching");

        sm.transition(WorkflowState::WorkersRunning {
            active_workers: vec!["worker_001".into()],
        })
        .unwrap();
        assert_eq!(sm.current_state_name(), "WorkersRunning");

        sm.transition(WorkflowState::Aggregating { results: vec![] })
            .unwrap();
        sm.transition(WorkflowState::LeaderSynthesis).unwrap();
        sm.transition(WorkflowState::Completed {
            summary: "Done".into(),
        })
        .unwrap();
        assert!(sm.state.is_terminal());
    }

    #[test]
    fn test_invalid_transition_rejected() {
        let mut sm = WorkflowStateMachine::new("wf_test".into());
        let result = sm.transition(WorkflowState::Aggregating { results: vec![] });
        assert!(result.is_err());
    }

    #[test]
    fn test_hitl_flow() {
        let mut sm = WorkflowStateMachine::new("wf_test".into());
        sm.transition(WorkflowState::Planning {
            leader_id: "l1".into(),
        })
        .unwrap();
        sm.transition(WorkflowState::Dispatching {
            pending_tasks: vec![],
        })
        .unwrap();
        sm.transition(WorkflowState::WorkersRunning {
            active_workers: vec!["w1".into()],
        })
        .unwrap();

        let trigger = HitlTrigger {
            node_id: "hitl_001".into(),
            description: "Approve PoC execution".into(),
            context_snapshot: json!({ "poc_script": "..." }),
        };
        let next_state = WorkflowState::Aggregating { results: vec![] };
        sm.transition(WorkflowState::AwaitingHitl {
            trigger,
            next_state: Box::new(next_state),
        })
        .unwrap();
        assert_eq!(sm.current_state_name(), "AwaitingHitl");

        // Resume after HITL approval
        sm.transition(WorkflowState::WorkersRunning {
            active_workers: vec![],
        })
        .unwrap();
    }

    #[test]
    fn test_abort_from_any_state() {
        let mut sm = WorkflowStateMachine::new("wf_test".into());
        sm.transition(WorkflowState::Planning {
            leader_id: "l1".into(),
        })
        .unwrap();
        sm.transition(WorkflowState::Aborted {
            reason: "User cancelled".into(),
        })
        .unwrap();
        assert!(sm.state.is_terminal());
    }

    #[test]
    fn test_workflow_state_serialization() {
        let state = WorkflowState::WorkersRunning {
            active_workers: vec!["w1".into(), "w2".into()],
        };
        let json = serde_json::to_string(&state).unwrap();
        let decoded: WorkflowState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, decoded);
    }
}
