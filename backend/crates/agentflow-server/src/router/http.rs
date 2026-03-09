use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use tracing::warn;
use uuid::Uuid;

use crate::attachments::{parse_attachments, render_attachment_context, AttachmentLike};
use crate::AppState;
use agentflow_agents::native::pty_broker;
use agentflow_core::graph::{
    node::{AgentKind, AgentNode, ModelProvider},
    DirectedEdge, WorkflowGraph,
};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/api/workflows", get(list_workflows).post(save_workflow))
        .route(
            "/api/workflows/:id",
            get(get_workflow).delete(delete_workflow),
        )
        .route("/api/workflows/:id/memory", get(get_workflow_memory))
        .route("/api/workflows/:id/group-chat", get(get_group_chat_history))
        .route("/api/chat", post(chat))
        .route("/api/group-chat", post(group_chat))
        .route("/api/pty", post(create_pty_session))
        .route(
            "/api/pty/:session_id",
            axum::routing::delete(close_pty_session),
        )
        .with_state(state)
}

#[derive(Deserialize)]
struct ChatRequest {
    workflow_id: String,
    agent_id: String,
    agent: AgentNode,
    message: String,
}

#[derive(Serialize)]
struct ChatResponse {
    agent_id: String,
    response: String,
}

async fn health_check() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "cooperation" }))
}

