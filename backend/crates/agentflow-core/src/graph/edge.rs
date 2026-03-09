use super::node::NodeId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type EdgeId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectedEdge {
    pub id: EdgeId,
    pub source: NodeId,
    pub target: NodeId,
    pub label: Option<String>,
}

impl DirectedEdge {
    pub fn new(source: NodeId, target: NodeId) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            source,
            target,
            label: None,
        }
    }

    pub fn with_label(mut self, label: String) -> Self {
        self.label = Some(label);
        self
    }
}
