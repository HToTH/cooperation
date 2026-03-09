use super::edge::DirectedEdge;
use super::node::{AgentNode, NodeId};
use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub type WorkflowId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowGraph {
    pub id: WorkflowId,
    pub name: String,
    pub nodes: HashMap<NodeId, AgentNode>,
    pub edges: Vec<DirectedEdge>,
}

impl WorkflowGraph {
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: AgentNode) -> &AgentNode {
        let id = node.id.clone();
        self.nodes.insert(id.clone(), node);
        self.nodes.get(&id).unwrap()
    }

    pub fn add_edge(&mut self, edge: DirectedEdge) {
        self.edges.push(edge);
    }

    pub fn get_node(&self, id: &str) -> Option<&AgentNode> {
        self.nodes.get(id)
    }

    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut AgentNode> {
        self.nodes.get_mut(id)
    }

    /// Nodes with no incoming edges — execution entry points.
    pub fn get_root_nodes(&self) -> Vec<&AgentNode> {
        let targets: std::collections::HashSet<&str> =
            self.edges.iter().map(|e| e.target.as_str()).collect();
        self.nodes
            .values()
            .filter(|n| !targets.contains(n.id.as_str()))
            .collect()
    }

    /// IDs of nodes that `node_id` has outgoing edges to.
    pub fn get_successor_ids(&self, node_id: &str) -> Vec<NodeId> {
        self.edges
            .iter()
            .filter(|e| e.source == node_id)
            .map(|e| e.target.clone())
            .collect()
    }

    /// IDs of nodes that have outgoing edges pointing to `node_id`.
    pub fn get_predecessor_ids(&self, node_id: &str) -> Vec<NodeId> {
        self.edges
            .iter()
            .filter(|e| e.target == node_id)
            .map(|e| e.source.clone())
            .collect()
    }

    /// Downstream nodes reachable directly from `node_id`.
    pub fn get_downstream_nodes(&self, node_id: &str) -> Vec<&AgentNode> {
        let ids = self.get_successor_ids(node_id);
        self.nodes
            .values()
            .filter(|n| ids.contains(&n.id))
            .collect()
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        if self.nodes.is_empty() {
            return Err(CoreError::InvalidMessage(
                "Workflow must have at least one node".into(),
            ));
        }
        Ok(())
    }
}
