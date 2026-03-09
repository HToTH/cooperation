import { useWorkflowStore } from '../../stores/workflowStore'
import type { AgentKind, ModelProvider } from '../../lib/types'

const MODEL_OPTIONS: Record<string, string[]> = {
  Claude: ['claude-opus-4-6', 'claude-sonnet-4-6', 'claude-haiku-4-5-20251001'],
  Gemini: ['gemini-2.0-flash', 'gemini-1.5-pro'],
  OpenAI: ['gpt-4o', 'gpt-4o-mini'],
}

const KIND_LABELS: Record<AgentKind, string> = {
  raw_llm: 'Raw LLM API',
  claude_code: 'Claude Code (CLI)',
  gemini_cli: 'Gemini CLI',
  codex: 'Codex CLI (OpenAI)',
}

export function NodeEditPanel() {
  const selectedNodeId = useWorkflowStore((s) => s.selectedNodeId)
  const graph = useWorkflowStore((s) => s.graph)
  const updateNode = useWorkflowStore((s) => s.updateNode)
  const removeNode = useWorkflowStore((s) => s.removeNode)
  const selectNode = useWorkflowStore((s) => s.selectNode)
  const syncGraph = useWorkflowStore((s) => s.syncGraph)

  if (!selectedNodeId) return null
  const node = graph.nodes[selectedNodeId]
  if (!node) return null

  const provider = node.model.provider
  const model = node.model.model

  const handleProviderChange = (p: string) => {
    updateNode(node.id, { model: { provider: p, model: MODEL_OPTIONS[p][0] } as ModelProvider })
  }

  const handleModelChange = (m: string) => {
    updateNode(node.id, { model: { provider, model: m } as ModelProvider })
  }

  const handlePromptChange = (prompt: string) => {
    updateNode(node.id, { model_config: { ...node.model_config, system_prompt: prompt } })
  }

  const handleKindChange = (k: AgentKind) => {
    updateNode(node.id, { kind: k })
  }

  const handleRoleChange = (r: string) => {
    updateNode(node.id, { role: r })
  }

  const handleLabelChange = (label: string) => {
    updateNode(node.id, { label })
  }

  const handleTemperatureChange = (t: string) => {
    const v = parseFloat(t)
    if (!isNaN(v)) updateNode(node.id, { model_config: { ...node.model_config, temperature: v } })
  }

  const handleMaxTokensChange = (t: string) => {
    const v = parseInt(t, 10)
    if (!isNaN(v)) updateNode(node.id, { model_config: { ...node.model_config, max_tokens: v } })
  }

  return (
    <div style={{ background: '#0f1117', borderTop: '1px solid #1e2533', padding: '12px 14px', flex: 1, overflow: 'auto' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
        <div style={{ fontWeight: 700, fontSize: 12, color: '#63b3ed' }}>Edit Node</div>
        <div style={{ display: 'flex', gap: 4 }}>
          <button
            onClick={() => { removeNode(node.id); selectNode(null) }}
            title="Delete node"
            style={{ background: 'none', border: '1px solid #e53e3e', color: '#e53e3e', cursor: 'pointer', fontSize: 11, borderRadius: 4, padding: '2px 6px', lineHeight: 1 }}
          >
            Delete
          </button>
          <button onClick={() => selectNode(null)} style={{ background: 'none', border: 'none', color: '#718096', cursor: 'pointer', fontSize: 14, lineHeight: 1 }}>✕</button>
        </div>
      </div>

      <Field label="Label">
        <input value={node.label} onChange={(e) => handleLabelChange(e.target.value)} onBlur={syncGraph} style={inputStyle} />
      </Field>

      <Field label="Role (free-form — use 'human_in_loop' for HITL pause)">
        <input
          value={node.role}
          onChange={(e) => handleRoleChange(e.target.value)}
          onBlur={syncGraph}
          placeholder="e.g. coordinator, analyst, reviewer"
          style={inputStyle}
        />
      </Field>

      {node.role !== 'human_in_loop' && (
        <Field label="Agent Kind">
          <select value={node.kind} onChange={(e) => { handleKindChange(e.target.value as AgentKind); syncGraph() }} style={inputStyle}>
            {(Object.keys(KIND_LABELS) as AgentKind[]).map((k) => (
              <option key={k} value={k}>{KIND_LABELS[k]}</option>
            ))}
          </select>
        </Field>
      )}

      {node.role !== 'human_in_loop' && node.kind === 'raw_llm' && (
        <>
          <Field label="Provider">
            <select value={provider} onChange={(e) => { handleProviderChange(e.target.value); syncGraph() }} style={inputStyle}>
              <option>Claude</option>
              <option>Gemini</option>
              <option>OpenAI</option>
            </select>
          </Field>
          <Field label="Model">
            <select value={model} onChange={(e) => { handleModelChange(e.target.value); syncGraph() }} style={inputStyle}>
              {(MODEL_OPTIONS[provider] ?? []).map((m) => (
                <option key={m}>{m}</option>
              ))}
            </select>
          </Field>
          <Field label="Temperature">
            <input type="number" min={0} max={2} step={0.1} value={node.model_config.temperature}
              onChange={(e) => handleTemperatureChange(e.target.value)} onBlur={syncGraph} style={inputStyle} />
          </Field>
          <Field label="Max Tokens">
            <input type="number" min={256} max={32768} step={256} value={node.model_config.max_tokens}
              onChange={(e) => handleMaxTokensChange(e.target.value)} onBlur={syncGraph} style={inputStyle} />
          </Field>
        </>
      )}

      {node.role !== 'human_in_loop' && (
        <Field label="System Prompt">
          <textarea
            value={node.model_config.system_prompt}
            onChange={(e) => handlePromptChange(e.target.value)}
            onBlur={syncGraph}
            rows={6}
            placeholder="You are a helpful agent..."
            style={{ ...inputStyle, resize: 'vertical', fontFamily: 'monospace', fontSize: 11, lineHeight: 1.4 }}
          />
        </Field>
      )}
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ marginBottom: 8 }}>
      <label style={{ fontSize: 10, color: '#718096', display: 'block', marginBottom: 3 }}>{label}</label>
      {children}
    </div>
  )
}

const inputStyle: React.CSSProperties = {
  width: '100%',
  background: '#1e2533',
  border: '1px solid #2d3748',
  borderRadius: 6,
  padding: '6px 8px',
  color: '#e2e8f0',
  fontSize: 12,
  outline: 'none',
  boxSizing: 'border-box',
}
