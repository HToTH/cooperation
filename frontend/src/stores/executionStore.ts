import { create } from 'zustand'
import type { AgentMessage, AgentNodeState, NodeId, TaskResultPayload, WorkflowId } from '../lib/types'
import { wsClient } from '../lib/wsClient'

interface ActivityEntry {
  id: string
  timestamp: number
  type: 'state_change' | 'message' | 'error' | 'completed'
  workflowId: WorkflowId
  content: string
}

interface ExecutionState {
  workflowState: string
  nodeStates: Record<NodeId, AgentNodeState>
  activityLog: ActivityEntry[]
  results: TaskResultPayload[]
  isRunning: boolean

  startWorkflow: (workflowId: WorkflowId, graph: unknown) => void
  stopWorkflow: (workflowId: WorkflowId) => void
  handleWorkflowStateChanged: (workflowId: WorkflowId, state: string) => void
  handleNodeStateChanged: (workflowId: WorkflowId, nodeId: NodeId, state: AgentNodeState) => void
  handleAgentMessage: (workflowId: WorkflowId, message: AgentMessage) => void
  handleWorkflowCompleted: (workflowId: WorkflowId, summary: string, results: TaskResultPayload[]) => void
  handleWorkflowAborted: (workflowId: WorkflowId, reason: string) => void
  clearLog: () => void
}

let logCounter = 0

const makeEntry = (type: ActivityEntry['type'], workflowId: WorkflowId, content: string): ActivityEntry => ({
  id: `log_${++logCounter}`,
  timestamp: Date.now(),
  type,
  workflowId,
  content,
})

export const useExecutionStore = create<ExecutionState>((set) => ({
  workflowState: 'Idle',
  nodeStates: {},
  activityLog: [],
  results: [],
  isRunning: false,

  startWorkflow: (workflowId, graph) => {
    wsClient.send({ type: 'start_workflow', payload: { workflow_id: workflowId, graph: graph as never } })
    set({ isRunning: true, activityLog: [], results: [], nodeStates: {}, workflowState: 'Planning' })
  },

  stopWorkflow: (workflowId) => {
    wsClient.send({ type: 'stop_workflow', payload: { workflow_id: workflowId } })
    set({ isRunning: false, workflowState: 'Idle' })
  },

  handleWorkflowStateChanged: (workflowId, state) => {
    set((s) => ({
      workflowState: state,
      activityLog: [...s.activityLog, makeEntry('state_change', workflowId, `Workflow → ${state}`)],
    }))
  },

  handleNodeStateChanged: (workflowId, nodeId, state) => {
    set((s) => ({
      nodeStates: { ...s.nodeStates, [nodeId]: state },
      activityLog: [
        ...s.activityLog,
        makeEntry('state_change', workflowId, `Node ${nodeId} → ${state}`),
      ],
    }))
  },

  handleAgentMessage: (workflowId, message) => {
    set((s) => ({
      activityLog: [
        ...s.activityLog,
        makeEntry('message', workflowId, `[${message.from_agent.role}→${message.to_agent.role}] ${message.message_type}`),
      ],
    }))
  },

  handleWorkflowCompleted: (workflowId, summary, results) => {
    set((s) => ({
      isRunning: false,
      workflowState: 'Completed',
      results,
      activityLog: [
        ...s.activityLog,
        makeEntry('completed', workflowId, `Completed: ${summary}`),
      ],
    }))
  },

  handleWorkflowAborted: (workflowId, reason) => {
    set((s) => ({
      isRunning: false,
      workflowState: 'Aborted',
      activityLog: [
        ...s.activityLog,
        makeEntry('error', workflowId, `Aborted: ${reason}`),
      ],
    }))
  },

  clearLog: () => set({ activityLog: [] }),
}))
