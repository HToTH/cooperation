use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{debug, error, info};

use super::{ContentBlock, LlmClient, LlmResponse, LlmTool, RawLlmResponse, StopReason};

pub struct OpenAIClient {
    http: Client,
    api_key: String,
    model: String,
}

impl OpenAIClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            model,
        }
    }

    async fn post_chat(&self, body: &Value) -> Result<Value> {
        debug!("OpenAI API call with model {}", self.model);

        let response = self
            .http
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await?;

        let status = response.status();
        let body: Value = response.json().await?;

        if !status.is_success() {
            let err = body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            error!("OpenAI API {}: {}", status, err);
            return Err(anyhow!("OpenAI API {}: {}", status, err));
        }
        Ok(body)
    }

    fn parse_raw(body: &Value) -> Result<RawLlmResponse> {
        let choice = body
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| anyhow!("No choices in OpenAI response"))?;

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|r| r.as_str())
            .unwrap_or("stop");

        let stop_reason = match finish_reason {
            "stop" => StopReason::EndTurn,
            "tool_calls" => StopReason::ToolUse,
            "length" => StopReason::MaxTokens,
            other => StopReason::Other(other.to_string()),
        };

        let message = choice
            .get("message")
            .ok_or_else(|| anyhow!("No message in choice"))?;

        let mut content_blocks = Vec::new();

        // Text content
        if let Some(text) = message.get("content").and_then(|c| c.as_str()) {
            if !text.is_empty() {
                content_blocks.push(ContentBlock::Text {
                    text: text.to_string(),
                });
            }
        }

        // Tool calls (OpenAI format)
        if let Some(calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
            for call in calls {
                let id = call
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = call
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let args_str = call
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|a| a.as_str())
                    .unwrap_or("{}");
                let input: Value = serde_json::from_str(args_str).unwrap_or(Value::Null);
                content_blocks.push(ContentBlock::ToolUse { id, name, input });
            }
        }

        let input_tokens = body
            .get("usage")
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = body
            .get("usage")
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u32;

        Ok(RawLlmResponse {
            content: content_blocks,
            stop_reason,
            input_tokens,
            output_tokens,
        })
    }
}

#[async_trait]
impl LlmClient for OpenAIClient {
    async fn complete(
        &self,
        system_prompt: Option<&str>,
        messages: Vec<Value>,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<LlmResponse> {
        let mut all_messages: Vec<Value> = Vec::new();
        if let Some(system) = system_prompt {
            all_messages.push(json!({ "role": "system", "content": system }));
        }
        all_messages.extend(messages);

        let body = json!({
            "model": self.model,
            "messages": all_messages,
            "max_tokens": max_tokens,
            "temperature": temperature,
        });

        let resp = self.post_chat(&body).await?;
        let raw = Self::parse_raw(&resp)?;
        Ok(LlmResponse {
            content: raw.text_content(),
            input_tokens: raw.input_tokens,
            output_tokens: raw.output_tokens,
        })
    }

    async fn complete_with_tools(
        &self,
        system_prompt: Option<&str>,
        messages: Vec<Value>,
        max_tokens: u32,
        temperature: f32,
        tools: &[LlmTool],
    ) -> Result<RawLlmResponse> {
        let mut all_messages: Vec<Value> = Vec::new();
        if let Some(system) = system_prompt {
            all_messages.push(json!({ "role": "system", "content": system }));
        }
        all_messages.extend(messages);

        // OpenAI uses "functions" style tool definitions
        let tool_defs: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect();

        let body = json!({
            "model": self.model,
            "messages": all_messages,
            "max_tokens": max_tokens,
            "temperature": temperature,
            "tools": tool_defs,
            "tool_choice": "auto",
        });

        info!(
            "OpenAI agentic call: {} messages, {} tools",
            all_messages.len(),
            tools.len()
        );
        let resp = self.post_chat(&body).await?;
        Self::parse_raw(&resp)
    }
}
