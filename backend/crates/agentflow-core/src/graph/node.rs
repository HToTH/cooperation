use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type NodeId = String;
pub type ContextPoolId = String;

/// How this agent is invoked — raw LLM API or a native agent product CLI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// Raw LLM API call — direct HTTP to Anthropic/Gemini/OpenAI
    RawLlm,
    /// Claude Code CLI subprocess (`claude --print --output-format json`)
    ClaudeCode,
    /// Google Gemini CLI subprocess (`gemini`)
    GeminiCli,
    /// OpenAI Codex CLI subprocess (`codex`)
    Codex,
}

impl Default for AgentKind {
    fn default() -> Self {
        Self::RawLlm
    }
}

/// Free-form user-defined role label (e.g. "coordinator", "analyst", "reviewer").
/// The special value "human_in_loop" pauses the workflow for manual approval.
pub type AgentRole = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "provider", content = "model")]
pub enum ModelProvider {
    Claude(String),
    Gemini(String),
    OpenAI(String),
}

impl ModelProvider {
    pub fn default_claude() -> Self {
        Self::Claude("claude-opus-4-6".to_string())
    }

    pub fn default_gemini() -> Self {
        Self::Gemini("gemini-2.0-flash".to_string())
    }

    pub fn default_openai() -> Self {
        Self::OpenAI("gpt-4o".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub temperature: f32,
    pub max_tokens: u32,
    pub system_prompt: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            max_tokens: 4096,
            system_prompt: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentNodeState {
    Idle,
    Running,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNode {
    pub id: NodeId,
    pub label: String,
    pub role: AgentRole,
    pub model: ModelProvider,
    /// Selects native agent product vs raw LLM call
    #[serde(default)]
    pub kind: AgentKind,
    pub model_config: ModelConfig,
    pub context_pool_id: ContextPoolId,
    pub state: AgentNodeState,
    pub position: NodePosition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePosition {
    pub x: f32,
    pub y: f32,
}

impl AgentNode {
    pub fn new(label: String, role: impl Into<AgentRole>, model: ModelProvider) -> Self {
        let role = role.into();
        let id = Uuid::new_v4().to_string();
        let context_pool_id = format!("ctx_{}_{}", id, Uuid::new_v4().simple());
        Self {
            id,
            label,
            role,
            model,
            kind: AgentKind::default(),
            model_config: ModelConfig::default(),
            context_pool_id,
            state: AgentNodeState::Idle,
            position: NodePosition { x: 0.0, y: 0.0 },
        }
    }

    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.position = NodePosition { x, y };
        self
    }

    pub fn with_config(mut self, config: ModelConfig) -> Self {
        self.model_config = config;
        self
    }
}
