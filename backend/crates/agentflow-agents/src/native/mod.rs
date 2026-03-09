pub mod claude_code;
pub mod codex;
pub mod gemini_cli;
pub mod pty_broker;

pub use claude_code::ClaudeCodeRunner;
pub use codex::CodexRunner;
pub use gemini_cli::GeminiCliRunner;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct NativeAuthorizationRequest {
    pub message: String,
    pub details: Option<String>,
}

/// Unified result from any native agent runner
#[derive(Debug)]
pub struct NativeAgentResult {
    pub output: String,
    pub structured: serde_json::Value,
    pub tool_calls: u32,
    pub tokens_used: u32,
    pub session_handle: Option<String>,
    pub authorization_required: Option<NativeAuthorizationRequest>,
}

pub fn detect_authorization_required(text: &str) -> Option<NativeAuthorizationRequest> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let patterns = [
        "permission denied",
        "permission denied by user",
        "permission request",
        "permission requests",
        "requires approval",
        "require approval",
        "approval required",
        "authorization required",
        "requires authorization",
        "needs authorization",
        "human review required",
        "do you trust this directory",
        "do you trust this folder",
        "trust this directory",
        "trust this folder",
        "working with untrusted contents",
    ];

    if let Some(pattern) = patterns.iter().find(|pattern| lower.contains(**pattern)) {
        let snippet = trimmed
            .lines()
            .find(|line| line.to_ascii_lowercase().contains(pattern))
            .or_else(|| trimmed.lines().find(|line| !line.trim().is_empty()))
            .unwrap_or(trimmed)
            .trim()
            .chars()
            .take(240)
            .collect::<String>();

        return Some(NativeAuthorizationRequest {
            message: "Agent requires authorization before it can continue.".to_string(),
            details: Some(snippet),
        });
    }

    None
}

pub fn native_cli_command(binary: &str, args: &[String]) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd.exe");
        cmd.arg("/d").arg("/c").arg(binary).args(args);
        cmd
    }

    #[cfg(not(windows))]
    {
        let mut cmd = Command::new(binary);
        cmd.args(args);
        cmd
    }
}

pub fn is_windows_command_not_found(stdout: &str, stderr: &str) -> bool {
    #[cfg(windows)]
    {
        let combined = format!("{}\n{}", stdout, stderr).to_ascii_lowercase();
        combined.contains("is not recognized as an internal or external command")
            || combined.contains("the system cannot find the path specified")
    }

    #[cfg(not(windows))]
    {
        let _ = stdout;
        let _ = stderr;
        false
    }
}
