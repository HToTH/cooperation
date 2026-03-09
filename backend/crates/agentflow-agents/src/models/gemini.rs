use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{debug, error, info};

use super::{ContentBlock, LlmClient, LlmResponse, LlmTool, RawLlmResponse, StopReason};

pub struct GeminiClient {
    http: Client,
    api_key: String,
    model: String,
}

impl GeminiClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            model,
        }
    }

    fn endpoint(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        )
    }

    fn messages_to_gemini_contents(messages: &[Value]) -> Vec<Value> {
        messages
            .iter()
            .filter_map(|msg| {
                let role = msg.get("role")?.as_str()?;
                // Skip system messages here (handled via systemInstruction)
                if role == "system" {
                    return None;
                }

                let gemini_role = if role == "assistant" { "model" } else { "user" };

                // Handle tool_result messages (from our agentic loop)
                if let Some(content_arr) = msg.get("content").and_then(|c| c.as_array()) {
                    let parts: Vec<Value> = content_arr
                        .iter()
                        .filter_map(|part| match part.get("type").and_then(|t| t.as_str()) {
                            Some("tool_result") => {
                                let func_id = part
                                    .get("tool_use_id")
                                    .and_then(|i| i.as_str())
                                    .unwrap_or("");
                                let content_val = part.get("content");
                                Some(json!({
                                    "functionResponse": {
                                        "name": func_id,
                                        "response": content_val.unwrap_or(&Value::Null)
                                    }
                                }))
                            }
                            _ => None,
                        })
                        .collect();
                    if !parts.is_empty() {
                        return Some(json!({ "role": gemini_role, "parts": parts }));
                    }
                }

                // Handle tool_use content (assistant messages with function calls)
                if let Some(content_arr) = msg.get("content").and_then(|c| c.as_array()) {
                    let parts: Vec<Value> = content_arr
                        .iter()
                        .filter_map(|part| match part.get("type").and_then(|t| t.as_str()) {
                            Some("text") => {
                                Some(json!({"text": part.get("text").unwrap_or(&json!(""))}))
                            }
                            Some("tool_use") => Some(json!({
                                "functionCall": {
                                    "name": part.get("name").unwrap_or(&json!("")),
                                    "args": part.get("input").unwrap_or(&json!({}))
                                }
                            })),
                            _ => None,
                        })
                        .collect();
                    if !parts.is_empty() {
                        return Some(json!({ "role": gemini_role, "parts": parts }));
                    }
                }

                // Simple string content
                let content = msg.get("content")?.as_str()?;
                Some(json!({
                    "role": gemini_role,
                    "parts": [{ "text": content }]
                }))
            })
            .collect()
    }

    fn parse_raw(body: &Value) -> Result<RawLlmResponse> {
        let candidate = body
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| anyhow!("No candidates in Gemini response"))?;

        let finish_reason = candidate
            .get("finishReason")
            .and_then(|r| r.as_str())
            .unwrap_or("STOP");

        let stop_reason = match finish_reason {
            "STOP" => StopReason::EndTurn,
            "MAX_TOKENS" => StopReason::MaxTokens,
            _ => StopReason::Other(finish_reason.to_string()),
        };

        let parts = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
            .ok_or_else(|| anyhow!("No parts in Gemini response"))?;

        let mut content_blocks = Vec::new();
        let mut has_function_call = false;

        for part in parts {
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                content_blocks.push(ContentBlock::Text {
                    text: text.to_string(),
                });
            } else if let Some(fc) = part.get("functionCall") {
                has_function_call = true;
                let name = fc
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = fc.get("args").cloned().unwrap_or(Value::Null);
                // Gemini doesn't provide a call id, generate a stable one
                let id = format!("gemini_call_{}", name);
                content_blocks.push(ContentBlock::ToolUse { id, name, input });
            }
        }

        let stop_reason = if has_function_call {
            StopReason::ToolUse
        } else {
            stop_reason
        };

        let input_tokens = body
            .get("usageMetadata")
            .and_then(|u| u.get("promptTokenCount"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = body
            .get("usageMetadata")
            .and_then(|u| u.get("candidatesTokenCount"))
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
impl LlmClient for GeminiClient {
    async fn complete(
        &self,
        system_prompt: Option<&str>,
        messages: Vec<Value>,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<LlmResponse> {
        let contents = Self::messages_to_gemini_contents(&messages);
        let mut body = json!({
            "contents": contents,
            "generationConfig": { "maxOutputTokens": max_tokens, "temperature": temperature }
        });
        if let Some(system) = system_prompt {
            body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
        }

        debug!("Gemini API call with model {}", self.model);
        let response = self.http.post(self.endpoint()).json(&body).send().await?;
        let status = response.status();
        let resp_body: Value = response.json().await?;
        if !status.is_success() {
            let err = resp_body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            error!("Gemini API {}: {}", status, err);
            return Err(anyhow!("Gemini API {}: {}", status, err));
        }

        let raw = Self::parse_raw(&resp_body)?;
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
        let contents = Self::messages_to_gemini_contents(&messages);

        // Gemini uses functionDeclarations
        let function_declarations: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                })
            })
            .collect();

        let mut body = json!({
            "contents": contents,
            "tools": [{ "functionDeclarations": function_declarations }],
            "generationConfig": { "maxOutputTokens": max_tokens, "temperature": temperature }
        });
        if let Some(system) = system_prompt {
            body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
        }

        info!("Gemini agentic call: {} tools", tools.len());
        let response = self.http.post(self.endpoint()).json(&body).send().await?;
        let status = response.status();
        let resp_body: Value = response.json().await?;
        if !status.is_success() {
            let err = resp_body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            error!("Gemini API {}: {}", status, err);
            return Err(anyhow!("Gemini API {}: {}", status, err));
        }

        Self::parse_raw(&resp_body)
    }
}
