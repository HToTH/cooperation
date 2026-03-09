//! OpenAI Responses API runner.
//!
//! Uses the `/v1/responses` endpoint (not Chat Completions) which provides
//! built-in hosted tools:
//!   - `web_search_preview`: real-time web search (OpenAI handles it)
//!   - `code_interpreter`: code execution in a sandboxed environment
//!   - `computer_use_preview`: computer control (requires special model)
//!
//! These tools execute on OpenAI's servers — we don't implement them.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{debug, info};

use super::NativeAgentResult;

#[derive(Debug, Clone, Default)]
pub struct OpenAIBuiltinTools {
    pub web_search: bool,
    pub code_interpreter: bool,
    pub computer_use: bool,
}

pub struct OpenAIResponsesRunner {
    http: Client,
    api_key: String,
    model: String,
    pub tools: OpenAIBuiltinTools,
}

impl OpenAIResponsesRunner {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            model,
            tools: OpenAIBuiltinTools {
                web_search: true,
                code_interpreter: true,
                computer_use: false,
            },
        }
    }

    pub fn with_tools(mut self, tools: OpenAIBuiltinTools) -> Self {
        self.tools = tools;
        self
    }

    /// Run an agent turn using the Responses API.
    ///
    /// OpenAI executes the built-in tools (search, code) server-side.
    /// We provide the task; OpenAI handles tool execution transparently.
    pub async fn run(
        &self,
        system_prompt: Option<&str>,
        task: &str,
        extra_function_tools: Option<Vec<Value>>,
    ) -> Result<NativeAgentResult> {
        info!("OpenAIResponses runner: task ({} chars)", task.len());

        let mut tools: Vec<Value> = Vec::new();

        if self.tools.web_search {
            // web_search_preview: OpenAI searches the web for you
            tools.push(json!({ "type": "web_search_preview" }));
        }

        if self.tools.code_interpreter {
            // code_interpreter: runs code in a sandboxed environment on OpenAI's servers
            tools.push(json!({ "type": "code_interpreter" }));
        }

        if self.tools.computer_use {
            // Requires specific model (computer-use-preview)
            tools.push(json!({ "type": "computer_use_preview" }));
        }

        // Append custom function tools for inter-agent communication
        if let Some(funcs) = extra_function_tools {
            for func in funcs {
                tools.push(json!({
                    "type": "function",
                    "function": func
                }));
            }
        }

        // Build input: system + user messages
        let mut input: Vec<Value> = Vec::new();
        if let Some(sys) = system_prompt {
            input.push(json!({ "role": "system", "content": sys }));
        }
        input.push(json!({ "role": "user", "content": task }));

        let body = json!({
            "model": self.model,
            "input": input,
            "tools": tools,
        });

        debug!("OpenAI Responses API: model={}, {} tools", self.model, tools.len());

        let response = self.http
            .post("https://api.openai.com/v1/responses")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let resp: Value = response.json().await?;

        if !status.is_success() {
            let err = resp.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(anyhow!("OpenAI Responses API {}: {}", status, err));
        }

        parse_responses_output(&resp)
    }
}

/// Parse the OpenAI Responses API output.
///
/// The output is a list of items; we extract text, tool calls, and code results.
fn parse_responses_output(resp: &Value) -> Result<NativeAgentResult> {
    let output_items = resp.get("output")
        .and_then(|o| o.as_array())
        .ok_or_else(|| anyhow!("No output in Responses API response"))?;

    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls = 0u32;
    let mut code_outputs: Vec<Value> = Vec::new();
    let mut search_results: Vec<Value> = Vec::new();

    for item in output_items {
        match item.get("type").and_then(|t| t.as_str()) {
            Some("message") => {
                if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                    for block in content {
                        match block.get("type").and_then(|t| t.as_str()) {
                            Some("output_text") => {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    text_parts.push(text.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            // web_search_preview result — OpenAI searched and found results
            Some("web_search_call") => {
                tool_calls += 1;
                if let Some(results) = item.get("results") {
                    search_results.push(results.clone());
                    info!("OpenAI web search completed");
                }
            }

            // code_interpreter — OpenAI ran the code
            Some("code_interpreter_call") => {
                tool_calls += 1;
                let outputs = item.get("outputs").and_then(|o| o.as_array());
                if let Some(outputs) = outputs {
                    for out in outputs {
                        match out.get("type").and_then(|t| t.as_str()) {
                            Some("logs") => {
                                let logs = out.get("logs").and_then(|l| l.as_str()).unwrap_or("");
                                code_outputs.push(json!({ "type": "logs", "content": logs }));
                                text_parts.push(format!("\n```\n{}\n```", logs));
                                info!("OpenAI code_interpreter executed code");
                            }
                            Some("image") => {
                                code_outputs.push(json!({ "type": "image" }));
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Custom function call (inter-agent worker call)
            Some("function_call") => {
                tool_calls += 1;
            }

            _ => {}
        }
    }

    let output = text_parts.join("\n");

    let tokens_used = resp.get("usage")
        .map(|u| {
            u.get("input_tokens").and_then(|t| t.as_u64()).unwrap_or(0)
                + u.get("output_tokens").and_then(|t| t.as_u64()).unwrap_or(0)
        })
        .unwrap_or(0) as u32;

    info!("OpenAIResponses completed: {} tool calls, {} tokens", tool_calls, tokens_used);

    let structured = if !code_outputs.is_empty() || !search_results.is_empty() {
        json!({
            "text": output,
            "code_outputs": code_outputs,
            "search_results": search_results,
        })
    } else {
        serde_json::from_str(&output).unwrap_or_else(|_| json!({ "result": output }))
    };

    Ok(NativeAgentResult {
        output,
        structured,
        tool_calls,
        tokens_used,
        session_handle: None,
        authorization_required: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_message() {
        let resp = json!({
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "text": "The result is 42."
                }]
            }],
            "usage": { "input_tokens": 10, "output_tokens": 8 }
        });
        let result = parse_responses_output(&resp).unwrap();
        assert_eq!(result.output, "The result is 42.");
        assert_eq!(result.tokens_used, 18);
        assert_eq!(result.tool_calls, 0);
    }

    #[test]
    fn test_parse_code_interpreter_result() {
        let resp = json!({
            "output": [
                {
                    "type": "code_interpreter_call",
                    "outputs": [{ "type": "logs", "logs": "42\n" }]
                },
                {
                    "type": "message",
                    "content": [{ "type": "output_text", "text": "The answer is 42." }]
                }
            ],
            "usage": { "input_tokens": 50, "output_tokens": 20 }
        });
        let result = parse_responses_output(&resp).unwrap();
        assert_eq!(result.tool_calls, 1);
        assert_eq!(result.tokens_used, 70);
        assert!(result.structured.get("code_outputs").is_some());
    }

    #[test]
    fn test_parse_web_search_result() {
        let resp = json!({
            "output": [
                { "type": "web_search_call", "results": [{"url": "https://example.com"}] },
                {
                    "type": "message",
                    "content": [{ "type": "output_text", "text": "Based on search..." }]
                }
            ],
            "usage": { "input_tokens": 30, "output_tokens": 15 }
        });
        let result = parse_responses_output(&resp).unwrap();
        assert_eq!(result.tool_calls, 1);
        assert!(result.structured.get("search_results").is_some());
    }
}
