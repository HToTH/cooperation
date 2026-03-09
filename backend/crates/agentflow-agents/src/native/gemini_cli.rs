use anyhow::{Context, Result};
use serde_json::{json, Value};
use tracing::info;

use super::{
    detect_authorization_required, is_windows_command_not_found, native_cli_command,
    NativeAgentResult,
};

/// Runs Google Gemini CLI as a subprocess.
/// Requires `gemini` CLI installed and authenticated (`gemini auth login`).
pub struct GeminiCliRunner {
    model: Option<String>,
}

impl GeminiCliRunner {
    pub fn new(model: Option<String>) -> Self {
        Self { model }
    }

    pub async fn run(&self, prompt: &str) -> Result<NativeAgentResult> {
        self.run_with_session(prompt, None).await
    }

    pub async fn run_with_session(
        &self,
        prompt: &str,
        session_handle: Option<&str>,
    ) -> Result<NativeAgentResult> {
        info!(
            "GeminiCliRunner: invoking gemini CLI, model={:?}",
            self.model
        );

        let mut args = vec![
            "--prompt".to_string(),
            prompt.to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
        ];
        append_gemini_bypass_permission_args(&mut args);
        if let Some(model) = &self.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }
        if let Some(handle) = session_handle {
            args.push("--resume".to_string());
            args.push(handle.to_string());
        }

        let output = native_cli_command("gemini", &args)
            .output()
            .await
            .context("Failed to spawn `gemini` CLI — is it installed and on PATH?")?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if is_windows_command_not_found(&stdout, &stderr) {
            anyhow::bail!("Failed to spawn `gemini` CLI — is it installed and on PATH?");
        }
        let mut parsed = parse_gemini_output(&stdout, session_handle)?;

        if !output.status.success() && parsed.authorization_required.is_none() {
            parsed.authorization_required =
                detect_authorization_required(&format!("{}\n{}", stdout, stderr));
        }

        if !output.status.success() {
            if parsed.authorization_required.is_some() {
                return Ok(parsed);
            }
            return Err(anyhow::anyhow!(
                "gemini CLI exited with {}: {}",
                output.status,
                stderr
            ));
        }

        Ok(parsed)
    }
}

fn append_gemini_bypass_permission_args(args: &mut Vec<String>) {
    let has_permission_override = args
        .iter()
        .any(|arg| arg == "--approval-mode" || arg == "--yolo" || arg == "-y");
    if has_permission_override {
        return;
    }

    args.push("--approval-mode".to_string());
    args.push("yolo".to_string());
}

fn parse_gemini_output(raw: &str, fallback_session: Option<&str>) -> Result<NativeAgentResult> {
    let mut final_text = String::new();
    let mut tokens_used = 0u32;
    let mut tool_calls = 0u32;
    let mut session_handle = fallback_session.map(str::to_string);

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }

        let Ok(obj) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if let Some(handle) = obj.get("session_id").and_then(|id| id.as_str()) {
            session_handle = Some(handle.to_string());
        }

        match obj.get("type").and_then(|t| t.as_str()) {
            Some("message") => {
                if obj.get("role").and_then(|r| r.as_str()) == Some("assistant") {
                    if let Some(text) = obj.get("content").and_then(|c| c.as_str()) {
                        final_text = text.to_string();
                    }
                }
            }
            Some("tool_call") => tool_calls += 1,
            Some("result") => {
                if let Some(stats) = obj.get("stats") {
                    tokens_used = stats
                        .get("total_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                    tool_calls = tool_calls.max(
                        stats
                            .get("tool_calls")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32,
                    );
                }
            }
            _ => {}
        }
    }

    if final_text.is_empty() {
        final_text = raw.trim().to_string();
    }

    let structured = serde_json::from_str(&final_text).unwrap_or_else(|_| json!(final_text));

    Ok(NativeAgentResult {
        output: final_text,
        structured,
        tool_calls,
        tokens_used,
        session_handle,
        authorization_required: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_runner() {
        let runner = GeminiCliRunner::new(Some("gemini-2.0-flash".to_string()));
        assert_eq!(runner.model.as_deref(), Some("gemini-2.0-flash"));
    }

    #[test]
    fn appends_yolo_approval_mode_by_default() {
        let mut args = vec!["--prompt".to_string(), "hello".to_string()];
        append_gemini_bypass_permission_args(&mut args);

        assert!(args
            .windows(2)
            .any(|pair| { pair[0] == "--approval-mode" && pair[1] == "yolo" }));
    }
}
