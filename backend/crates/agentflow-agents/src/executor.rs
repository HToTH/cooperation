use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use agentflow_core::{
    graph::node::{AgentKind, AgentNode, ModelProvider},
    protocol::{
        messages::{ExecutionMetadata, TaskResultPayload, TaskStatus},
        ws::WsEvent,
    },
};
use agentflow_memory::{context_pool::ConversationTurn, ContextPool, MemoryManager};

use crate::models::{ClaudeClient, ContentBlock, GeminiClient, LlmClient, LlmTool, OpenAIClient};
use crate::native::{ClaudeCodeRunner, CodexRunner, GeminiCliRunner, NativeAgentResult};

fn gemini_cli_model(model: &ModelProvider) -> Option<String> {
    match model {
        ModelProvider::Gemini(m) if !m.trim().is_empty() => Some(m.clone()),
        _ => None,
    }
}

fn codex_cli_model(model: &ModelProvider) -> Option<String> {
    match model {
        ModelProvider::OpenAI(m) if !m.trim().is_empty() => Some(m.clone()),
        _ => None,
    }
}

// ─── Config ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ExecutorConfig {
    pub anthropic_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub openai_api_key: Option<String>,
}

impl ExecutorConfig {
    pub fn from_env() -> Self {
        Self {
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            gemini_api_key: std::env::var("GEMINI_API_KEY").ok(),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
        }
    }
}

// ─── Worker registration ──────────────────────────────────────────────────────

/// A Worker Agent exposed as a callable tool to the Leader
#[derive(Clone)]
pub struct WorkerTool {
    pub tool_def: LlmTool,
    pub node: AgentNode,
}

// ─── Agentic loop result ──────────────────────────────────────────────────────

pub struct AgenticLoopResult {
    pub final_text: String,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub tool_call_count: u32,
}

// ─── Executor ────────────────────────────────────────────────────────────────

pub struct AgentExecutor {
    pub config: ExecutorConfig,
}

