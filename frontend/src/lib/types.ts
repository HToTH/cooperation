// ────────────────────────────────────────────────────────────────────────────
// Domain types mirroring the Rust backend structures
// ────────────────────────────────────────────────────────────────────────────

export type NodeId = string
export type WorkflowId = string

/** Free-form user-defined role label. Use "human_in_loop" to insert a HITL pause. */
export type AgentRole = string

export type ModelProvider =
  | { provider: 'Claude'; model: string }
  | { provider: 'Gemini'; model: string }
  | { provider: 'OpenAI'; model: string }

/**
 * How this agent node is invoked.
 * - raw_llm:     Direct LLM API call
 * - claude_code: Spawns `claude` CLI subprocess
 * - gemini_cli:  Spawns `gemini` CLI subprocess
 * - codex:       Spawns `codex` CLI subprocess (OpenAI Codex)
 */
export type AgentKind =
  | 'raw_llm'
  | 'claude_code'
  | 'gemini_cli'
  | 'codex'

export interface ModelConfig {
  temperature: number
  max_tokens: number
  system_prompt: string
}

export type AgentNodeState = 'idle' | 'running' | 'paused' | 'completed' | 'failed'

export interface NodePosition {
  x: number
  y: number
}

export interface AgentNode {
  id: NodeId
  label: string
  role: AgentRole
  model: ModelProvider
  kind: AgentKind
  model_config: ModelConfig
  context_pool_id: string
  state: AgentNodeState
  position: NodePosition
}

export interface DirectedEdge {
  id: string
  source: NodeId
  target: NodeId
  label?: string
}

export interface WorkflowGraph {
  id: WorkflowId
  name: string
  nodes: Record<NodeId, AgentNode>
  edges: DirectedEdge[]
}

// ────────────────────────────────────────────────────────────────────────────
// Protocol types
// ────────────────────────────────────────────────────────────────────────────

export interface AgentIdentity {
  id: NodeId
  role: string
}

export interface AgentMessage {
  protocol_version: string
  message_id: string
  from_agent: AgentIdentity
  to_agent: AgentIdentity
  message_type: string
  payload: unknown
  in_reply_to?: string
}

export interface ExecutionMetadata {
  tokens_used: number
  context_pool_id: string
  duration_ms: number
}

export interface TaskResultPayload {
  status: 'completed' | 'failed' | 'partial'
  result: unknown
  error?: string
  execution_metadata: ExecutionMetadata
}

// ────────────────────────────────────────────────────────────────────────────
// WebSocket Commands (frontend → backend)
// ────────────────────────────────────────────────────────────────────────────

export type HitlDecision =
  | 'approved'
  | { rejected: { reason: string } }

export type WsCommand =
  | { type: 'start_workflow'; payload: { workflow_id: WorkflowId; graph: WorkflowGraph } }
  | { type: 'stop_workflow'; payload: { workflow_id: WorkflowId } }
  | { type: 'update_graph'; payload: { workflow_id: WorkflowId; graph: WorkflowGraph } }
  | { type: 'hitl_resume'; payload: { workflow_id: WorkflowId; node_id?: NodeId; decision: HitlDecision } }
  | { type: 'query_global_memory'; payload: { workflow_id: WorkflowId; query: string } }

// ────────────────────────────────────────────────────────────────────────────
// WebSocket Events (backend → frontend)
// ────────────────────────────────────────────────────────────────────────────

export type WsEvent =
  | { type: 'workflow_state_changed'; payload: { workflow_id: WorkflowId; state: string } }
  | { type: 'node_state_changed'; payload: { workflow_id: WorkflowId; node_id: NodeId; state: AgentNodeState } }
  | { type: 'agent_message_sent'; payload: { workflow_id: WorkflowId; message: AgentMessage } }
  | { type: 'hitl_paused'; payload: { workflow_id: WorkflowId; node_id: NodeId; context: unknown; description: string } }
  | { type: 'workflow_completed'; payload: { workflow_id: WorkflowId; summary: string; results: TaskResultPayload[] } }
  | { type: 'workflow_aborted'; payload: { workflow_id: WorkflowId; reason: string } }
  | { type: 'global_memory_query_result'; payload: { workflow_id: WorkflowId; query: string; results: unknown[] } }
  | { type: 'error'; payload: { workflow_id?: WorkflowId; code: string; message: string } }
