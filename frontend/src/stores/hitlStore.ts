import { create } from 'zustand'
import type { NodeId, WorkflowId } from '../lib/types'
import { wsClient } from '../lib/wsClient'

interface HitlPendingState {
  workflowId: WorkflowId
  nodeId: NodeId
  context: unknown
  description: string
}

interface HitlState {
  pending: HitlPendingState | null
  setPending: (state: HitlPendingState | null) => void
  approve: () => void
  reject: (reason: string) => void
}

export const useHitlStore = create<HitlState>((set, get) => ({
  pending: null,

  setPending: (pending) => set({ pending }),

  approve: () => {
    const { pending } = get()
    if (!pending) return
    wsClient.send({
      type: 'hitl_resume',
      payload: { workflow_id: pending.workflowId, node_id: pending.nodeId, decision: 'approved' },
    })
    set({ pending: null })
  },

  reject: (reason) => {
    const { pending } = get()
    if (!pending) return
    wsClient.send({
      type: 'hitl_resume',
      payload: { workflow_id: pending.workflowId, node_id: pending.nodeId, decision: { rejected: { reason } } },
    })
    set({ pending: null })
  },
}))
