//! Claude Code CLI runner.
//!
//! Invokes the `claude` CLI as a subprocess. Claude Code is a fully autonomous
//! agent — it has native access to Bash, file system, web search, and more.
//! We don't implement those tools; Claude Code executes them itself.
//!
//! Prerequisites:
//!   - `claude` CLI installed: https://claude.ai/claude-code
//!   - Authenticated: `claude auth login`

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{debug, info};

use super::{detect_authorization_required, NativeAgentResult, NativeAuthorizationRequest};

pub struct ClaudeCodeRunner {
    /// Path to the `claude` binary (defaults to searching PATH)
    pub binary: String,
    /// Maximum conversation turns Claude Code may take
    pub max_turns: u32,
}

impl Default for ClaudeCodeRunner {
    fn default() -> Self {
        Self {
            binary: "claude".to_string(),
            max_turns: 20,
        }
    }
}

impl ClaudeCodeRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_binary(mut self, path: String) -> Self {
        self.binary = path;
        self
    }

    /// Run Claude Code with a task prompt.
    ///
    /// Uses `--print` (non-interactive) + `--output-format json` so we get
    /// a machine-readable result back. The agent autonomously decides which
    /// of its native tools to use (Bash, Read, Write, WebSearch, etc.).
    pub async fn run(
        &self,
        system_prompt: Option<&str>,
        task: &str,
        allowed_tools: Option<&[&str]>,
    ) -> Result<NativeAgentResult> {
        self.run_with_session(system_prompt, task, allowed_tools, None)
            .await
    }

    pub async fn run_with_session(
        &self,
        system_prompt: Option<&str>,
        task: &str,
        allowed_tools: Option<&[&str]>,
        session_handle: Option<&str>,
    ) -> Result<NativeAgentResult> {
        info!(
            "ClaudeCode runner: starting agent for task ({} chars)",
            task.len()
        );

        let mut args = vec![
            "--print".to_string(),
            "--verbose".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--max-turns".to_string(),
            self.max_turns.to_string(),
        ];
        append_claude_bypass_permission_args(&mut args);

        if let Some(handle) = session_handle {
            args.push("--resume".to_string());
            args.push(handle.to_string());
        }

        if let Some(tools) = allowed_tools {
            // Restrict which tools the agent can use, e.g. ["Bash", "Read", "WebSearch"]
            args.push("--allowedTools".to_string());
            args.push(tools.join(","));
        }

        if session_handle.is_none() {
            if let Some(sys) = system_prompt {
                args.push("--system-prompt".to_string());
                args.push(sys.to_string());
            }
        }

        // The task goes as the final positional argument
        args.push(task.to_string());

        debug!("Spawning: {} {}", self.binary, args.join(" "));

        let output = Command::new(&self.binary)
            .args(&args)
            .env("TERM", "xterm-256color")
            .env_remove("CLAUDECODE")
            .env_remove("CLAUDE_CODE_ENTRYPOINT")
            .env_remove("CLAUDE_CODE_SESSION")
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    anyhow!(
                        "Claude Code CLI not found. Install it: https://claude.ai/claude-code\n\
                         Then authenticate: claude auth login\nOriginal error: {}",
                        e
                    )
                } else {
                    anyhow!("Failed to spawn claude CLI: {}", e)
                }
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!("Claude Code raw output: {} bytes", stdout.len());

        let mut parsed = parse_claude_code_output(&stdout, session_handle)?;
        if !output.status.success() && parsed.authorization_required.is_none() {
            parsed.authorization_required =
                detect_authorization_required(&format!("{}\n{}", stdout, stderr));
        }

        if !output.status.success() {
            if parsed.authorization_required.is_some() {
                return Ok(parsed);
            }
            return Err(anyhow!(
                "claude CLI exited with {}: {}",
                output.status,
                stderr.trim()
            ));
        }

        Ok(parsed)
    }

    /// Interactive mode: send task via stdin, stream output.
    /// Used when you need to feed follow-up messages (e.g. for HITL scenarios).
    pub async fn run_interactive(
        &self,
        system_prompt: Option<&str>,
        initial_task: &str,
    ) -> Result<NativeAgentResult> {
        let mut args = vec![
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--max-turns".to_string(),
            self.max_turns.to_string(),
        ];
        append_claude_bypass_permission_args(&mut args);

        if let Some(sys) = system_prompt {
            args.push("--system-prompt".to_string());
            args.push(sys.to_string());
        }

        let mut child = Command::new(&self.binary)
            .args(&args)
            .env_remove("CLAUDECODE")
            .env_remove("CLAUDE_CODE_ENTRYPOINT")
            .env_remove("CLAUDE_CODE_SESSION")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn claude CLI: {}", e))?;

        // Write initial task
        if let Some(stdin) = child.stdin.take() {
            let mut stdin = stdin;
            stdin.write_all(initial_task.as_bytes()).await?;
        }

        let output = child.wait_with_output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_claude_code_stream(&stdout)
    }
}

