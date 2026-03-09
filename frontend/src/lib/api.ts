import type { WorkflowGraph } from './types'
import { API_BASE } from './runtime'

export interface WorkflowSummary {
  id: string
  name: string
  updated_at: string
}

export async function listWorkflows(): Promise<WorkflowSummary[]> {
  const res = await fetch(`${API_BASE}/api/workflows`)
  if (!res.ok) throw new Error(`Failed to list workflows: ${res.status}`)
  return res.json()
}

export async function saveWorkflow(graph: WorkflowGraph): Promise<void> {
  const res = await fetch(`${API_BASE}/api/workflows`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(graph),
  })
  if (!res.ok) throw new Error(`Failed to save workflow: ${res.status}`)
}

export async function loadWorkflow(id: string): Promise<WorkflowGraph> {
  const res = await fetch(`${API_BASE}/api/workflows/${id}`)
  if (!res.ok) throw new Error(`Failed to load workflow: ${res.status}`)
  return res.json()
}

export async function deleteWorkflow(id: string): Promise<void> {
  const res = await fetch(`${API_BASE}/api/workflows/${id}`, { method: 'DELETE' })
  if (!res.ok) throw new Error(`Failed to delete workflow: ${res.status}`)
}

export interface ChatMessage {
  role: 'user' | 'assistant'
  content: string
  timestamp: number
}

export interface GroupAttachment {
  name: string
  content_type: string
  data: string // base64
}

export interface GroupAuthorizationRequired {
  message: string
  details?: string
}

export type GroupSystemLevel = 'info' | 'warning'
export type GroupTaskStatus = 'queued' | 'running' | 'completed' | 'failed' | 'blocked'
export type GroupHitlStatus = 'pending' | 'approved' | 'rejected'

export interface GroupUserMessage {
  type: 'user'
  id: string
  content: string
  attachments: GroupAttachment[]
  mentioned_agent_ids: string[]
  timestamp: number
}

export interface GroupAgentMessage {
  type: 'agent'
  id: string
  agent_id: string
  content: string
  timestamp: number
}

export interface GroupSystemMessage {
  type: 'system'
  id: string
  content: string
  level: GroupSystemLevel
  timestamp: number
}

export interface GroupTaskMessage {
  type: 'task'
  id: string
  task_id: string
  agent_id: string
  command: string
  status: GroupTaskStatus
  summary?: string | null
  timestamp: number
}

export interface GroupHitlMessage {
  type: 'hitl'
  id: string
  workflow_id: string
  node_id: string
  description: string
  context: unknown
  status: GroupHitlStatus
  reason?: string | null
  timestamp: number
}

export type GroupChatMessage =
  | GroupUserMessage
  | GroupAgentMessage
  | GroupSystemMessage
  | GroupTaskMessage
  | GroupHitlMessage

export interface GroupChatResponse {
  messages: GroupChatMessage[]
  workflow_graph?: WorkflowGraph | null
}

export async function loadGroupChatHistory(workflowId: string): Promise<GroupChatMessage[]> {
  const res = await fetch(`${API_BASE}/api/workflows/${workflowId}/group-chat`)
  if (!res.ok) throw new Error(`Failed to load group chat history: ${res.status}`)
  const data = await res.json()
  return data.messages as GroupChatMessage[]
}

export async function sendGroupChatMessage(
  workflowId: string,
  message: string,
  mentionedAgentIds: string[],
  graph: WorkflowGraph,
  attachments?: GroupAttachment[],
): Promise<GroupChatResponse> {
  const res = await fetch(`${API_BASE}/api/group-chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      workflow_id: workflowId,
      message,
      mentioned_agent_ids: mentionedAgentIds,
      graph,
      attachments,
    }),
  })
  if (!res.ok) {
    const err = await res.text()
    throw new Error(err || `Group chat failed: ${res.status}`)
  }
  return res.json()
}

export async function sendChatMessage(
  workflowId: string,
  agentId: string,
  agent: import('./types').AgentNode,
  message: string,
): Promise<string> {
  const res = await fetch(`${API_BASE}/api/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ workflow_id: workflowId, agent_id: agentId, agent, message }),
  })
  if (!res.ok) {
    const err = await res.text()
    throw new Error(err || `Chat failed: ${res.status}`)
  }
  const data = await res.json()
  return data.response as string
}
