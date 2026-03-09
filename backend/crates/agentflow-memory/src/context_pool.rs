use agentflow_core::graph::node::NodeId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;

const DEFAULT_MAX_TOKENS: usize = 32_000;
const DEFAULT_MAX_TURNS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub role: TurnRole,
    pub content: String,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TurnRole {
    User,
    Assistant,
    System,
}

impl ConversationTurn {
    pub fn user(content: String) -> Self {
        let token_estimate = content.len() / 4;
        Self {
            role: TurnRole::User,
            content,
            token_estimate,
        }
    }

    pub fn assistant(content: String) -> Self {
        let token_estimate = content.len() / 4;
        Self {
            role: TurnRole::Assistant,
            content,
            token_estimate,
        }
    }

    pub fn system(content: String) -> Self {
        let token_estimate = content.len() / 4;
        Self {
            role: TurnRole::System,
            content,
            token_estimate,
        }
    }
}

/// Per-agent isolated context pool. NEVER shared across agents.
pub struct ContextPool {
    pub id: String,
    pub owner_agent_id: NodeId,
    turns: VecDeque<ConversationTurn>,
    system_prompt: Option<String>,
    pub max_tokens: usize,
    pub max_turns: usize,
    current_token_count: usize,
}

impl ContextPool {
    pub fn new(id: String, owner_agent_id: NodeId) -> Self {
        Self {
            id,
            owner_agent_id,
            turns: VecDeque::new(),
            system_prompt: None,
            max_tokens: DEFAULT_MAX_TOKENS,
            max_turns: DEFAULT_MAX_TURNS,
            current_token_count: 0,
        }
    }

    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = Some(prompt);
        self
    }

    pub fn set_system_prompt(&mut self, prompt: String) {
        self.system_prompt = Some(prompt);
    }

    pub fn get_system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    pub fn add_turn(&mut self, turn: ConversationTurn) {
        self.current_token_count += turn.token_estimate;
        self.turns.push_back(turn);
        self.evict_if_needed();
    }

    pub fn get_turns(&self) -> &VecDeque<ConversationTurn> {
        &self.turns
    }

    /// Returns conversation history formatted for LLM API calls.
    /// Does NOT expose raw internal history — only structured turns.
    pub fn get_messages_for_api(&self) -> Vec<serde_json::Value> {
        self.turns
            .iter()
            .map(|t| {
                serde_json::json!({
                    "role": match t.role {
                        TurnRole::User => "user",
                        TurnRole::Assistant => "assistant",
                        TurnRole::System => "system",
                    },
                    "content": t.content,
                })
            })
            .collect()
    }

    /// Extract only specified fields from the last assistant response.
    /// This is what Leaders receive — not the full conversation history.
    pub fn extract_structured(&self, fields: &[&str]) -> Value {
        let last_assistant = self
            .turns
            .iter()
            .rev()
            .find(|t| t.role == TurnRole::Assistant);

        match last_assistant {
            Some(turn) => {
                if let Ok(parsed) = serde_json::from_str::<Value>(&turn.content) {
                    let mut result = serde_json::Map::new();
                    for field in fields {
                        if let Some(val) = parsed.get(*field) {
                            result.insert((*field).to_string(), val.clone());
                        }
                    }
                    Value::Object(result)
                } else {
                    Value::String(turn.content.clone())
                }
            }
            None => Value::Null,
        }
    }

    pub fn token_count(&self) -> usize {
        self.current_token_count
    }

    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }

    pub fn clear(&mut self) {
        self.turns.clear();
        self.current_token_count = 0;
    }

    fn evict_if_needed(&mut self) {
        // Evict oldest turns when over token limit or turn limit
        while (self.current_token_count > self.max_tokens || self.turns.len() > self.max_turns)
            && self.turns.len() > 1
        {
            if let Some(evicted) = self.turns.pop_front() {
                self.current_token_count = self
                    .current_token_count
                    .saturating_sub(evicted.token_estimate);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_pool_add_and_retrieve() {
        let mut pool = ContextPool::new("ctx_test".into(), "agent_001".into());
        pool.add_turn(ConversationTurn::user("Hello".into()));
        pool.add_turn(ConversationTurn::assistant("Hi there".into()));

        assert_eq!(pool.turn_count(), 2);
        let msgs = pool.get_messages_for_api();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["role"], "assistant");
    }

    #[test]
    fn test_context_pool_eviction() {
        let mut pool = ContextPool::new("ctx_test".into(), "agent_001".into());
        pool.max_turns = 3;

        for i in 0..5 {
            pool.add_turn(ConversationTurn::user(format!("Message {}", i)));
        }

        // Should have evicted down to max_turns
        assert!(pool.turn_count() <= 3);
    }

    #[test]
    fn test_extract_structured() {
        let mut pool = ContextPool::new("ctx_test".into(), "agent_001".into());
        pool.add_turn(ConversationTurn::assistant(
            r#"{"discovered_routes": ["/api/v1"], "confidence": 0.9}"#.into(),
        ));

        let extracted = pool.extract_structured(&["discovered_routes"]);
        assert!(extracted.get("discovered_routes").is_some());
        assert!(extracted.get("confidence").is_none());
    }

    #[test]
    fn test_system_prompt() {
        let pool = ContextPool::new("ctx_test".into(), "agent_001".into())
            .with_system_prompt("You are a security analyst.".into());
        assert_eq!(
            pool.get_system_prompt(),
            Some("You are a security analyst.")
        );
    }
}
