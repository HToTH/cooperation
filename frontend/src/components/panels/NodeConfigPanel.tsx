import { useState } from 'react'
import { useWorkflowStore } from '../../stores/workflowStore'
import type { AgentKind, AgentNode, ModelProvider } from '../../lib/types'
import { v4 as uuid } from 'uuid'

const MODEL_OPTIONS: Record<string, string[]> = {
  Claude: ['claude-opus-4-6', 'claude-sonnet-4-6', 'claude-haiku-4-5-20251001'],
  Gemini: ['gemini-2.0-flash', 'gemini-1.5-pro'],
  OpenAI: ['gpt-4o', 'gpt-4o-mini'],
}

const KIND_LABELS: Record<AgentKind, string> = {
  raw_llm: '⚙ Raw LLM API',
  claude_code: '🖥 Claude Code (CLI)',
  gemini_cli: '✦ Gemini CLI',
  codex: '◈ Codex CLI (OpenAI)',
}

function createNode(role: string, label: string, provider: string, model: string, kind: AgentKind): AgentNode {
  const id = uuid()
  return {
    id,
    label: label || role || 'agent',
    role,
    model: { provider, model } as ModelProvider,
    kind,
    model_config: { temperature: 0.7, max_tokens: 4096, system_prompt: '' },
    context_pool_id: `ctx_${id}_${uuid().slice(0, 8)}`,
    state: 'idle',
    position: { x: Math.random() * 400 + 50, y: Math.random() * 300 + 50 },
  }
}

export function NodeConfigPanel() {
  const addNode = useWorkflowStore((s) => s.addNode)
  const [provider, setProvider] = useState('Claude')
  const [model, setModel] = useState('claude-opus-4-6')
  const [label, setLabel] = useState('')
  const [role, setRole] = useState('worker')
  const [kind, setKind] = useState<AgentKind>('raw_llm')

  const handleProviderChange = (p: string) => {
    setProvider(p)
    setModel(MODEL_OPTIONS[p][0])
  }

  const handleAdd = () => {
    addNode(createNode(role, label, provider, model, kind))
    setLabel('')
  }

  return (
    <div style={{ background: '#0f1117', borderTop: '1px solid #1e2533', padding: '12px 14px' }}>
      <div style={{ fontWeight: 700, fontSize: 12, color: '#e2e8f0', marginBottom: 10 }}>Add Node</div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, marginBottom: 8 }}>
        <div>
          <label style={labelStyle}>Role</label>
          <input
            value={role}
            onChange={(e) => setRole(e.target.value)}
            placeholder="e.g. coordinator"
            style={selectStyle}
          />
        </div>
        <div>
          <label style={labelStyle}>Label</label>
          <input
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder={role || 'agent'}
            style={{ ...selectStyle, background: '#1a2332' }}
          />
        </div>
      </div>

      <div style={{ marginBottom: 8 }}>
        <label style={labelStyle}>Agent Kind</label>
        <select value={kind} onChange={(e) => setKind(e.target.value as AgentKind)} style={{ ...selectStyle, width: '100%' }}>
          {(Object.keys(KIND_LABELS) as AgentKind[]).map((k) => (
            <option key={k} value={k}>{KIND_LABELS[k]}</option>
          ))}
        </select>
      </div>

      {kind === 'raw_llm' && (
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, marginBottom: 8 }}>
          <div>
            <label style={labelStyle}>Provider</label>
            <select value={provider} onChange={(e) => handleProviderChange(e.target.value)} style={selectStyle}>
              <option>Claude</option>
              <option>Gemini</option>
              <option>OpenAI</option>
            </select>
          </div>
          <div>
            <label style={labelStyle}>Model</label>
            <select value={model} onChange={(e) => setModel(e.target.value)} style={selectStyle}>
              {MODEL_OPTIONS[provider].map((m) => <option key={m}>{m}</option>)}
            </select>
          </div>
        </div>
      )}

      <div style={{ marginBottom: 8 }}>
        <label style={labelStyle}>
          <span style={{ color: '#e53e3e' }}>⏸</span> Add HITL pause node
        </label>
        <button
          onClick={() => addNode(createNode('human_in_loop', 'Human Review', provider, model, 'raw_llm'))}
          style={{ ...selectStyle, background: '#2d3748', cursor: 'pointer', textAlign: 'left', border: '1px dashed #4a5568' }}
        >
          + Human Review Checkpoint
        </button>
      </div>

      <button onClick={handleAdd} style={{
        width: '100%', background: '#2b6cb0', border: 'none', borderRadius: 6,
        padding: '8px', color: '#fff', fontSize: 12, cursor: 'pointer', fontWeight: 600,
      }}>
        + Add Node
      </button>
    </div>
  )
}

const labelStyle: React.CSSProperties = { fontSize: 10, color: '#718096', display: 'block', marginBottom: 3 }

const selectStyle: React.CSSProperties = {
  width: '100%', background: '#1e2533', border: '1px solid #2d3748',
  borderRadius: 6, padding: '6px 8px', color: '#e2e8f0', fontSize: 12, outline: 'none',
  boxSizing: 'border-box',
}
