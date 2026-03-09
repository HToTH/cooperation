pub mod claude;
pub mod gemini;
pub mod openai;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

pub use claude::ClaudeClient;
pub use gemini::GeminiClient;
pub use openai::OpenAIClient;

// ─── Tool definition (passed to LLM) ────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LlmTool {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's input
    pub input_schema: Value,
}

// ─── Raw response types (support tool_use blocks) ───────────────────────────

#[derive(Debug, Clone)]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

impl ContentBlock {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        }
    }

    pub fn is_tool_use(&self) -> bool {
        matches!(self, ContentBlock::ToolUse { .. })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other(String),
}

impl StopReason {
    pub fn is_done(&self) -> bool {
        matches!(self, StopReason::EndTurn | StopReason::MaxTokens)
    }
}

#[derive(Debug, Clone)]
pub struct RawLlmResponse {
    pub content: Vec<ContentBlock>,
    pub stop_reason: StopReason,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl RawLlmResponse {
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn tool_uses(&self) -> Vec<&ContentBlock> {
        self.content.iter().filter(|b| b.is_tool_use()).collect()
    }
}

// ─── Simple response (for single-turn workers without tools) ─────────────────

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

// ─── Unified client trait ────────────────────────────────────────────────────

#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Single-turn completion (no tool support, for simple workers)
    async fn complete(
        &self,
        system_prompt: Option<&str>,
        messages: Vec<Value>,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<LlmResponse>;

    /// Multi-turn with tool support — returns raw content blocks
    async fn complete_with_tools(
        &self,
        system_prompt: Option<&str>,
        messages: Vec<Value>,
        max_tokens: u32,
        temperature: f32,
        tools: &[LlmTool],
    ) -> Result<RawLlmResponse>;
}