/// Parse `--output-format json` output from Claude Code.
///
/// Claude Code JSON output is a newline-delimited stream of events.
/// The final `result` event contains the assistant's response.
fn parse_claude_code_output(
    raw: &str,
    fallback_session: Option<&str>,
) -> Result<NativeAgentResult> {
    let mut final_text = String::new();
    let mut tool_calls = 0u32;
    let mut tokens_used = 0u32;
    let mut session_handle = fallback_session.map(str::to_string);
    let mut authorization_required: Option<NativeAuthorizationRequest> = None;

    // Each line is a JSON object (NDJSON format)
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let Ok(obj) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if let Some(handle) = obj.get("session_id").and_then(|s| s.as_str()) {
            session_handle = Some(handle.to_string());
        }

        match obj.get("type").and_then(|t| t.as_str()) {
            Some("result") => {
                if let Some(text) = obj.get("result").and_then(|r| r.as_str()) {
                    final_text = text.to_string();
                }
                if let Some(cost) = obj.get("usage") {
                    tokens_used = cost
                        .get("input_tokens")
                        .and_then(|t| t.as_u64())
                        .unwrap_or(0) as u32
                        + cost
                            .get("output_tokens")
                            .and_then(|t| t.as_u64())
                            .unwrap_or(0) as u32;
                }
                if let Some(denials) = obj.get("permission_denials") {
                    let has_denials = denials
                        .as_array()
                        .map(|items| !items.is_empty())
                        .unwrap_or(false);
                    if has_denials {
                        authorization_required = Some(NativeAuthorizationRequest {
                            message: "Claude Code requires authorization before it can continue."
                                .to_string(),
                            details: Some(denials.to_string()),
                        });
                    }
                }
            }
            Some("tool_use") | Some("bash") | Some("read") | Some("write") => {
                tool_calls += 1;
            }
            Some("assistant") => {
                // Fallback: grab text from assistant messages if no result block
                if final_text.is_empty() {
                    if let Some(content) = obj
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        final_text = content.to_string();
                    }
                }
            }
            _ => {}
        }
    }

    // If no structured JSON events, treat entire output as plain text result
    if final_text.is_empty() {
        final_text = raw.to_string();
    }

    info!(
        "ClaudeCode completed: {} tool calls, {} tokens",
        tool_calls, tokens_used
    );

    let structured =
        serde_json::from_str(&final_text).unwrap_or_else(|_| json!({ "result": final_text }));

    Ok(NativeAgentResult {
        output: final_text,
        structured,
        tool_calls,
        tokens_used,
        session_handle,
        authorization_required,
    })
}

/// Parse stream-json output (newline-delimited events from interactive mode).
fn parse_claude_code_stream(raw: &str) -> Result<NativeAgentResult> {
    // Stream format is the same NDJSON structure
    parse_claude_code_output(raw, None)
}

fn append_claude_bypass_permission_args(args: &mut Vec<String>) {
    let has_permission_override = args.iter().any(|arg| {
        arg == "--permission-mode"
            || arg == "--dangerously-skip-permissions"
            || arg == "--allow-dangerously-skip-permissions"
    });
    if has_permission_override {
        return;
    }

    args.push("--permission-mode".to_string());
    args.push("bypassPermissions".to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ndjson_result() {
        let output = r#"
{"type":"system","subtype":"init"}
{"type":"tool_use","name":"Bash","input":{"command":"ls"}}
{"type":"tool_result","content":"file1.txt\nfile2.txt"}
{"type":"result","result":"{\"files\":[\"file1.txt\",\"file2.txt\"]}","usage":{"input_tokens":100,"output_tokens":50}}
"#;
        let result = parse_claude_code_output(output, None).unwrap();
        assert_eq!(result.tool_calls, 1);
        assert_eq!(result.tokens_used, 150);
        assert!(result.structured.get("files").is_some());
    }

    #[test]
    fn test_parse_plain_text_fallback() {
        let output = "This is a plain text response with no JSON structure.";
        let result = parse_claude_code_output(output, None).unwrap();
        assert_eq!(result.output, output);
        assert!(result.structured.get("result").is_some());
    }

    #[test]
    fn test_append_claude_bypass_permission_args() {
        let mut args = vec!["--print".to_string()];
        append_claude_bypass_permission_args(&mut args);

        assert!(args
            .windows(2)
            .any(|pair| { pair[0] == "--permission-mode" && pair[1] == "bypassPermissions" }));
    }
}
