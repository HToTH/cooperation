import { Handle, Position } from '@xyflow/react'
import type { AgentNode, AgentNodeState } from '../../../lib/types'

const STATE_COLORS: Record<AgentNodeState, string> = {
  idle: '#4a5568',
  running: '#3182ce',
  paused: '#d69e2e',
  completed: '#38a169',
  failed: '#e53e3e',
}

const KIND_ICONS: Record<string, string> = {
  raw_llm: '⚙',
  claude_code: '🖥',
  gemini_cli: '✦',
  codex: '◈',
}

const handleStyle: React.CSSProperties = {
  width: 12,
  height: 12,
  background: '#63b3ed',
  border: '2px solid #1a2332',
  borderRadius: '50%',
}

export function AgentNodeComponent({ data, selected }: { data: AgentNode; selected?: boolean }) {
  const stateColor = STATE_COLORS[data.state] ?? '#4a5568'
  const kindIcon = KIND_ICONS[data.kind] ?? '⚙'

  return (
    <div style={{
      background: '#1a2332',
      border: `2px solid ${selected ? '#63b3ed' : stateColor}`,
      borderRadius: 10,
      padding: '10px 14px',
      minWidth: 150,
      boxShadow: `0 0 ${data.state === 'running' ? '12px' : '4px'} ${stateColor}60`,
      transition: 'all 0.2s',
    }}>
      <Handle type="target" position={Position.Top} style={handleStyle} />

      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
        <span style={{ fontSize: 14 }}>{kindIcon}</span>
        <span style={{
          fontSize: 11,
          fontWeight: 700,
          color: '#e2e8f0',
          flex: 1,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}>
          {data.label}
        </span>
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <span style={{
          fontSize: 9,
          background: '#2d3748',
          color: '#a0aec0',
          padding: '1px 6px',
          borderRadius: 8,
          textTransform: 'uppercase',
          letterSpacing: 0.5,
          maxWidth: 100,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}>
          {data.role || 'agent'}
        </span>
        <span style={{
          fontSize: 9,
          color: stateColor,
          marginLeft: 'auto',
          fontWeight: 600,
        }}>
          {data.state}
        </span>
      </div>

      <Handle type="source" position={Position.Bottom} style={handleStyle} />
    </div>
  )
}
