import { useCallback, useEffect } from 'react'
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  addEdge,
  useNodesState,
  useEdgesState,
  type Connection,
  type Node,
  type Edge,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'

import { AgentNodeComponent } from './nodes/AgentNodeComponent'
import { useWorkflowStore } from '../../stores/workflowStore'
import { useExecutionStore } from '../../stores/executionStore'
import type { AgentNode } from '../../lib/types'

const nodeTypes = {
  agent: AgentNodeComponent,
}

function agentNodeToRf(node: AgentNode): Node {
  return {
    id: node.id,
    type: 'agent',
    position: node.position,
    data: node as unknown as Record<string, unknown>,
  }
}

export function AgentFlowCanvas() {
  const graph = useWorkflowStore((s) => s.graph)
  const updateNode = useWorkflowStore((s) => s.updateNode)
  const addEdgeToStore = useWorkflowStore((s) => s.addEdge)
  const selectNode = useWorkflowStore((s) => s.selectNode)
  const nodeStates = useExecutionStore((s) => s.nodeStates)

  const initialNodes = Object.values(graph.nodes).map(agentNodeToRf)
  const initialEdges: Edge[] = graph.edges.map((e) => ({
    id: e.id,
    source: e.source,
    target: e.target,
    label: e.label,
    animated: false,
    style: { stroke: '#4a5568' },
  }))

  const [nodes, setNodes, onNodesChange] = useNodesState(initialNodes)
  const [edges, setEdges, onEdgesChange] = useEdgesState(initialEdges)

  // Sync store → React Flow: handles add, remove, and data updates (label, role, config…)
  useEffect(() => {
    const storeIds = new Set(Object.keys(graph.nodes))
    setNodes((nds) => {
      const rfIds = new Set(nds.map((n) => n.id))
      // Update data on existing nodes, preserve RF position
      const updated = nds
        .filter((n) => storeIds.has(n.id))
        .map((n) => {
          const storeNode = graph.nodes[n.id]
          return { ...n, data: storeNode as unknown as Record<string, unknown> }
        })
      // Add brand-new nodes
      const added = Object.values(graph.nodes)
        .filter((n) => !rfIds.has(n.id))
        .map(agentNodeToRf)
      return [...updated, ...added]
    })
  }, [graph.nodes, setNodes])

  // Sync store → React Flow when edges change
  useEffect(() => {
    setEdges(graph.edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      label: e.label,
      animated: false,
      style: { stroke: '#4a5568' },
    })))
  }, [graph.edges, setEdges])

  // Sync live node states from execution into canvas nodes
  useEffect(() => {
    setNodes((nds) =>
      nds.map((n) => {
        const liveState = nodeStates[n.id]
        if (!liveState) return n
        return {
          ...n,
          data: { ...(n.data as unknown as AgentNode), state: liveState } as unknown as Record<string, unknown>,
        }
      })
    )
  }, [nodeStates, setNodes])

  // Animate edges when workflow is running
  useEffect(() => {
    setEdges((eds) =>
      eds.map((e) => ({
        ...e,
        animated: Object.values(nodeStates).some((s) => s === 'running'),
        style: { stroke: '#4a5568' },
      }))
    )
  }, [nodeStates, setEdges])

  const onConnect = useCallback(
    (connection: Connection) => {
      const edge = {
        id: `edge_${connection.source}_${connection.target}`,
        source: connection.source!,
        target: connection.target!,
      }
      addEdgeToStore(edge)
      setEdges((eds) => addEdge({ ...connection, animated: false, style: { stroke: '#4a5568' } }, eds))
    },
    [addEdgeToStore, setEdges]
  )

  const onNodeDragStop = useCallback(
    (_: unknown, node: Node) => {
      updateNode(node.id, { position: node.position })
    },
    [updateNode]
  )

  const removeNode = useWorkflowStore((s) => s.removeNode)
  const removeEdge = useWorkflowStore((s) => s.removeEdge)

  const onNodeClick = useCallback(
    (_: unknown, node: Node) => {
      selectNode(node.id)
    },
    [selectNode]
  )

  const onNodesDelete = useCallback(
    (deleted: Node[]) => {
      deleted.forEach((n) => removeNode(n.id))
    },
    [removeNode]
  )

  const onEdgesDelete = useCallback(
    (deleted: Edge[]) => {
      deleted.forEach((e) => removeEdge(e.id))
    },
    [removeEdge]
  )

  return (
    <div style={{ width: '100%', height: '100%' }}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onConnect={onConnect}
        onNodeDragStop={onNodeDragStop}
        onNodeClick={onNodeClick}
        onNodesDelete={onNodesDelete}
        onEdgesDelete={onEdgesDelete}
        nodeTypes={nodeTypes}
        deleteKeyCode={['Backspace', 'Delete']}
        fitView
        style={{ background: '#0f1117' }}
      >
        <Background color="#1e2533" gap={20} />
        <Controls style={{ background: '#1a2332', border: '1px solid #2d3748' }} />
        <MiniMap
          style={{ background: '#1a2332', border: '1px solid #2d3748' }}
          nodeColor={(n) => {
            const state = (n.data as unknown as AgentNode)?.state
            if (state === 'running') return '#3182ce'
            if (state === 'completed') return '#38a169'
            if (state === 'failed') return '#e53e3e'
            return '#4a5568'
          }}
        />
      </ReactFlow>
    </div>
  )
}
