use anyhow::{Context, Result};
use serde_json::{json, Value};
use tracing::info;

use super::{
    detect_authorization_required, is_windows_command_not_found, native_cli_command,
    NativeAgentResult,
};

/// Runs OpenAI Codex CLI as a subprocess.
/// Requires `codex` CLI installed (`npm install -g @openai/codex`) and
/// `OPENAI_API_KEY` set in the environment.
pub struct CodexRunner {
    model: Option<String>,
}

impl CodexRunner {
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
        info!("CodexRunner: invoking codex CLI, model={:?}", self.model);

        let mut args = vec!["exec".to_string()];
        if let Some(handle) = session_handle {
            args.push("resume".to_string());
            args.push(handle.to_string());
        }
        append_codex_bypass_permission_args(&mut args);
        args.push("--json".to_string());
        if let Some(model) = &self.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }
        args.push(prompt.to_string());

        let output = native_cli_command("codex", &args).output().await.context(
            "Failed to spawn `codex` CLI — is it installed? (`npm install -g @openai/codex`)",
        )?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if is_windows_command_not_found(&stdout, &stderr) {
            anyhow::bail!(
                "Failed to spawn `codex` CLI — is it installed? (`npm install -g @openai/codex`)"
            );
        }
        let mut parsed = parse_codex_output(&stdout, session_handle)?;

        if !output.status.success() && parsed.authorization_required.is_none() {
            parsed.authorization_required =
                detect_authorization_required(&format!("{}\n{}", stdout, stderr));
        }

        if !output.status.success() {
            if parsed.authorization_required.is_some() {
                return Ok(parsed);
            }
            return Err(anyhow::anyhow!(
                "codex CLI exited with {}: {}",
                output.status,
                stderr
            ));
        }

        Ok(parsed)
    }
}

fn append_codex_bypass_permission_args(args: &mut Vec<String>) {
    let has_permission_override = args.iter().any(|arg| {
        arg == "--dangerously-bypass-approvals-and-sandbox"
            || arg == "--full-auto"
            || arg == "--ask-for-approval"
            || arg == "-a"
            || arg == "--sandbox"
            || arg == "-s"
    });
    if has_permission_override {
        return;
    }

    args.push("--dangerously-bypass-approvals-and-sandbox".to_string());
}

fn parse_codex_output(raw: &str, fallback_session: Option<&str>) -> Result<NativeAgentResult> {
    let mut final_text = String::new();
    let mut tokens_used = 0u32;
    let mut session_handle = fallback_session.map(str::to_string);

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }

        let Ok(obj) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        match obj.get("type").and_then(|t| t.as_str()) {
            Some("thread.started") => {
                if let Some(thread_id) = obj.get("thread_id").and_then(|id| id.as_str()) {
                    session_handle = Some(thread_id.to_string());
                }
            }
            Some("item.completed") => {
                if let Some(text) = obj
                    .get("item")
                    .and_then(|item| item.get("text"))
                    .and_then(|text| text.as_str())
                {
                    final_text = text.to_string();
                }
            }
            Some("turn.completed") => {
                if let Some(usage) = obj.get("usage") {
                    tokens_used = usage
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32
                        + usage
                            .get("output_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32;
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
        tool_calls: 0,
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
        let runner = CodexRunner::new(Some("codex-mini".to_string()));
        assert_eq!(runner.model.as_deref(), Some("codex-mini"));
    }

    #[test]
    fn appends_bypass_flag_by_default() {
        let mut args = vec!["exec".to_string()];
        append_codex_bypass_permission_args(&mut args);

        assert!(args
            .iter()
            .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox"));
    }
}
