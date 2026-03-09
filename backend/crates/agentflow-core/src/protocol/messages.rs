use crate::graph::node::NodeId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: NodeId,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    TaskDispatch,
    TaskResult,
    StatusUpdate,
    HitlTrigger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextObject(pub Value);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedFormat {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub properties: Option<Value>,
    pub required: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDispatchPayload {
    pub task_intent: String,
    pub context: ContextObject,
    pub expected_format: ExpectedFormat,
    pub context_pool_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Completed,
    Failed,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionMetadata {
    pub tokens_used: u32,
    pub context_pool_id: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskResultPayload {
    pub status: TaskStatus,
    pub result: Value,
    pub error: Option<String>,
    pub execution_metadata: ExecutionMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub protocol_version: String,
    pub message_id: String,
    pub from_agent: AgentIdentity,
    pub to_agent: AgentIdentity,
    pub message_type: MessageType,
    pub payload: Value,
    pub in_reply_to: Option<String>,
}

impl AgentMessage {
    pub fn new_dispatch(
        from: AgentIdentity,
        to: AgentIdentity,
        payload: TaskDispatchPayload,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            protocol_version: "1.0".into(),
            message_id: Uuid::new_v4().to_string(),
            from_agent: from,
            to_agent: to,
            message_type: MessageType::TaskDispatch,
            payload: serde_json::to_value(payload)?,
            in_reply_to: None,
        })
    }

    pub fn new_result(
        from: AgentIdentity,
        to: AgentIdentity,
        in_reply_to: String,
        payload: TaskResultPayload,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            protocol_version: "1.0".into(),
            message_id: Uuid::new_v4().to_string(),
            from_agent: from,
            to_agent: to,
            message_type: MessageType::TaskResult,
            payload: serde_json::to_value(payload)?,
            in_reply_to: Some(in_reply_to),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_task_dispatch_roundtrip() {
        let payload = TaskDispatchPayload {
            task_intent: "Analyze JS bundle for API routes".into(),
            context: ContextObject(json!({ "target_url": "https://example.com" })),
            expected_format: ExpectedFormat {
                schema_type: "object".into(),
                properties: Some(json!({
                    "discovered_routes": { "type": "array" }
                })),
                required: Some(vec!["discovered_routes".into()]),
            },
            context_pool_id: "ctx_worker_001".into(),
        };

        let json = serde_json::to_string(&payload).unwrap();
        let decoded: TaskDispatchPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.task_intent, payload.task_intent);
    }

    #[test]
    fn test_agent_message_dispatch_roundtrip() {
        let from = AgentIdentity {
            id: "leader_001".into(),
            role: "leader".into(),
        };
        let to = AgentIdentity {
            id: "worker_002".into(),
            role: "worker".into(),
        };
        let payload = TaskDispatchPayload {
            task_intent: "Test task".into(),
            context: ContextObject(json!({})),
            expected_format: ExpectedFormat {
                schema_type: "object".into(),
                properties: None,
                required: None,
            },
            context_pool_id: "ctx_test".into(),
        };

        let msg = AgentMessage::new_dispatch(from, to, payload).unwrap();
        assert_eq!(msg.protocol_version, "1.0");
        assert_eq!(msg.message_type, MessageType::TaskDispatch);

        let json = serde_json::to_string(&msg).unwrap();
        let decoded: AgentMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.message_id, msg.message_id);
    }

    #[test]
    fn test_task_result_roundtrip() {
        let payload = TaskResultPayload {
            status: TaskStatus::Completed,
            result: json!({
                "discovered_routes": ["/api/v1/users"],
                "confidence_scores": { "/api/v1/users": 0.9 }
            }),
            error: None,
            execution_metadata: ExecutionMetadata {
                tokens_used: 1234,
                context_pool_id: "ctx_worker_001".into(),
                duration_ms: 850,
            },
        };

        let json = serde_json::to_string(&payload).unwrap();
        let decoded: TaskResultPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.status, TaskStatus::Completed);
    }
}
