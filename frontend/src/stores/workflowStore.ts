import { create } from 'zustand'
import type { AgentNode, DirectedEdge, WorkflowGraph } from '../lib/types'
import { wsClient } from '../lib/wsClient'
import { saveWorkflow, loadWorkflow, listWorkflows, deleteWorkflow, type WorkflowSummary } from '../lib/api'
import { v4 as uuid } from 'uuid'

interface WorkflowState {
  graph: WorkflowGraph
  selectedNodeId: string | null
  addNode: (node: AgentNode) => void
  removeNode: (id: string) => void
  updateNode: (id: string, patch: Partial<AgentNode>) => void
  addEdge: (edge: DirectedEdge) => void
  removeEdge: (id: string) => void
  selectNode: (id: string | null) => void
  syncGraph: () => void
  loadGraph: (graph: WorkflowGraph) => void
  saveToServer: () => Promise<void>
  loadFromServer: (id: string) => Promise<void>
  listFromServer: () => Promise<WorkflowSummary[]>
  renameWorkflow: (name: string) => void
  newWorkflow: () => void
  deleteFromServer: (id: string) => Promise<void>
}

const initialGraph: WorkflowGraph = {
  id: uuid(),
  name: 'Untitled Workflow',
  nodes: {},
  edges: [],
}

export const useWorkflowStore = create<WorkflowState>((set, get) => ({
  graph: initialGraph,
  selectedNodeId: null,

  addNode: (node) =>
    set((s) => ({
      graph: { ...s.graph, nodes: { ...s.graph.nodes, [node.id]: node } },
    })),

  removeNode: (id) =>
    set((s) => {
      const { [id]: _, ...rest } = s.graph.nodes
      return {
        graph: { ...s.graph, nodes: rest, edges: s.graph.edges.filter((e) => e.source !== id && e.target !== id) },
        selectedNodeId: s.selectedNodeId === id ? null : s.selectedNodeId,
      }
    }),

  updateNode: (id, patch) =>
    set((s) => ({
      graph: {
        ...s.graph,
        nodes: { ...s.graph.nodes, [id]: { ...s.graph.nodes[id], ...patch } },
      },
    })),

  addEdge: (edge) =>
    set((s) => ({ graph: { ...s.graph, edges: [...s.graph.edges, edge] } })),

  removeEdge: (id) =>
    set((s) => ({ graph: { ...s.graph, edges: s.graph.edges.filter((e) => e.id !== id) } })),

  selectNode: (id) => set({ selectedNodeId: id }),

  syncGraph: () => {
    const { graph } = get()
    wsClient.send({ type: 'update_graph', payload: { workflow_id: graph.id, graph } })
  },

  loadGraph: (graph) => set({ graph }),

  saveToServer: async () => {
    const { graph } = get()
    await saveWorkflow(graph)
  },

  loadFromServer: async (id) => {
    const graph = await loadWorkflow(id)
    set({ graph, selectedNodeId: null })
  },

  listFromServer: () => listWorkflows(),

  renameWorkflow: (name) =>
    set((s) => ({ graph: { ...s.graph, name } })),

  newWorkflow: () =>
    set({
      graph: { id: uuid(), name: 'Untitled Workflow', nodes: {}, edges: [] },
      selectedNodeId: null,
    }),

  deleteFromServer: async (id) => {
    await deleteWorkflow(id)
    // If deleting the current workflow, switch to a new one
    if (get().graph.id === id) {
      set({
        graph: { id: uuid(), name: 'Untitled Workflow', nodes: {}, edges: [] },
        selectedNodeId: null,
      })
    }
  },
}))
