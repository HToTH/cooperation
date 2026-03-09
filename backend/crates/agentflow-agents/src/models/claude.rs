use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{debug, error, info};

use super::{ContentBlock, LlmClient, LlmResponse, LlmTool, RawLlmResponse, StopReason};

pub struct ClaudeClient {
    http: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl ClaudeClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            base_url: "https://api.anthropic.com".to_string(),
            model,
        }
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    async fn post_messages(&self, body: &Value) -> Result<Value> {
        debug!("Claude API request to model {}", self.model);

        let response = self
            .http
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
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
                .unwrap_or("Unknown API error");
            error!("Claude API {} error: {}", status, err);
            return Err(anyhow!("Claude API {}: {}", status, err));
        }

        Ok(body)
    }

    fn parse_raw_response(body: &Value) -> Result<RawLlmResponse> {
        let stop_reason = match body.get("stop_reason").and_then(|r| r.as_str()) {
            Some("end_turn") => StopReason::EndTurn,
            Some("tool_use") => StopReason::ToolUse,
            Some("max_tokens") => StopReason::MaxTokens,
            Some(other) => StopReason::Other(other.to_string()),
            None => StopReason::EndTurn,
        };

        let content = body
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| anyhow!("Missing content array in Claude response"))?
            .iter()
            .map(|block| match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    Ok(ContentBlock::Text {
                        text: text.to_string(),
                    })
                }
                Some("tool_use") => {
                    let id = block
                        .get("id")
                        .and_then(|i| i.as_str())
                        .ok_or_else(|| anyhow!("tool_use block missing id"))?
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|n| n.as_str())
                        .ok_or_else(|| anyhow!("tool_use block missing name"))?
                        .to_string();
                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                    Ok(ContentBlock::ToolUse { id, name, input })
                }
                other => Err(anyhow!("Unknown content block type: {:?}", other)),
            })
            .collect::<Result<Vec<_>>>()?;

        let input_tokens = body
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = body
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u32;

        Ok(RawLlmResponse {
            content,
            stop_reason,
            input_tokens,
            output_tokens,
        })
    }
}

#[async_trait]
impl LlmClient for ClaudeClient {
    async fn complete(
        &self,
        system_prompt: Option<&str>,
        messages: Vec<Value>,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<LlmResponse> {
        let mut body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "temperature": temperature,
            "messages": messages,
        });
        if let Some(system) = system_prompt {
            body["system"] = json!(system);
        }

        let resp = self.post_messages(&body).await?;
        let raw = Self::parse_raw_response(&resp)?;
        let content = raw.text_content();

        Ok(LlmResponse {
            content,
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
        let tool_defs: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();

        let mut body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "temperature": temperature,
            "messages": messages,
            "tools": tool_defs,
        });
        if let Some(system) = system_prompt {
            body["system"] = json!(system);
        }

        info!(
            "Claude agentic call: {} messages, {} tools",
            messages.len(),
            tools.len()
        );
        let resp = self.post_messages(&body).await?;
        Self::parse_raw_response(&resp)
    }
}