async fn list_workflows(State(state): State<Arc<AppState>>) -> Result<Json<Value>, StatusCode> {
    match state.memory_manager.list_workflows().await {
        Ok(workflows) => {
            let list: Vec<Value> = workflows
                .into_iter()
                .map(|(id, name, updated_at)| json!({ "id": id, "name": name, "updated_at": updated_at }))
                .collect();
            Ok(Json(json!(list)))
        }
        Err(e) => {
            tracing::error!("Failed to list workflows: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn save_workflow(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let id = body.get("id").and_then(|v| v.as_str()).unwrap_or_default();
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled");
    let graph_json = serde_json::to_string(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    match state
        .memory_manager
        .save_workflow(id, name, &graph_json)
        .await
    {
        Ok(_) => Ok(Json(json!({ "id": id, "saved": true }))),
        Err(e) => {
            tracing::error!("Failed to save workflow: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_workflow(
    Path(workflow_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, StatusCode> {
    match state.memory_manager.load_workflow(&workflow_id).await {
        Ok(Some(json_str)) => {
            let graph: Value =
                serde_json::from_str(&json_str).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(Json(graph))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!("Failed to load workflow: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn delete_workflow(
    Path(workflow_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, StatusCode> {
    match state.memory_manager.delete_workflow(&workflow_id).await {
        Ok(_) => Ok(Json(json!({ "id": workflow_id, "deleted": true }))),
        Err(e) => {
            tracing::error!("Failed to delete workflow: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_workflow_memory(
    Path(workflow_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, StatusCode> {
    match state.memory_manager.query_global(&workflow_id).await {
        Ok(entries) => Ok(Json(
            json!({ "workflow_id": workflow_id, "entries": entries }),
        )),
        Err(e) => {
            tracing::error!("Failed to query global memory: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_group_chat_history(
    Path(workflow_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<GroupChatHistoryResponse>, (StatusCode, String)> {
    let messages = load_group_chat_history(&state, &workflow_id).await?;
    Ok(Json(GroupChatHistoryResponse { messages }))
}

async fn chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, String)> {
    let history_key = format!("{}_{}", req.workflow_id, req.agent_id);

    let history: Vec<Value> = state
        .chat_histories
        .get(&history_key)
        .map(|h| h.value().clone())
        .unwrap_or_default();

    let executor = state.executor_pool.executor();
    let response = executor
        .chat(&req.agent, &history, &req.message)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Update history
    let mut new_history = history;
    new_history.push(json!({ "role": "user", "content": req.message }));
    new_history.push(json!({ "role": "assistant", "content": response.clone() }));
    state.chat_histories.insert(history_key, new_history);

    Ok(Json(ChatResponse {
        agent_id: req.agent_id,
        response,
    }))
}

const GROUP_CHAT_MESSAGE_KEY: &str = "group_chat_message";
const USER_TARGET_LABEL: &str = "我";
const MAX_GROUP_CHAT_ROUTE_STEPS: usize = 12;
const GROUP_CHAT_SYSTEM_PROMPT_SUFFIX: &str = r#"你正在团队群聊中协作。

通用规则：
- 你只代表你自己发言，不能替其他 Agent 回答。
- 你只能处理当前收到的这条消息，不要自发插话。
- 回复必须简短，最多 3 句短句或 3 个短 bullet。
- 不要寒暄，不要重复背景，不要输出思维过程。

结构化回复协议：
- 你必须只返回一个 JSON 对象。
- 不要输出 markdown。
- 不要输出代码块。
- 不要输出 JSON 之外的任何文字。
- 固定格式：{"target":"<目标>","message":"<简短回复>","done":<true|false>}
- target 必须从当前消息给出的允许目标中选择一个。
- message 必须简洁明确，优先给结论、状态、阻塞或下一步。
- done=true 表示这轮沟通结束；done=false 表示还需要继续传递。
- done=true 时，target 必须是“我”。
- target 不能指向你自己。

结束规则：
- 如果问题已经沟通完成，返回类似：
  {"target":"我","message":"好的","done":true}
  {"target":"我","message":"了解","done":true}
- 如果还需要别人继续处理，就把 target 指向下一个对象，done=false。"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupAttachment {
    name: String,
    content_type: String,
    data: String,
}

#[derive(Deserialize)]
struct GroupChatRequest {
    workflow_id: String,
    message: String,
    mentioned_agent_ids: Vec<String>,
    graph: WorkflowGraph,
    attachments: Option<Vec<GroupAttachment>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GroupSystemLevel {
    Info,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum GroupTaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum GroupHitlStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GroupChatMessage {
    User {
        id: String,
        content: String,
        attachments: Vec<GroupAttachment>,
        mentioned_agent_ids: Vec<String>,
        timestamp: i64,
    },
    Agent {
        id: String,
        agent_id: String,
        content: String,
        timestamp: i64,
    },
    System {
        id: String,
        content: String,
        level: GroupSystemLevel,
        timestamp: i64,
    },
    Task {
        id: String,
        task_id: String,
        agent_id: String,
        command: String,
        status: GroupTaskStatus,
        summary: Option<String>,
        timestamp: i64,
    },
    Hitl {
        id: String,
        workflow_id: String,
        node_id: String,
        description: String,
        context: Value,
        status: GroupHitlStatus,
        reason: Option<String>,
        timestamp: i64,
    },
}

#[derive(Serialize)]
struct GroupChatResponse {
    messages: Vec<GroupChatMessage>,
    workflow_graph: Option<WorkflowGraph>,
}

#[derive(Serialize)]
struct GroupChatHistoryResponse {
    messages: Vec<GroupChatMessage>,
}

struct GroupChatCommandOutcome {
    messages: Vec<GroupChatMessage>,
    workflow_graph: Option<WorkflowGraph>,
}

#[derive(Debug, Clone, Deserialize)]
struct GroupReplyEnvelope {
    target: String,
    message: String,
    #[serde(default)]
    done: bool,
}

#[derive(Debug, Clone)]
enum GroupReplyTarget {
    User,
    Agent(String),
}

#[derive(Debug, Clone)]
struct PendingGroupDelivery {
    sender_label: String,
    target_agent_id: String,
    body: String,
    hop: usize,
}

impl AttachmentLike for GroupAttachment {
    fn name(&self) -> &str {
        &self.name
    }

    fn content_type(&self) -> &str {
        &self.content_type
    }

    fn data(&self) -> &str {
        &self.data
    }
}

fn collect_group_chat_targets(
    mentioned_agent_ids: &[String],
    message: &str,
    agents: &HashMap<String, AgentNode>,
) -> Vec<String> {
    let parsed_targets = collect_group_chat_targets_from_message(message, agents);
    if !parsed_targets.is_empty() {
        return parsed_targets;
    }

    if mentioned_agent_ids.is_empty() {
        let mut participants = agents
            .values()
            .filter(|agent| is_chat_participant(agent))
            .map(|agent| (agent.label.clone(), agent.id.clone()))
            .collect::<Vec<_>>();
        participants.sort_by(|left, right| left.0.cmp(&right.0));
        return participants.into_iter().map(|(_, id)| id).collect();
    }

    let mut targets = Vec::new();

    for agent_id in mentioned_agent_ids {
        let Some(agent) = agents.get(agent_id) else {
            continue;
        };
        if !is_chat_participant(agent) {
            continue;
        }
        if targets.iter().any(|id| id == agent_id) {
            continue;
        }
        targets.push(agent_id.clone());
    }

    targets
}

fn collect_group_chat_targets_from_message(
    message: &str,
    agents: &HashMap<String, AgentNode>,
) -> Vec<String> {
    let mut candidates = agents
        .values()
        .filter(|agent| is_chat_participant(agent))
        .map(|agent| {
            (
                agent.label.clone(),
                agent.label.to_lowercase(),
                agent.id.clone(),
            )
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .0
            .chars()
            .count()
            .cmp(&left.0.chars().count())
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut targets = Vec::new();

    for (index, ch) in message.char_indices() {
        if ch != '@' {
            continue;
        }
        if index > 0
            && !message[..index]
                .chars()
                .next_back()
                .map(|value| value.is_whitespace())
                .unwrap_or(true)
        {
            continue;
        }

        let remaining = &message[index + ch.len_utf8()..];
        let remaining_lower = remaining.to_lowercase();
        let matched = candidates.iter().find(|(label, label_lower, _)| {
            if !remaining_lower.starts_with(label_lower) {
                return false;
            }

            let next = remaining.chars().nth(label.chars().count());
            next.map(is_group_chat_mention_boundary).unwrap_or(true)
        });

        let Some((_, _, agent_id)) = matched else {
            continue;
        };

        if targets.iter().any(|id| id == agent_id) {
            continue;
        }
        targets.push(agent_id.clone());
    }

    targets
}

fn is_group_chat_mention_boundary(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | '.'
                | '!'
                | '?'
                | ';'
                | ':'
                | '，'
                | '。'
                | '！'
                | '？'
                | '；'
                | '：'
                | '、'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '"'
                | '\''
        )
}

fn now_ts() -> i64 {
    Utc::now().timestamp_millis()
}

fn new_group_message_id() -> String {
    Uuid::new_v4().to_string()
}

fn new_group_user_message(
    content: String,
    attachments: Vec<GroupAttachment>,
    mentioned_agent_ids: Vec<String>,
) -> GroupChatMessage {
    GroupChatMessage::User {
        id: new_group_message_id(),
        content,
        attachments,
        mentioned_agent_ids,
        timestamp: now_ts(),
    }
}

fn new_group_agent_message(agent_id: String, content: String) -> GroupChatMessage {
    GroupChatMessage::Agent {
        id: new_group_message_id(),
        agent_id,
        content,
        timestamp: now_ts(),
    }
}

fn new_group_system_message(
    content: impl Into<String>,
    level: GroupSystemLevel,
) -> GroupChatMessage {
    GroupChatMessage::System {
        id: new_group_message_id(),
        content: content.into(),
        level,
        timestamp: now_ts(),
    }
}

fn new_group_task_message(
    task_id: &str,
    agent_id: String,
    command: String,
    status: GroupTaskStatus,
    summary: Option<String>,
) -> GroupChatMessage {
    GroupChatMessage::Task {
        id: new_group_message_id(),
        task_id: task_id.to_string(),
        agent_id,
        command,
        status,
        summary,
        timestamp: now_ts(),
    }
}

fn group_message_owner(message: &GroupChatMessage) -> String {
    match message {
        GroupChatMessage::Agent { agent_id, .. } | GroupChatMessage::Task { agent_id, .. } => {
            agent_id.clone()
        }
        GroupChatMessage::Hitl { node_id, .. } => node_id.clone(),
        GroupChatMessage::User { .. } => "user".to_string(),
        GroupChatMessage::System { .. } => "system".to_string(),
    }
}

async fn persist_group_chat_message(
    state: &Arc<AppState>,
    workflow_id: &str,
    message: &GroupChatMessage,
) -> Result<(), (StatusCode, String)> {
    let owner = group_message_owner(message);
    let value = serde_json::to_value(message)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .memory_manager
        .write_global(workflow_id, &owner, GROUP_CHAT_MESSAGE_KEY, value)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn persist_group_chat_messages(
    state: &Arc<AppState>,
    workflow_id: &str,
    messages: &[GroupChatMessage],
) -> Result<(), (StatusCode, String)> {
    for message in messages {
        persist_group_chat_message(state, workflow_id, message).await?;
    }
    Ok(())
}

async fn load_group_chat_history(
    state: &Arc<AppState>,
    workflow_id: &str,
) -> Result<Vec<GroupChatMessage>, (StatusCode, String)> {
    let values = state
        .memory_manager
        .query_global_by_key(workflow_id, GROUP_CHAT_MESSAGE_KEY)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    values
        .into_iter()
        .map(|value| {
            serde_json::from_value(value)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        })
        .collect()
}

fn build_routed_group_chat_prompt(
    agent: &AgentNode,
    sender_label: &str,
    body: &str,
    allowed_targets: &[String],
    shared_context: Option<&str>,
) -> String {
    let mut prompt = format!(
        "[Incoming team message / 收到团队消息]\nYou are {label}.\nFrom: {sender_label}\nContent:\n{body}\n\n[Allowed targets / 允许目标]\n{targets}\n\n[Return schema reminder / 返回格式提醒]\n{{\"target\":\"<目标>\",\"message\":\"<简短回复>\",\"done\":<true|false>}}",
        label = agent.label,
        targets = allowed_targets.join(", "),
    );

    if let Some(context) = shared_context.filter(|value| !value.trim().is_empty()) {
        prompt.push_str("\n\n[Shared document context / 共享文档上下文]\n");
        prompt.push_str(context);
    }

    prompt
}

fn build_group_chat_system_prompt(base_prompt: &str) -> String {
    let trimmed = base_prompt.trim();
    if trimmed.is_empty() {
        GROUP_CHAT_SYSTEM_PROMPT_SUFFIX.to_string()
    } else {
        format!(
            "{trimmed}\n\n[Default Group Chat Prompt / 默认群聊提示词]\n{GROUP_CHAT_SYSTEM_PROMPT_SUFFIX}"
        )
    }
}

fn build_recent_chat_context(messages: &[GroupChatMessage]) -> Option<String> {
    let entries: Vec<String> = messages
        .iter()
        .filter_map(|message| match message {
            GroupChatMessage::User { content, .. } if !content.trim_start().starts_with('/') => {
                Some(format!("[User]\n{}", content.trim()))
            }
            GroupChatMessage::Agent {
                agent_id, content, ..
            } => Some(format!("[Agent: {}]\n{}", agent_id, content.trim())),
            _ => None,
        })
        .filter(|entry| !entry.trim().is_empty())
        .collect();

    if entries.is_empty() {
        return None;
    }

    let mut recent = entries.into_iter().rev().take(8).collect::<Vec<_>>();
    recent.reverse();

    Some(truncate_for_context(&recent.join("\n\n"), 4_000))
}

fn is_chat_participant(agent: &AgentNode) -> bool {
    agent.role != "human_in_loop"
}

fn build_allowed_targets(graph: &WorkflowGraph, current_agent_id: &str) -> Vec<String> {
    let mut targets = vec![USER_TARGET_LABEL.to_string()];
    let mut peers = graph
        .nodes
        .values()
        .filter(|agent| is_chat_participant(agent) && agent.id != current_agent_id)
        .map(|agent| agent.label.clone())
        .collect::<Vec<_>>();
    peers.sort();
    targets.extend(peers);
    targets
}

fn parse_group_reply_protocol(
    graph: &WorkflowGraph,
    current_agent_id: &str,
    allowed_targets: &[String],
    raw_response: &str,
) -> Result<(GroupReplyTarget, String, bool), String> {
    let json_text = extract_group_reply_json(raw_response)
        .ok_or_else(|| "回复不是合法 JSON 对象".to_string())?;
    let envelope: GroupReplyEnvelope =
        serde_json::from_str(json_text).map_err(|e| format!("JSON 解析失败: {}", e))?;

    let normalized_target = envelope.target.trim();
    if normalized_target.is_empty() {
        return Err("target 不能为空".to_string());
    }
    if !allowed_targets
        .iter()
        .any(|target| target == normalized_target)
    {
        return Err(format!("target 不在允许列表中: {}", normalized_target));
    }

    let message = envelope.message.trim().to_string();
    if message.is_empty() {
        return Err("message 不能为空".to_string());
    }

    let lowered = normalized_target.to_ascii_lowercase();
    let target = if matches!(normalized_target, "我" | "用户")
        || matches!(lowered.as_str(), "user" | "me")
    {
        GroupReplyTarget::User
    } else {
        let agent_id = find_agent_id(graph, normalized_target)
            .ok_or_else(|| format!("找不到目标 Agent: {}", normalized_target))?;
        if agent_id == current_agent_id {
            return Err("target 不能指向自己".to_string());
        }
        GroupReplyTarget::Agent(agent_id)
    };

    if envelope.done && !matches!(target, GroupReplyTarget::User) {
        return Err("done=true 时 target 必须是 我".to_string());
    }

    Ok((target, message, envelope.done))
}

fn extract_group_reply_json(raw_response: &str) -> Option<&str> {
    let trimmed = raw_response.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }

    if trimmed.starts_with("```") && trimmed.ends_with("```") {
        let inner = trimmed.trim_start_matches("```");
        let inner = inner
            .strip_prefix("json")
            .or_else(|| inner.strip_prefix("JSON"))
            .unwrap_or(inner);
        let inner = inner.trim();
        let inner = inner.strip_suffix("```")?.trim();
        if inner.starts_with('{') && inner.ends_with('}') {
            return Some(inner);
        }
    }

    None
}

fn render_group_reply(target: &GroupReplyTarget, graph: &WorkflowGraph, body: &str) -> String {
    let target_label = match target {
        GroupReplyTarget::User => USER_TARGET_LABEL.to_string(),
        GroupReplyTarget::Agent(agent_id) => graph
            .nodes
            .get(agent_id)
            .map(|agent| agent.label.clone())
            .unwrap_or_else(|| agent_id.clone()),
    };

    if body.trim().is_empty() {
        format!("@{}", target_label)
    } else {
        format!("@{} {}", target_label, body.trim())
    }
}

fn is_terminal_group_reply(body: &str) -> bool {
    let normalized = body
        .trim()
        .trim_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, '.' | '。' | '!' | '！' | '?' | '？' | ',' | '，')
        })
        .to_ascii_lowercase();

    matches!(
        normalized.as_str(),
        "好的" | "了解" | "收到" | "明白" | "ok" | "okay" | "roger"
    )
}

async fn execute_group_chat_routes(
    state: &Arc<AppState>,
    workflow_id: &str,
    graph: &WorkflowGraph,
    initial_targets: Vec<String>,
    message_body: &str,
    shared_context: Option<&str>,
) -> Result<Vec<GroupChatMessage>, (StatusCode, String)> {
    let mut messages = Vec::new();
    let mut queue = VecDeque::new();

    for target_agent_id in initial_targets {
        queue.push_back(PendingGroupDelivery {
            sender_label: USER_TARGET_LABEL.to_string(),
            target_agent_id,
            body: message_body.to_string(),
            hop: 0,
        });
    }

    let mut processed_steps = 0usize;
    while let Some(delivery) = queue.pop_front() {
        if processed_steps >= MAX_GROUP_CHAT_ROUTE_STEPS {
            messages.push(new_group_system_message(
                "团队沟通达到最大轮次，已自动停止，避免循环对话。",
                GroupSystemLevel::Warning,
            ));
            break;
        }
        processed_steps += 1;

        let Some(agent) = graph.nodes.get(&delivery.target_agent_id).cloned() else {
            continue;
        };
        let mut effective_agent = agent.clone();
        effective_agent.model_config.system_prompt =
            build_group_chat_system_prompt(&agent.model_config.system_prompt);

        let task_id = new_group_message_id();
        messages.push(new_group_task_message(
            &task_id,
            agent.id.clone(),
            format!("来自 {}: {}", delivery.sender_label, delivery.body),
            GroupTaskStatus::Queued,
            Some("Agent 已收到消息".to_string()),
        ));

        let executor = state.executor_pool.executor();
        let prompt_message = build_routed_group_chat_prompt(
            &agent,
            &delivery.sender_label,
            &delivery.body,
            &build_allowed_targets(graph, &agent.id),
            shared_context,
        );
        let stored_message =
            if let Some(context) = shared_context.filter(|value| !value.trim().is_empty()) {
                format!(
                    "From {}:\n{}",
                    delivery.sender_label,
                    build_message_with_attachment_context(&delivery.body, Some(context))
                )
            } else {
                format!("From {}:\n{}", delivery.sender_label, delivery.body)
            };
        let history_key = format!("group_{}_{}", workflow_id, agent.id);

        let result = match &agent.kind {
            AgentKind::RawLlm => {
                let history: Vec<Value> = state
                    .chat_histories
                    .get(&history_key)
                    .map(|h| h.value().clone())
                    .unwrap_or_default();

                match executor
                    .chat(&effective_agent, &history, &prompt_message)
                    .await
                {
                    Ok(response) => {
                        let mut new_history = history;
                        new_history.push(json!({ "role": "user", "content": stored_message }));
                        new_history
                            .push(json!({ "role": "assistant", "content": response.clone() }));
                        state.chat_histories.insert(history_key, new_history);
                        Ok((response, None, false))
                    }
                    Err(err) => Err(err.to_string()),
                }
            }
            _ => {
                let existing_handle = state
                    .group_native_sessions
                    .get(&history_key)
                    .filter(|session| session.kind == native_kind_key(&agent.kind))
                    .map(|session| session.handle.clone());

                match executor
                    .chat_native_with_session(
                        &effective_agent,
                        &prompt_message,
                        existing_handle.as_deref(),
                    )
                    .await
                {
                    Ok(result) => {
                        if let Some(handle) = result.session_handle.clone() {
                            state.group_native_sessions.insert(
                                history_key,
                                crate::app_state::GroupNativeSession {
                                    kind: native_kind_key(&agent.kind).to_string(),
                                    handle,
                                },
                            );
                        }

                        let authorization_warning = result.authorization_required.map(|auth| {
                            let details = auth
                                .details
                                .map(|details| format!("\n{}", details))
                                .unwrap_or_default();
                            format!("{}{}", auth.message, details)
                        });

                        Ok((
                            result.output,
                            authorization_warning.clone(),
                            authorization_warning.is_some(),
                        ))
                    }
                    Err(err) => Err(err.to_string()),
                }
            }
        };

        match result {
            Ok((raw_response, authorization_warning, blocked)) => {
                if blocked {
                    messages.push(new_group_task_message(
                        &task_id,
                        agent.id.clone(),
                        format!("来自 {}: {}", delivery.sender_label, delivery.body),
                        GroupTaskStatus::Blocked,
                        authorization_warning
                            .as_ref()
                            .cloned()
                            .or_else(|| Some("Agent 需要授权".to_string())),
                    ));
                    if let Some(warning) = authorization_warning {
                        messages.push(new_group_system_message(
                            format!("{}: {}", agent.label, warning),
                            GroupSystemLevel::Warning,
                        ));
                    }
                    continue;
                }

                let allowed_targets = build_allowed_targets(graph, &agent.id);
                let (target, reply_body, done) = match parse_group_reply_protocol(
                    graph,
                    &agent.id,
                    &allowed_targets,
                    &raw_response,
                ) {
                    Ok(parsed) => parsed,
                    Err(err) => {
                        messages.push(new_group_task_message(
                            &task_id,
                            agent.id.clone(),
                            format!("来自 {}: {}", delivery.sender_label, delivery.body),
                            GroupTaskStatus::Failed,
                            Some(format!("回复协议无效: {}", err)),
                        ));
                        messages.push(new_group_system_message(
                            format!("{} 回复协议无效: {}", agent.label, err),
                            GroupSystemLevel::Warning,
                        ));
                        continue;
                    }
                };

                let rendered_reply = render_group_reply(&target, graph, &reply_body);
                messages.push(new_group_task_message(
                    &task_id,
                    agent.id.clone(),
                    format!("来自 {}: {}", delivery.sender_label, delivery.body),
                    GroupTaskStatus::Completed,
                    authorization_warning
                        .as_ref()
                        .cloned()
                        .or_else(|| Some("Agent 已完成回复".to_string())),
                ));

                if !rendered_reply.trim().is_empty() {
                    messages.push(new_group_agent_message(agent.id.clone(), rendered_reply));
                }

                if done || is_terminal_group_reply(&reply_body) {
                    continue;
                }

                if let GroupReplyTarget::Agent(target_agent_id) = target {
                    queue.push_back(PendingGroupDelivery {
                        sender_label: agent.label.clone(),
                        target_agent_id,
                        body: reply_body,
                        hop: delivery.hop + 1,
                    });
                }
            }
            Err(err) => {
                messages.push(new_group_task_message(
                    &task_id,
                    agent.id.clone(),
                    format!("来自 {}: {}", delivery.sender_label, delivery.body),
                    GroupTaskStatus::Failed,
                    Some(err.clone()),
                ));
                messages.push(new_group_system_message(
                    format!("{} 回复失败: {}", agent.label, err),
                    GroupSystemLevel::Warning,
                ));
            }
        }
    }

    Ok(messages)
}

fn prepare_attachment_context(
    attachments: &[GroupAttachment],
) -> (Option<String>, Vec<GroupChatMessage>) {
    let parsed = parse_attachments(attachments);
    let warnings = parsed
        .warnings
        .into_iter()
        .map(|warning| new_group_system_message(warning, GroupSystemLevel::Warning))
        .collect();
    (render_attachment_context(&parsed.contexts), warnings)
}

fn build_message_with_attachment_context(
    message: &str,
    attachment_context: Option<&str>,
) -> String {
    match attachment_context {
        Some(context) if !context.trim().is_empty() && !message.trim().is_empty() => {
            format!("{message}\n\n[Document context / 文档上下文]\n{context}")
        }
        Some(context) if !context.trim().is_empty() => {
            format!("[Document context / 文档上下文]\n{context}")
        }
        _ => message.to_string(),
    }
}

fn build_run_initial_context(
    recent_chat_context: Option<&str>,
    run_instruction: &str,
    attachment_context: Option<&str>,
) -> Option<String> {
    let mut sections = Vec::new();

    if let Some(context) = recent_chat_context.filter(|value| !value.trim().is_empty()) {
        sections.push(format!(
            "[Recent team context / 最近团队上下文]\n{}",
            context
        ));
    }

    if !run_instruction.trim().is_empty() {
        sections.push(format!(
            "[Run instruction / 执行说明]\n{}",
            run_instruction.trim()
        ));
    }

    if let Some(context) = attachment_context.filter(|value| !value.trim().is_empty()) {
        sections.push(format!("[Document context / 文档上下文]\n{}", context));
    }

    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

fn truncate_for_context(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let truncated: String = value.chars().take(max_chars).collect();
    format!("{truncated}\n...[truncated]")
}

fn split_command_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in input.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

fn parse_command_pairs(tokens: &[String]) -> HashMap<String, String> {
    let mut params = HashMap::new();
    for token in tokens {
        if let Some((key, value)) = token.split_once('=') {
            params.insert(key.to_lowercase(), value.to_string());
        }
    }
    params
}

fn parse_agent_kind(value: &str) -> Option<AgentKind> {
    match value {
        "raw_llm" => Some(AgentKind::RawLlm),
        "claude_code" => Some(AgentKind::ClaudeCode),
        "gemini_cli" => Some(AgentKind::GeminiCli),
        "codex" => Some(AgentKind::Codex),
        _ => None,
    }
}

fn parse_model_provider(value: &str, model: String) -> Option<ModelProvider> {
    match value {
        "claude" => Some(ModelProvider::Claude(model)),
        "gemini" => Some(ModelProvider::Gemini(model)),
        "openai" => Some(ModelProvider::OpenAI(model)),
        _ => None,
    }
}

fn default_provider_for_kind(kind: &AgentKind) -> ModelProvider {
    match kind {
        AgentKind::RawLlm | AgentKind::ClaudeCode => ModelProvider::default_claude(),
        AgentKind::GeminiCli => ModelProvider::default_gemini(),
        AgentKind::Codex => ModelProvider::default_openai(),
    }
}

fn find_agent_id(graph: &WorkflowGraph, token: &str) -> Option<String> {
    let needle = token.trim().trim_start_matches('@');
    if graph.nodes.contains_key(needle) {
        return Some(needle.to_string());
    }

    graph
        .nodes
        .values()
        .find(|node| node.label.eq_ignore_ascii_case(needle))
        .map(|node| node.id.clone())
}

async fn save_workflow_graph(
    state: &Arc<AppState>,
    graph: &WorkflowGraph,
) -> Result<(), (StatusCode, String)> {
    let graph_json =
        serde_json::to_string(graph).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    state
        .memory_manager
        .save_workflow(&graph.id, &graph.name, &graph_json)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn execute_group_chat_command(
    state: &Arc<AppState>,
    workflow_id: &str,
    graph: &WorkflowGraph,
    raw_command: &str,
    attachment_context: Option<&str>,
) -> Result<GroupChatCommandOutcome, (StatusCode, String)> {
    let tokens = split_command_args(raw_command.trim_start_matches('/').trim());
    if tokens.is_empty() {
        return Ok(GroupChatCommandOutcome {
            messages: vec![new_group_system_message(
                "空命令。输入 /help 查看可用聊天命令。",
                GroupSystemLevel::Warning,
            )],
            workflow_graph: None,
        });
    }

    let command = tokens[0].to_lowercase();
    let mut graph = graph.clone();

    let outcome = match command.as_str() {
        "help" => GroupChatCommandOutcome {
            messages: vec![new_group_system_message(
                "可用命令:\n/help\n/run [执行说明]\n/rename-workflow <新名称>\n/add-agent label=<名称> [role=<角色>] [kind=<raw_llm|claude_code|gemini_cli|codex>] [provider=<Claude|Gemini|OpenAI>] [model=<模型>]\n/remove-agent <标签或ID>\n/connect <源节点> <目标节点>\n/disconnect <源节点> <目标节点>\n/set-role <标签或ID> <新角色>",
                GroupSystemLevel::Info,
            )],
            workflow_graph: None,
        },
        "run" => {
            let run_instruction = tokens[1..].join(" ");
            let recent_chat_context = build_recent_chat_context(
                &load_group_chat_history(state, workflow_id).await?,
            );
            let initial_context = build_run_initial_context(
                recent_chat_context.as_deref(),
                &run_instruction,
                attachment_context,
            );

            save_workflow_graph(state, &graph).await?;
            match state
                .workflow_engine
                .start_workflow_with_input(graph.id.clone(), graph.clone(), initial_context)
                .await
            {
                Ok(()) => GroupChatCommandOutcome {
                    messages: vec![new_group_system_message(
                        "工作流已启动。当前聊天上下文会作为根节点输入。",
                        GroupSystemLevel::Info,
                    )],
                    workflow_graph: None,
                },
                Err(err) => GroupChatCommandOutcome {
                    messages: vec![new_group_system_message(
                        format!("工作流启动失败: {}", err),
                        GroupSystemLevel::Warning,
                    )],
                    workflow_graph: None,
                },
            }
        }
        "rename-workflow" => {
            let new_name = tokens[1..].join(" ").trim().to_string();
            if new_name.is_empty() {
                GroupChatCommandOutcome {
                    messages: vec![new_group_system_message(
                        "用法: /rename-workflow <新名称>",
                        GroupSystemLevel::Warning,
                    )],
                    workflow_graph: None,
                }
            } else {
                graph.name = new_name.clone();
                save_workflow_graph(state, &graph).await?;
                GroupChatCommandOutcome {
                    messages: vec![new_group_system_message(
                        format!("工作流已重命名为: {}", new_name),
                        GroupSystemLevel::Info,
                    )],
                    workflow_graph: Some(graph),
                }
            }
        }
        "add-agent" => {
            let params = parse_command_pairs(&tokens[1..]);
            let label = match params.get("label").map(String::as_str) {
                Some(label) if !label.is_empty() => label.to_string(),
                _ => {
                    return Ok(GroupChatCommandOutcome {
                        messages: vec![new_group_system_message(
                            "用法: /add-agent label=<名称> [role=<角色>] [kind=<raw_llm|claude_code|gemini_cli|codex>] [provider=<Claude|Gemini|OpenAI>] [model=<模型>]",
                            GroupSystemLevel::Warning,
                        )],
                        workflow_graph: None,
                    })
                }
            };

            let role = params
                .get("role")
                .cloned()
                .unwrap_or_else(|| "worker".to_string());
            let kind = params
                .get("kind")
                .and_then(|value| parse_agent_kind(value))
                .unwrap_or(AgentKind::RawLlm);
            let provider = if let Some(provider_name) = params.get("provider") {
                let model = params
                    .get("model")
                    .cloned()
                    .unwrap_or_else(|| match provider_name.to_lowercase().as_str() {
                        "claude" => "claude-opus-4-6".to_string(),
                        "gemini" => "gemini-2.0-flash".to_string(),
                        "openai" => "gpt-4o".to_string(),
                        _ => "claude-opus-4-6".to_string(),
                    });
                parse_model_provider(&provider_name.to_lowercase(), model)
                    .unwrap_or_else(|| default_provider_for_kind(&kind))
            } else {
                let mut provider = default_provider_for_kind(&kind);
                if let Some(model) = params.get("model") {
                    provider = match provider {
                        ModelProvider::Claude(_) => ModelProvider::Claude(model.clone()),
                        ModelProvider::Gemini(_) => ModelProvider::Gemini(model.clone()),
                        ModelProvider::OpenAI(_) => ModelProvider::OpenAI(model.clone()),
                    };
                }
                provider
            };

            let mut node = AgentNode::new(label.clone(), role.clone(), provider);
            node.kind = kind;
            graph.add_node(node);
            save_workflow_graph(state, &graph).await?;

            GroupChatCommandOutcome {
                messages: vec![new_group_system_message(
                    format!("已添加 Agent: {} ({})", label, role),
                    GroupSystemLevel::Info,
                )],
                workflow_graph: Some(graph),
            }
        }
        "remove-agent" => {
            let Some(target) = tokens.get(1).and_then(|value| find_agent_id(&graph, value)) else {
                return Ok(GroupChatCommandOutcome {
                    messages: vec![new_group_system_message(
                        "用法: /remove-agent <标签或ID>",
                        GroupSystemLevel::Warning,
                    )],
                    workflow_graph: None,
                });
            };

            let removed = graph.nodes.remove(&target);
            graph.edges.retain(|edge| edge.source != target && edge.target != target);
            save_workflow_graph(state, &graph).await?;

            GroupChatCommandOutcome {
                messages: vec![new_group_system_message(
                    format!(
                        "已移除 Agent: {}",
                        removed.map(|node| node.label).unwrap_or(target)
                    ),
                    GroupSystemLevel::Info,
                )],
                workflow_graph: Some(graph),
            }
        }
        "connect" | "disconnect" => {
            let refs: Vec<&String> = tokens[1..].iter().filter(|token| token.as_str() != "->").collect();
            if refs.len() < 2 {
                return Ok(GroupChatCommandOutcome {
                    messages: vec![new_group_system_message(
                        format!("用法: /{} <源节点> <目标节点>", command),
                        GroupSystemLevel::Warning,
                    )],
                    workflow_graph: None,
                });
            }

            let Some(source_id) = find_agent_id(&graph, refs[0]) else {
                return Ok(GroupChatCommandOutcome {
                    messages: vec![new_group_system_message("未找到源节点。", GroupSystemLevel::Warning)],
                    workflow_graph: None,
                });
            };
            let Some(target_id) = find_agent_id(&graph, refs[1]) else {
                return Ok(GroupChatCommandOutcome {
                    messages: vec![new_group_system_message("未找到目标节点。", GroupSystemLevel::Warning)],
                    workflow_graph: None,
                });
            };

            let source_label = graph
                .nodes
                .get(&source_id)
                .map(|node| node.label.clone())
                .unwrap_or_else(|| source_id.clone());
            let target_label = graph
                .nodes
                .get(&target_id)
                .map(|node| node.label.clone())
                .unwrap_or_else(|| target_id.clone());

            let info_message = if command == "connect" {
                if graph
                    .edges
                    .iter()
                    .any(|edge| edge.source == source_id && edge.target == target_id)
                {
                    "该连接已存在。".to_string()
                } else {
                    graph.add_edge(DirectedEdge::new(source_id.clone(), target_id.clone()));
                    save_workflow_graph(state, &graph).await?;
                    format!("已连接 {} -> {}", source_label, target_label)
                }
            } else {
                let before = graph.edges.len();
                graph
                    .edges
                    .retain(|edge| !(edge.source == source_id && edge.target == target_id));
                if graph.edges.len() == before {
                    "没有找到对应的连接。".to_string()
                } else {
                    save_workflow_graph(state, &graph).await?;
                    format!("已断开 {} -> {}", source_label, target_label)
                }
            };

            GroupChatCommandOutcome {
                messages: vec![new_group_system_message(info_message, GroupSystemLevel::Info)],
                workflow_graph: Some(graph),
            }
        }
        "set-role" => {
            if tokens.len() < 3 {
                return Ok(GroupChatCommandOutcome {
                    messages: vec![new_group_system_message(
                        "用法: /set-role <标签或ID> <新角色>",
                        GroupSystemLevel::Warning,
                    )],
                    workflow_graph: None,
                });
            }

            let Some(target) = find_agent_id(&graph, &tokens[1]) else {
                return Ok(GroupChatCommandOutcome {
                    messages: vec![new_group_system_message("未找到目标 Agent。", GroupSystemLevel::Warning)],
                    workflow_graph: None,
                });
            };
            let new_role = tokens[2..].join(" ");
            if let Some(node) = graph.get_node_mut(&target) {
                node.role = new_role.clone();
                let label = node.label.clone();
                save_workflow_graph(state, &graph).await?;
                GroupChatCommandOutcome {
                    messages: vec![new_group_system_message(
                        format!("{} 的角色已更新为 {}", label, new_role),
                        GroupSystemLevel::Info,
                    )],
                    workflow_graph: Some(graph),
                }
            } else {
                GroupChatCommandOutcome {
                    messages: vec![new_group_system_message("未找到目标 Agent。", GroupSystemLevel::Warning)],
                    workflow_graph: None,
                }
            }
        }
        _ => GroupChatCommandOutcome {
            messages: vec![new_group_system_message(
                format!("未知命令: /{}。输入 /help 查看支持的命令。", command),
                GroupSystemLevel::Warning,
            )],
            workflow_graph: None,
        },
    };

    Ok(outcome)
}

async fn group_chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GroupChatRequest>,
) -> Result<Json<GroupChatResponse>, (StatusCode, String)> {
    let mut response_messages = Vec::new();
    let user_message = new_group_user_message(
        req.message.clone(),
        req.attachments.clone().unwrap_or_default(),
        req.mentioned_agent_ids.clone(),
    );
    persist_group_chat_message(&state, &req.workflow_id, &user_message).await?;
    response_messages.push(user_message);

    let attachments = req.attachments.clone().unwrap_or_default();

    if req.message.trim_start().starts_with('/') {
        let is_run_command = req
            .message
            .trim_start()
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .map(|command| command.eq_ignore_ascii_case("run"))
            .unwrap_or(false);

        let (attachment_context, mut attachment_messages) = if is_run_command {
            prepare_attachment_context(&attachments)
        } else {
            (None, Vec::new())
        };
        let outcome = execute_group_chat_command(
            &state,
            &req.workflow_id,
            &req.graph,
            req.message.trim(),
            attachment_context.as_deref(),
        )
        .await?;

        attachment_messages.extend(outcome.messages);
        persist_group_chat_messages(&state, &req.workflow_id, &attachment_messages).await?;
        response_messages.extend(attachment_messages);
        return Ok(Json(GroupChatResponse {
            messages: response_messages,
            workflow_graph: outcome.workflow_graph,
        }));
    }

    let (attachment_context, attachment_messages) = prepare_attachment_context(&attachments);
    if !attachment_messages.is_empty() {
        persist_group_chat_messages(&state, &req.workflow_id, &attachment_messages).await?;
        response_messages.extend(attachment_messages);
    }

    let target_agent_ids =
        collect_group_chat_targets(&req.mentioned_agent_ids, &req.message, &req.graph.nodes);
    if target_agent_ids.is_empty() {
        let warning = new_group_system_message(
            "当前没有可参与群聊的 Agent。先创建 Agent，或输入 /help 查看聊天命令。",
            GroupSystemLevel::Warning,
        );
        persist_group_chat_message(&state, &req.workflow_id, &warning).await?;
        response_messages.push(warning);
        return Ok(Json(GroupChatResponse {
            messages: response_messages,
            workflow_graph: None,
        }));
    }

    if req.mentioned_agent_ids.is_empty() {
        let broadcast = new_group_system_message(
            "未指定 @对象，已广播给全部 Agent。Agent 回复必须带 @目标。",
            GroupSystemLevel::Info,
        );
        persist_group_chat_message(&state, &req.workflow_id, &broadcast).await?;
        response_messages.push(broadcast);
    }

    let routed_messages = execute_group_chat_routes(
        &state,
        &req.workflow_id,
        &req.graph,
        target_agent_ids,
        &req.message,
        attachment_context.as_deref(),
    )
    .await?;
    persist_group_chat_messages(&state, &req.workflow_id, &routed_messages).await?;
    response_messages.extend(routed_messages);

    Ok(Json(GroupChatResponse {
        messages: response_messages,
        workflow_graph: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as StdHashMap;

    #[test]
    fn collect_group_chat_targets_only_keeps_valid_unique_mentions() {
        let alpha = AgentNode::new(
            "alpha".to_string(),
            "analyst",
            ModelProvider::default_claude(),
        );
        let beta = AgentNode::new(
            "beta".to_string(),
            "reviewer",
            ModelProvider::default_claude(),
        );

        let mut agents = StdHashMap::new();
        agents.insert(alpha.id.clone(), alpha.clone());
        agents.insert(beta.id.clone(), beta.clone());

        let targets = collect_group_chat_targets(
            &[
                alpha.id.clone(),
                "missing".to_string(),
                alpha.id.clone(),
                beta.id.clone(),
            ],
            "",
            &agents,
        );

        assert_eq!(targets, vec![alpha.id, beta.id]);
    }

    #[test]
    fn collect_group_chat_targets_without_mentions_broadcasts_all_participants() {
        let alpha = AgentNode::new(
            "alpha".to_string(),
            "analyst",
            ModelProvider::default_claude(),
        );
        let beta = AgentNode::new(
            "beta".to_string(),
            "reviewer",
            ModelProvider::default_claude(),
        );
        let hitl = AgentNode::new(
            "审批".to_string(),
            "human_in_loop",
            ModelProvider::default_claude(),
        );

        let mut agents = StdHashMap::new();
        agents.insert(alpha.id.clone(), alpha.clone());
        agents.insert(beta.id.clone(), beta.clone());
        agents.insert(hitl.id.clone(), hitl);

        let targets = collect_group_chat_targets(&[], "", &agents);

        assert_eq!(targets, vec![alpha.id, beta.id]);
    }

    #[test]
    fn collect_group_chat_targets_parses_manual_mentions_with_spaces() {
        let alpha = AgentNode::new(
            "Planner".to_string(),
            "analyst",
            ModelProvider::default_claude(),
        );
        let mut beta = AgentNode::new(
            "Project Manager".to_string(),
            "reviewer",
            ModelProvider::default_claude(),
        );
        beta.label = "Project Manager".to_string();

        let mut agents = StdHashMap::new();
        agents.insert(alpha.id.clone(), alpha);
        agents.insert(beta.id.clone(), beta.clone());

        let targets = collect_group_chat_targets(&[], "@Project Manager 请处理排期。", &agents);

        assert_eq!(targets, vec![beta.id]);
    }

    #[test]
    fn collect_group_chat_targets_prefers_labels_found_in_message_over_stale_ids() {
        let alpha = AgentNode::new(
            "Planner".to_string(),
            "analyst",
            ModelProvider::default_claude(),
        );
        let mut beta = AgentNode::new(
            "Project Manager".to_string(),
            "reviewer",
            ModelProvider::default_claude(),
        );
        beta.label = "Project Manager".to_string();

        let mut agents = StdHashMap::new();
        agents.insert(alpha.id.clone(), alpha.clone());
        agents.insert(beta.id.clone(), beta.clone());

        let targets = collect_group_chat_targets(
            &[alpha.id.clone()],
            "@Project Manager 请处理排期。",
            &agents,
        );

        assert_eq!(targets, vec![beta.id]);
    }

    #[test]
    fn build_routed_group_chat_prompt_enforces_at_target_and_concise_reply() {
        let agent = AgentNode::new(
            "planner".to_string(),
            "coordinator",
            ModelProvider::default_claude(),
        );
        let prompt = build_routed_group_chat_prompt(
            &agent,
            USER_TARGET_LABEL,
            "总结一下方案",
            &[USER_TARGET_LABEL.to_string(), "reviewer".to_string()],
            None,
        );

        assert!(prompt.contains("You are planner."));
        assert!(prompt.contains("Return schema reminder"));
        assert!(prompt.contains("\"target\":\"<目标>\""));
        assert!(prompt.contains("[Allowed targets / 允许目标]"));
        assert!(prompt.contains("我, reviewer"));
    }

    #[test]
    fn build_group_chat_system_prompt_appends_default_suffix_after_base_prompt() {
        let prompt = build_group_chat_system_prompt("You are a hiring coordinator.");

        assert!(prompt.starts_with("You are a hiring coordinator."));
        assert!(prompt.contains("[Default Group Chat Prompt / 默认群聊提示词]"));
        assert!(prompt.contains("你正在团队群聊中协作"));
        assert!(prompt
            .contains("{\"target\":\"<目标>\",\"message\":\"<简短回复>\",\"done\":<true|false>}"));
    }

    #[test]
    fn split_command_args_supports_quotes() {
        let args = split_command_args(r#"add-agent label="项目经理" role=lead kind=claude_code"#);
        assert_eq!(
            args,
            vec![
                "add-agent".to_string(),
                "label=项目经理".to_string(),
                "role=lead".to_string(),
                "kind=claude_code".to_string()
            ]
        );
    }

    #[test]
    fn find_agent_id_matches_label_without_at_prefix() {
        let mut graph = WorkflowGraph::new("demo".to_string());
        let node = AgentNode::new(
            "项目经理".to_string(),
            "lead",
            ModelProvider::default_claude(),
        );
        let id = node.id.clone();
        graph.add_node(node);

        assert_eq!(find_agent_id(&graph, "@项目经理"), Some(id));
    }

    #[test]
    fn build_recent_chat_context_skips_commands_and_system_messages() {
        let context = build_recent_chat_context(&[
            new_group_user_message("/run".to_string(), vec![], vec![]),
            new_group_system_message("ignored", GroupSystemLevel::Info),
            new_group_user_message("梳理上线计划".to_string(), vec![], vec![]),
            new_group_agent_message("planner".to_string(), "先列里程碑".to_string()),
        ])
        .expect("context");

        assert!(context.contains("梳理上线计划"));
        assert!(context.contains("先列里程碑"));
        assert!(!context.contains("/run"));
        assert!(!context.contains("ignored"));
    }

    #[test]
    fn build_run_initial_context_combines_recent_chat_instruction_and_documents() {
        let context = build_run_initial_context(
            Some("[User]\n先讨论招聘流程"),
            "输出执行计划",
            Some("[Document: brief.md | text/markdown]\n项目背景"),
        )
        .expect("context");

        assert!(context.contains("最近团队上下文"));
        assert!(context.contains("输出执行计划"));
        assert!(context.contains("项目背景"));
    }

    #[test]
    fn build_message_with_attachment_context_includes_document_block() {
        let message = build_message_with_attachment_context(
            "@项目经理 看下附件",
            Some("[Document: spec.md | text/markdown]\n需求说明"),
        );

        assert!(message.contains("@项目经理 看下附件"));
        assert!(message.contains("文档上下文"));
        assert!(message.contains("需求说明"));
    }

    #[test]
    fn parse_group_reply_protocol_supports_user_and_agent_targets() {
        let mut graph = WorkflowGraph::new("demo".to_string());
        let reviewer = AgentNode::new(
            "reviewer".to_string(),
            "reviewer",
            ModelProvider::default_claude(),
        );
        let reviewer_id = reviewer.id.clone();
        graph.add_node(reviewer);

        let allowed = vec![USER_TARGET_LABEL.to_string(), "reviewer".to_string()];

        let (user_target, user_body, user_done) = parse_group_reply_protocol(
            &graph,
            "planner",
            &allowed,
            r#"{"target":"我","message":"好的","done":true}"#,
        )
        .expect("user target");
        assert!(matches!(user_target, GroupReplyTarget::User));
        assert_eq!(user_body, "好的");
        assert!(user_done);

        let (agent_target, agent_body, agent_done) = parse_group_reply_protocol(
            &graph,
            "planner",
            &allowed,
            r#"{"target":"reviewer","message":"请复核一下","done":false}"#,
        )
        .expect("agent target");
        assert!(matches!(agent_target, GroupReplyTarget::Agent(id) if id == reviewer_id));
        assert_eq!(agent_body, "请复核一下");
        assert!(!agent_done);
    }

    #[test]
    fn parse_group_reply_protocol_rejects_invalid_targets_and_done_contract() {
        let mut graph = WorkflowGraph::new("demo".to_string());
        let reviewer = AgentNode::new(
            "reviewer".to_string(),
            "reviewer",
            ModelProvider::default_claude(),
        );
        let reviewer_id = reviewer.id.clone();
        graph.add_node(reviewer);
        let allowed = vec![USER_TARGET_LABEL.to_string(), "reviewer".to_string()];

        let self_target_err = parse_group_reply_protocol(
            &graph,
            &reviewer_id,
            &allowed,
            r#"{"target":"reviewer","message":"我来继续","done":false}"#,
        )
        .expect_err("self target should fail");
        assert!(self_target_err.contains("target 不能指向自己"));

        let done_err = parse_group_reply_protocol(
            &graph,
            "planner",
            &allowed,
            r#"{"target":"reviewer","message":"结束","done":true}"#,
        )
        .expect_err("done must point to user");
        assert!(done_err.contains("done=true"));
    }

    #[test]
    fn terminal_group_reply_detects_acknowledgements() {
        assert!(is_terminal_group_reply("好的"));
        assert!(is_terminal_group_reply("了解。"));
        assert!(is_terminal_group_reply("OK"));
        assert!(!is_terminal_group_reply("继续跟进"));
    }

    #[test]
    fn ensure_bypass_permission_args_adds_defaults_for_supported_ptys() {
        assert_eq!(
            ensure_bypass_permission_args("claude", &[]),
            vec!["--permission-mode", "bypassPermissions"]
        );
        assert_eq!(
            ensure_bypass_permission_args("gemini", &[]),
            vec!["--approval-mode", "yolo"]
        );
        assert_eq!(
            ensure_bypass_permission_args("codex", &[]),
            vec!["--dangerously-bypass-approvals-and-sandbox"]
        );
    }

    #[test]
    fn ensure_bypass_permission_args_preserves_explicit_overrides() {
        let claude = ensure_bypass_permission_args(
            "claude",
            &["--dangerously-skip-permissions".to_string()],
        );
        assert_eq!(claude, vec!["--dangerously-skip-permissions"]);

        let gemini = ensure_bypass_permission_args(
            "gemini",
            &["--approval-mode".to_string(), "plan".to_string()],
        );
        assert_eq!(gemini, vec!["--approval-mode", "plan"]);

        let codex = ensure_bypass_permission_args(
            "codex",
            &["--sandbox".to_string(), "workspace-write".to_string()],
        );
        assert_eq!(codex, vec!["--sandbox", "workspace-write"]);
    }
}

fn native_kind_key(kind: &AgentKind) -> &'static str {
    match kind {
        AgentKind::ClaudeCode => "claude_code",
        AgentKind::GeminiCli => "gemini_cli",
        AgentKind::Codex => "codex",
        AgentKind::RawLlm => "raw_llm",
    }
}

// ── PTY session management ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreatePtyRequest {
    agent_id: String,
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "default_cols")]
    cols: u16,
    #[serde(default = "default_rows")]
    rows: u16,
}

fn default_cols() -> u16 {
    220
}
fn default_rows() -> u16 {
    50
}

fn ensure_bypass_permission_args(program: &str, args: &[String]) -> Vec<String> {
    let normalized = program.trim().to_ascii_lowercase();
    let mut merged = args.to_vec();

    let prefix = match normalized.as_str() {
        "claude" => {
            let has_override = merged.iter().any(|arg| {
                arg == "--permission-mode"
                    || arg == "--dangerously-skip-permissions"
                    || arg == "--allow-dangerously-skip-permissions"
            });
            if has_override {
                Vec::new()
            } else {
                vec![
                    "--permission-mode".to_string(),
                    "bypassPermissions".to_string(),
                ]
            }
        }
        "gemini" => {
            let has_override = merged
                .iter()
                .any(|arg| arg == "--approval-mode" || arg == "--yolo" || arg == "-y");
            if has_override {
                Vec::new()
            } else {
                vec!["--approval-mode".to_string(), "yolo".to_string()]
            }
        }
        "codex" => {
            let has_override = merged.iter().any(|arg| {
                arg == "--dangerously-bypass-approvals-and-sandbox"
                    || arg == "--full-auto"
                    || arg == "--ask-for-approval"
                    || arg == "-a"
                    || arg == "--sandbox"
                    || arg == "-s"
            });
            if has_override {
                Vec::new()
            } else {
                vec!["--dangerously-bypass-approvals-and-sandbox".to_string()]
            }
        }
        _ => Vec::new(),
    };

    if prefix.is_empty() {
        return merged;
    }

    let mut output = prefix;
    output.append(&mut merged);
    output
}

async fn create_pty_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreatePtyRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let merged_args = ensure_bypass_permission_args(&req.program, &req.args);
    let args: Vec<&str> = merged_args.iter().map(String::as_str).collect();

    let handle = pty_broker::spawn(
        session_id.clone(),
        req.agent_id.clone(),
        &req.program,
        &args,
        req.cols,
        req.rows,
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state.pty_sessions.insert(session_id.clone(), handle);

    Ok(Json(
        json!({ "session_id": session_id, "agent_id": req.agent_id }),
    ))
}

async fn close_pty_session(
    Path(session_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, StatusCode> {
    if let Some((_, handle)) = state.pty_sessions.remove(&session_id) {
        if let Err(err) = handle.shutdown() {
            warn!("Failed to shutdown PTY session {}: {}", session_id, err);
        }
    }
    Ok(Json(json!({ "session_id": session_id, "closed": true })))
}
