use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Invalid state transition from {from} to {to}")]
    InvalidStateTransition { from: String, to: String },

    #[error("Agent node not found: {0}")]
    AgentNotFound(String),

    #[error("Workflow not found: {0}")]
    WorkflowNotFound(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Context pool not found: {0}")]
    ContextPoolNotFound(String),

    #[error("HITL decision channel closed")]
    HitlChannelClosed,

    #[error("Invalid message format: {0}")]
    InvalidMessage(String),
}
