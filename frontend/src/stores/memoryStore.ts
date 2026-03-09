import { create } from 'zustand'
import type { WorkflowId } from '../lib/types'
import { wsClient } from '../lib/wsClient'

interface MemoryState {
  entries: unknown[]
  query: string
  setQuery: (q: string) => void
  search: (workflowId: WorkflowId) => void
  handleQueryResult: (results: unknown[]) => void
}

export const useMemoryStore = create<MemoryState>((set, get) => ({
  entries: [],
  query: '',

  setQuery: (q) => set({ query: q }),

  search: (workflowId) => {
    const { query } = get()
    wsClient.send({ type: 'query_global_memory', payload: { workflow_id: workflowId, query } })
  },

  handleQueryResult: (results) => set({ entries: results }),
}))