impl AgentExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    fn build_client(&self, model: &ModelProvider) -> Result<Arc<dyn LlmClient>> {
        match model {
            ModelProvider::Claude(model_name) => {
                let api_key = self
                    .config
                    .anthropic_api_key
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("ANTHROPIC_API_KEY not configured"))?;
                Ok(Arc::new(ClaudeClient::new(api_key, model_name.clone())))
            }
            ModelProvider::Gemini(model_name) => {
                let api_key = self
                    .config
                    .gemini_api_key
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("GEMINI_API_KEY not configured"))?;
                Ok(Arc::new(GeminiClient::new(api_key, model_name.clone())))
            }
            ModelProvider::OpenAI(model_name) => {
                let api_key = self
                    .config
                    .openai_api_key
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not configured"))?;
                Ok(Arc::new(OpenAIClient::new(api_key, model_name.clone())))
            }
        }
    }

    /// **Agentic Loop** — Leader drives the orchestration via native tool calls.
    ///
    /// Flow:
    ///   1. Leader sends initial message with Worker tools registered
    ///   2. If LLM returns tool_use → run the Worker agent with the tool input
    ///   3. Return tool_result to Leader → LLM continues reasoning
    ///   4. Repeat until stop_reason == EndTurn
    ///
    /// Context isolation is preserved: each Worker only sees its own task + history.
    pub async fn run_agentic_loop(
        &self,
        leader: &AgentNode,
        leader_pool: &mut ContextPool,
        initial_message: String,
        worker_tools: Vec<WorkerTool>,
        memory_manager: &Arc<MemoryManager>,
        event_tx: &broadcast::Sender<WsEvent>,
        workflow_id: &str,
    ) -> Result<AgenticLoopResult> {
        let client = self.build_client(&leader.model)?;
        let start = Instant::now();

        // Build tool index: tool_name -> WorkerTool
        let tool_index: HashMap<String, WorkerTool> = worker_tools
            .iter()
            .map(|wt| (wt.tool_def.name.clone(), wt.clone()))
            .collect();

        let tool_defs: Vec<LlmTool> = worker_tools.iter().map(|wt| wt.tool_def.clone()).collect();

        // Conversation messages for the Leader (raw API format)
        let mut messages: Vec<Value> = vec![json!({ "role": "user", "content": initial_message })];

        let system_prompt = leader_pool.get_system_prompt().map(String::from);
        let mut total_input = 0u32;
        let mut total_output = 0u32;
        let mut tool_call_count = 0u32;
        let max_iterations = 20; // safety limit

        for iteration in 0..max_iterations {
            debug!(
                "Agentic loop iteration {} for leader {}",
                iteration, leader.id
            );

            let response = client
                .complete_with_tools(
                    system_prompt.as_deref(),
                    messages.clone(),
                    leader.model_config.max_tokens,
                    leader.model_config.temperature,
                    &tool_defs,
                )
                .await?;

            total_input += response.input_tokens;
            total_output += response.output_tokens;

            // Build the assistant message from content blocks (Claude format)
            let assistant_content: Vec<Value> = response
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
                    ContentBlock::ToolUse { id, name, input } => json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input
                    }),
                })
                .collect();

            messages.push(json!({ "role": "assistant", "content": assistant_content }));

            // Log text output if any
            let text = response.text_content();
            if !text.is_empty() {
                info!(
                    "[Leader {}] {}",
                    leader.id,
                    text.chars().take(200).collect::<String>()
                );
                leader_pool.add_turn(ConversationTurn::assistant(text));
            }

            // Done — no tool calls
            if response.stop_reason.is_done() {
                break;
            }

            // Process tool calls (Worker invocations)
            let tool_uses: Vec<&ContentBlock> = response.tool_uses();
            if tool_uses.is_empty() {
                break;
            }

            let mut tool_results: Vec<Value> = Vec::new();

            for block in tool_uses {
                let ContentBlock::ToolUse { id, name, input } = block else {
                    continue;
                };

                tool_call_count += 1;
                info!(
                    "[Leader {}] calling tool '{}' (call #{})",
                    leader.id, name, tool_call_count
                );

                // Emit event so frontend animates the edge
                let _ = event_tx.send(WsEvent::AgentMessageSent {
                    workflow_id: workflow_id.to_string(),
                    message: agentflow_core::protocol::messages::AgentMessage {
                        protocol_version: "1.0".into(),
                        message_id: uuid::Uuid::new_v4().to_string(),
                        from_agent: agentflow_core::protocol::messages::AgentIdentity {
                            id: leader.id.clone(),
                            role: "leader".into(),
                        },
                        to_agent: agentflow_core::protocol::messages::AgentIdentity {
                            id: name.clone(),
                            role: "worker".into(),
                        },
                        message_type: agentflow_core::protocol::messages::MessageType::TaskDispatch,
                        payload: input.clone(),
                        in_reply_to: None,
                    },
                });

                // Find the worker and run it
                let result_json = match tool_index.get(name) {
                    Some(worker_tool) => {
                        match self
                            .run_worker_for_tool(
                                &worker_tool.node,
                                input.clone(),
                                memory_manager,
                                event_tx,
                                workflow_id,
                            )
                            .await
                        {
                            Ok(result) => result,
                            Err(e) => {
                                warn!("Worker tool '{}' failed: {}", name, e);
                                json!({ "error": e.to_string() })
                            }
                        }
                    }
                    None => {
                        warn!("Unknown tool called: {}", name);
                        json!({ "error": format!("Unknown tool: {}", name) })
                    }
                };

                // Claude tool_result format
                tool_results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": id,
                    "content": result_json.to_string(),
                }));

                // Emit result event
                let _ = event_tx.send(WsEvent::AgentMessageSent {
                    workflow_id: workflow_id.to_string(),
                    message: agentflow_core::protocol::messages::AgentMessage {
                        protocol_version: "1.0".into(),
                        message_id: uuid::Uuid::new_v4().to_string(),
                        from_agent: agentflow_core::protocol::messages::AgentIdentity {
                            id: name.clone(),
                            role: "worker".into(),
                        },
                        to_agent: agentflow_core::protocol::messages::AgentIdentity {
                            id: leader.id.clone(),
                            role: "leader".into(),
                        },
                        message_type: agentflow_core::protocol::messages::MessageType::TaskResult,
                        payload: result_json,
                        in_reply_to: Some(id.clone()),
                    },
                });
            }

            // Feed tool results back to Leader as user message
            messages.push(json!({ "role": "user", "content": tool_results }));
        }

        let final_text = leader_pool
            .get_turns()
            .iter()
            .rev()
            .find(|t| t.role == agentflow_memory::context_pool::TurnRole::Assistant)
            .map(|t| t.content.clone())
            .unwrap_or_default();

        info!(
            "[Leader {}] agentic loop done: {} tool calls, {} tokens, {}ms",
            leader.id,
            tool_call_count,
            total_input + total_output,
            start.elapsed().as_millis()
        );

        Ok(AgenticLoopResult {
            final_text,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            tool_call_count,
        })
    }

    /// Run a Worker agent with a tool input. The Worker gets an isolated ContextPool.
    /// Returns structured JSON — this is ALL the Leader will ever see (context isolation).
    ///
    /// Dispatches to the correct runner based on `worker.kind`:
    /// - `ClaudeCode`      → spawns `claude` CLI subprocess (native bash/file/web)
    /// - `GeminiNative`    → Gemini API with code_execution + google_search (server-side)
    /// - `OpenAIResponses` → OpenAI Responses API with code_interpreter + web_search (server-side)
    /// - `RawLlm`          → raw LLM API call (existing behavior)
    async fn run_worker_for_tool(
        &self,
        worker: &AgentNode,
        tool_input: Value,
        memory_manager: &Arc<MemoryManager>,
        event_tx: &broadcast::Sender<WsEvent>,
        workflow_id: &str,
    ) -> Result<Value> {
        let start = Instant::now();

        let _ = event_tx.send(WsEvent::NodeStateChanged {
            workflow_id: workflow_id.to_string(),
            node_id: worker.id.clone(),
            state: agentflow_core::graph::AgentNodeState::Running,
        });

        let task_str = tool_input
            .get("task")
            .and_then(|t| t.as_str())
            .unwrap_or_else(|| tool_input.as_str().unwrap_or("Complete the assigned task"))
            .to_string();

        let system_prompt = if worker.model_config.system_prompt.is_empty() {
            None
        } else {
            Some(worker.model_config.system_prompt.clone())
        };

        let native_result: NativeAgentResult = match &worker.kind {
            // ── Claude Code CLI ──────────────────────────────────────────────
            AgentKind::ClaudeCode => {
                info!("[Worker {}] using Claude Code CLI", worker.id);
                let runner = ClaudeCodeRunner::new();
                runner
                    .run(system_prompt.as_deref(), &task_str, None)
                    .await?
            }

            // ── Gemini CLI subprocess ────────────────────────────────────────
            AgentKind::GeminiCli => {
                info!("[Worker {}] using Gemini CLI", worker.id);
                let prompt = if let Some(sys) = &system_prompt {
                    format!("{}\n\n{}", sys, task_str)
                } else {
                    task_str.clone()
                };
                let runner = GeminiCliRunner::new(gemini_cli_model(&worker.model));
                runner.run(&prompt).await?
            }

            // ── Codex CLI subprocess ─────────────────────────────────────────
            AgentKind::Codex => {
                info!("[Worker {}] using Codex CLI", worker.id);
                let prompt = if let Some(sys) = &system_prompt {
                    format!("{}\n\n{}", sys, task_str)
                } else {
                    task_str.clone()
                };
                let runner = CodexRunner::new(codex_cli_model(&worker.model));
                runner.run(&prompt).await?
            }

            // ── Raw LLM API (original behavior) ──────────────────────────────
            AgentKind::RawLlm => {
                info!("[Worker {}] using raw LLM API", worker.id);
                let client = self.build_client(&worker.model)?;

                let pool_id = worker.context_pool_id.clone();
                if memory_manager.get_context_pool(&pool_id).is_none() {
                    memory_manager.create_context_pool_with_prompt(
                        pool_id.clone(),
                        worker.id.clone(),
                        worker.model_config.system_prompt.clone(),
                    );
                }

                let sys = memory_manager
                    .get_context_pool(&pool_id)
                    .and_then(|p| p.get_system_prompt().map(String::from));

                let messages = vec![json!({ "role": "user", "content": task_str })];
                let resp = client
                    .complete(
                        sys.as_deref(),
                        messages,
                        worker.model_config.max_tokens,
                        worker.model_config.temperature,
                    )
                    .await?;

                if let Some(mut pool) = memory_manager.get_context_pool_mut(&pool_id) {
                    pool.add_turn(ConversationTurn::user(task_str.clone()));
                    pool.add_turn(ConversationTurn::assistant(resp.content.clone()));
                }

                let structured = serde_json::from_str(&resp.content)
                    .unwrap_or_else(|_| json!({ "result": resp.content }));
                NativeAgentResult {
                    output: resp.content,
                    structured,
                    tool_calls: 0,
                    tokens_used: resp.input_tokens + resp.output_tokens,
                    session_handle: None,
                    authorization_required: None,
                }
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;
        info!(
            "[Worker {}] {:?} completed: {} tool calls, {} tokens, {}ms",
            worker.id,
            worker.kind,
            native_result.tool_calls,
            native_result.tokens_used,
            duration_ms
        );

        let _ = event_tx.send(WsEvent::NodeStateChanged {
            workflow_id: workflow_id.to_string(),
            node_id: worker.id.clone(),
            state: agentflow_core::graph::AgentNodeState::Completed,
        });

        Ok(native_result.structured)
    }

    /// Simple single-turn (for workers that don't use tools themselves)
    pub async fn run_single_turn(
        &self,
        node: &AgentNode,
        context_pool: &mut ContextPool,
        user_message: &str,
    ) -> Result<TaskResultPayload> {
        let client = self.build_client(&node.model)?;
        let start = Instant::now();

        context_pool.add_turn(ConversationTurn::user(user_message.to_string()));
        let system_prompt = context_pool.get_system_prompt().map(String::from);
        let messages = context_pool.get_messages_for_api();

        let response = client
            .complete(
                system_prompt.as_deref(),
                messages,
                node.model_config.max_tokens,
                node.model_config.temperature,
            )
            .await?;

        let duration_ms = start.elapsed().as_millis() as u64;
        let total_tokens = response.input_tokens + response.output_tokens;

        context_pool.add_turn(ConversationTurn::assistant(response.content.clone()));

        let result = serde_json::from_str(&response.content)
            .unwrap_or_else(|_| json!({ "raw_response": response.content }));

        Ok(TaskResultPayload {
            status: TaskStatus::Completed,
            result,
            error: None,
            execution_metadata: ExecutionMetadata {
                tokens_used: total_tokens,
                context_pool_id: context_pool.id.clone(),
                duration_ms,
            },
        })
    }

    /// Multi-turn conversational chat with a single agent.
    /// `history` is the existing conversation as `[{"role":"user"|"assistant","content":"..."}]`.
    /// Returns the assistant's reply text.
    pub async fn chat(
        &self,
        agent: &AgentNode,
        history: &[serde_json::Value],
        user_message: &str,
    ) -> Result<String> {
        use crate::native::{ClaudeCodeRunner, CodexRunner, GeminiCliRunner};
        use agentflow_core::graph::node::AgentKind;

        let system_prompt = if agent.model_config.system_prompt.is_empty() {
            None
        } else {
            Some(agent.model_config.system_prompt.as_str())
        };

        match &agent.kind {
            AgentKind::RawLlm => {
                let client = self.build_client(&agent.model)?;
                let mut messages = history.to_vec();
                messages.push(serde_json::json!({ "role": "user", "content": user_message }));
                let resp = client
                    .complete(
                        system_prompt,
                        messages,
                        agent.model_config.max_tokens,
                        agent.model_config.temperature,
                    )
                    .await?;
                Ok(resp.content)
            }
            // For CLI agents, collapse history into a single prompt
            AgentKind::ClaudeCode => {
                let prompt = build_cli_prompt(system_prompt, history, user_message);
                let runner = ClaudeCodeRunner::new();
                let res = runner.run(None, &prompt, None).await?;
                Ok(res.output)
            }
            AgentKind::GeminiCli => {
                let prompt = build_cli_prompt(system_prompt, history, user_message);
                let runner = GeminiCliRunner::new(gemini_cli_model(&agent.model));
                let res = runner.run(&prompt).await?;
                Ok(res.output)
            }
            AgentKind::Codex => {
                let prompt = build_cli_prompt(system_prompt, history, user_message);
                let runner = CodexRunner::new(codex_cli_model(&agent.model));
                let res = runner.run(&prompt).await?;
                Ok(res.output)
            }
        }
    }

    /// Single-turn native chat that relies on the CLI's own persisted session state.
    /// `session_handle` is runner-specific continuation metadata returned by the
    /// previous native invocation for this workflow+agent pair.
    pub async fn chat_native_with_session(
        &self,
        agent: &AgentNode,
        user_message: &str,
        session_handle: Option<&str>,
    ) -> Result<NativeAgentResult> {
        use crate::native::{ClaudeCodeRunner, CodexRunner, GeminiCliRunner};
        use agentflow_core::graph::node::AgentKind;

        let system_prompt = if agent.model_config.system_prompt.is_empty() {
            None
        } else {
            Some(agent.model_config.system_prompt.as_str())
        };

        let prompt = if session_handle.is_some() || system_prompt.is_none() {
            user_message.to_string()
        } else {
            format!("{}\n\n{}", system_prompt.unwrap_or_default(), user_message)
        };

        match &agent.kind {
            AgentKind::ClaudeCode => {
                let runner = ClaudeCodeRunner::new();
                runner
                    .run_with_session(system_prompt, user_message, None, session_handle)
                    .await
            }
            AgentKind::GeminiCli => {
                let runner = GeminiCliRunner::new(gemini_cli_model(&agent.model));
                runner.run_with_session(&prompt, session_handle).await
            }
            AgentKind::Codex => {
                let runner = CodexRunner::new(codex_cli_model(&agent.model));
                runner.run_with_session(&prompt, session_handle).await
            }
            AgentKind::RawLlm => anyhow::bail!("Raw LLM agents do not use native session chat"),
        }
    }

    /// Simple single-turn task execution for topology-driven workflows.
    /// Each node receives a `prompt` (built from predecessor outputs) and
    /// returns its text output. Dispatches based on `node.kind`.
    pub async fn run_task(
        &self,
        node: &AgentNode,
        prompt: &str,
        memory_manager: &MemoryManager,
    ) -> Result<String> {
        use crate::native::{ClaudeCodeRunner, CodexRunner, GeminiCliRunner};
        use agentflow_core::graph::node::AgentKind;

        let system_prompt = if node.model_config.system_prompt.is_empty() {
            None
        } else {
            Some(node.model_config.system_prompt.clone())
        };

        match &node.kind {
            AgentKind::ClaudeCode => {
                let full_prompt = if let Some(sys) = &system_prompt {
                    format!("{}\n\n{}", sys, prompt)
                } else {
                    prompt.to_string()
                };
                let runner = ClaudeCodeRunner::new();
                let result = runner.run(None, &full_prompt, None).await?;
                Ok(result.output)
            }
            AgentKind::GeminiCli => {
                let full_prompt = if let Some(sys) = &system_prompt {
                    format!("{}\n\n{}", sys, prompt)
                } else {
                    prompt.to_string()
                };
                let runner = GeminiCliRunner::new(gemini_cli_model(&node.model));
                let result = runner.run(&full_prompt).await?;
                Ok(result.output)
            }
            AgentKind::Codex => {
                let full_prompt = if let Some(sys) = &system_prompt {
                    format!("{}\n\n{}", sys, prompt)
                } else {
                    prompt.to_string()
                };
                let runner = CodexRunner::new(codex_cli_model(&node.model));
                let result = runner.run(&full_prompt).await?;
                Ok(result.output)
            }
            AgentKind::RawLlm => {
                let pool_id = node.context_pool_id.clone();
                if memory_manager.get_context_pool(&pool_id).is_none() {
                    memory_manager.create_context_pool_with_prompt(
                        pool_id.clone(),
                        node.id.clone(),
                        node.model_config.system_prompt.clone(),
                    );
                }
                let client = self.build_client(&node.model)?;
                let sys = memory_manager
                    .get_context_pool(&pool_id)
                    .and_then(|p| p.get_system_prompt().map(String::from));
                let messages = vec![serde_json::json!({ "role": "user", "content": prompt })];
                let resp = client
                    .complete(
                        sys.as_deref(),
                        messages,
                        node.model_config.max_tokens,
                        node.model_config.temperature,
                    )
                    .await?;
                Ok(resp.content)
            }
        }
    }
}

/// Build a single text prompt from conversation history for CLI-based agents.
fn build_cli_prompt(
    system_prompt: Option<&str>,
    history: &[serde_json::Value],
    user_message: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(sys) = system_prompt {
        parts.push(format!("[System]\n{}\n", sys));
    }
    for msg in history {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
        let label = if role == "user" { "User" } else { "Assistant" };
        parts.push(format!("[{}]\n{}", label, content));
    }
    parts.push(format!("[User]\n{}", user_message));
    parts.push("[Assistant]".to_string());
    parts.join("\n\n")
}
