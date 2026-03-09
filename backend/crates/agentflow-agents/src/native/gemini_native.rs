//! Gemini Native Agent runner.
//!
//! Uses the Gemini API with built-in tools:
//!   - `code_execution`: Google executes Python code on their servers
//!   - `google_search`: Real-time web search grounding
//!
//! Neither tool requires us to implement anything — Google's servers handle
//! the execution. We just enable them in the API request.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{debug, info};

use super::NativeAgentResult;

/// Which of Gemini's built-in tools to enable
#[derive(Debug, Clone)]
pub struct GeminiNativeTools {
    pub code_execution: bool,
    pub google_search: bool,
}

impl Default for GeminiNativeTools {
    fn default() -> Self {
        Self { code_execution: true, google_search: true }
    }
}

pub struct GeminiNativeRunner {
    http: Client,
    api_key: String,
    model: String,
    pub tools: GeminiNativeTools,
}

impl GeminiNativeRunner {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            model,
            tools: GeminiNativeTools::default(),
        }
    }

    pub fn with_tools(mut self, tools: GeminiNativeTools) -> Self {
        self.tools = tools;
        self
    }

    /// Run the agent. Gemini will autonomously use code_execution and
    /// google_search as needed — all execution happens on Google's servers.
    pub async fn run(
        &self,
        system_prompt: Option<&str>,
        task: &str,
        extra_function_tools: Option<Vec<Value>>,
    ) -> Result<NativeAgentResult> {
        info!("GeminiNative runner: task ({} chars)", task.len());

        // Build the tools array with Gemini's native built-in tools
        let mut tools_arr: Vec<Value> = Vec::new();

        if self.tools.google_search {
            // google_search is a native Gemini tool — no schema needed
            tools_arr.push(json!({ "google_search": {} }));
        }

        if self.tools.code_execution {
            // code_execution: Gemini runs Python on Google's servers
            tools_arr.push(json!({ "code_execution": {} }));
        }

        // Append any custom function tools (e.g. inter-agent call_worker tools)
        if let Some(funcs) = extra_function_tools {
            if !funcs.is_empty() {
                tools_arr.push(json!({ "function_declarations": funcs }));
            }
        }

        let mut body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": task }]
            }],
            "tools": tools_arr,
            "generationConfig": {
                "maxOutputTokens": 8192,
                "temperature": 0.7,
            }
        });

        if let Some(sys) = system_prompt {
            body["systemInstruction"] = json!({
                "parts": [{ "text": sys }]
            });
        }

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        debug!("Calling Gemini native API with {} tools", tools_arr.len());

        let response = self.http.post(&url).json(&body).send().await?;
        let status = response.status();
        let resp: Value = response.json().await?;

        if !status.is_success() {
            let err = resp.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(anyhow!("Gemini API {}: {}", status, err));
        }

        parse_gemini_native_response(&resp)
    }
}

/// Parse Gemini's response, including code execution results and search grounding.
fn parse_gemini_native_response(resp: &Value) -> Result<NativeAgentResult> {
    let candidate = resp.get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| anyhow!("No candidates in Gemini response"))?;

    let parts = candidate
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .ok_or_else(|| anyhow!("No parts in Gemini response"))?;

    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls = 0u32;
    let mut code_results: Vec<Value> = Vec::new();

    for part in parts {
        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
            text_parts.push(text.to_string());
        }

        // code_execution result — Google ran the code, this is the output
        if let Some(exec_result) = part.get("executableCode") {
            tool_calls += 1;
            let lang = exec_result.get("language").and_then(|l| l.as_str()).unwrap_or("python");
            let code = exec_result.get("code").and_then(|c| c.as_str()).unwrap_or("");
            info!("Gemini executed {} code ({} chars)", lang, code.len());
        }

        if let Some(code_result) = part.get("codeExecutionResult") {
            let outcome = code_result.get("outcome").and_then(|o| o.as_str()).unwrap_or("");
            let output = code_result.get("output").and_then(|o| o.as_str()).unwrap_or("");
            code_results.push(json!({ "outcome": outcome, "output": output }));
            text_parts.push(format!("\n```\n{}\n```", output));
        }

        // google_search grounding result
        if part.get("searchResultMetadata").is_some() {
            tool_calls += 1;
        }

        // function calls (custom tools)
        if part.get("functionCall").is_some() {
            tool_calls += 1;
        }
    }

    let output = text_parts.join("\n");

    // Also check grounding metadata for search tool usage
    if let Some(meta) = candidate.get("groundingMetadata") {
        if meta.get("searchEntryPoint").is_some() {
            tool_calls += 1;
        }
    }

    let tokens_used = resp.get("usageMetadata")
        .map(|u| {
            u.get("promptTokenCount").and_then(|t| t.as_u64()).unwrap_or(0)
                + u.get("candidatesTokenCount").and_then(|t| t.as_u64()).unwrap_or(0)
        })
        .unwrap_or(0) as u32;

    info!("GeminiNative completed: {} tool invocations, {} tokens", tool_calls, tokens_used);

    let structured = if !code_results.is_empty() {
        json!({ "text": output, "code_execution_results": code_results })
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
    fn test_parse_text_only_response() {
        let resp = json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "The answer is 42." }]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5
            }
        });
        let result = parse_gemini_native_response(&resp).unwrap();
        assert_eq!(result.output, "The answer is 42.");
        assert_eq!(result.tokens_used, 15);
        assert_eq!(result.tool_calls, 0);
    }

    #[test]
    fn test_parse_code_execution_response() {
        let resp = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "Let me calculate that:" },
                        { "executableCode": { "language": "PYTHON", "code": "print(6*7)" } },
                        { "codeExecutionResult": { "outcome": "OUTCOME_OK", "output": "42\n" } }
                    ]
                }
            }],
            "usageMetadata": { "promptTokenCount": 50, "candidatesTokenCount": 30 }
        });
        let result = parse_gemini_native_response(&resp).unwrap();
        assert_eq!(result.tool_calls, 1);
        assert_eq!(result.tokens_used, 80);
        assert!(result.output.contains("42"));
        assert!(result.structured.get("code_execution_results").is_some());
    }
}
