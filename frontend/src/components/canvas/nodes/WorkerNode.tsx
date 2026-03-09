import { Handle, Position, type NodeProps } from '@xyflow/react'
import type { AgentNode, AgentNodeState } from '../../../lib/types'

const stateColors: Record<AgentNodeState, string> = {
  idle: '#4a5568',
  running: '#3182ce',
  paused: '#d69e2e',
  completed: '#38a169',
  failed: '#e53e3e',
}

export function WorkerNode({ data, selected }: NodeProps) {
  const node = data as unknown as AgentNode
  const color = stateColors[node.state] ?? stateColors.idle

  return (
    <div style={{
      background: '#1a2332',
      border: `2px solid ${color}`,
      borderRadius: 12,
      padding: '12px 16px',
      minWidth: 160,
      boxShadow: selected ? `0 0 0 2px ${color}40` : 'none',
    }}>
      <Handle type="target" position={Position.Top} />
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6 }}>
        <span style={{ fontSize: 18 }}>🤖</span>
        <span style={{ fontWeight: 600, fontSize: 13, color: '#e2e8f0' }}>{node.label || 'Worker'}</span>
      </div>
      <div style={{ fontSize: 11, color: '#718096' }}>
        {(node.model as { provider: string; model: string }).provider} · {(node.model as { provider: string; model: string }).model}
      </div>
      <div style={{
        marginTop: 8,
        fontSize: 10,
        padding: '2px 8px',
        borderRadius: 20,
        background: `${color}22`,
        color,
        display: 'inline-block',
        textTransform: 'uppercase',
        letterSpacing: 1,
      }}>
        {node.state}
      </div>
      <Handle type="source" position={Position.Bottom} />
    </div>
  )
}
