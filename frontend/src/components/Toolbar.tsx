import { useState } from 'react'
import { useExecutionStore } from '../stores/executionStore'
import { useWorkflowStore } from '../stores/workflowStore'
import { useWsStore } from '../stores/wsStore'
import { useChatStore } from '../stores/chatStore'
import type { WorkflowSummary } from '../lib/api'

export function Toolbar() {
  const graph = useWorkflowStore((s) => s.graph)
  const workflowState = useExecutionStore((s) => s.workflowState)
  const connected = useWsStore((s) => s.connected)
  const saveToServer = useWorkflowStore((s) => s.saveToServer)
  const loadFromServer = useWorkflowStore((s) => s.loadFromServer)
  const listFromServer = useWorkflowStore((s) => s.listFromServer)
  const deleteFromServer = useWorkflowStore((s) => s.deleteFromServer)
  const renameWorkflow = useWorkflowStore((s) => s.renameWorkflow)
  const newWorkflow = useWorkflowStore((s) => s.newWorkflow)

  const openChat = useChatStore((s) => s.open)

  const [saving, setSaving] = useState(false)
  const [loadList, setLoadList] = useState<WorkflowSummary[] | null>(null)
  const [editingName, setEditingName] = useState(false)

  const handleSave = async () => {
    setSaving(true)
    try { await saveToServer() } finally { setSaving(false) }
  }

  const handleLoadClick = async () => {
    if (loadList) { setLoadList(null); return }
    const list = await listFromServer()
    setLoadList(list)
  }

  const handleLoadSelect = async (id: string) => {
    await loadFromServer(id)
    setLoadList(null)
  }

  return (
    <div style={{
      height: 48,
      background: '#1a2332',
      borderBottom: '1px solid #2d3748',
      display: 'flex',
      alignItems: 'center',
      padding: '0 16px',
      gap: 12,
      position: 'relative',
    }}>
      <span style={{ fontWeight: 800, fontSize: 16, color: '#63b3ed', letterSpacing: -0.5 }}>
        cooperation
      </span>

      {editingName ? (
        <input
          autoFocus
          defaultValue={graph.name}
          onBlur={(e) => { renameWorkflow(e.target.value); setEditingName(false); saveToServer() }}
          onKeyDown={(e) => { if (e.key === 'Enter') { renameWorkflow((e.target as HTMLInputElement).value); setEditingName(false); saveToServer() } }}
          style={{ fontSize: 13, background: '#0f1117', border: '1px solid #3182ce', borderRadius: 4, color: '#e2e8f0', padding: '2px 6px', outline: 'none', width: 180 }}
        />
      ) : (
        <span
          onClick={() => setEditingName(true)}
          title="Click to rename"
          style={{ fontSize: 13, color: '#a0aec0', cursor: 'text', userSelect: 'none' }}
        >
          {graph.name}
        </span>
      )}

      <div style={{ flex: 1 }}/>

      {/* Load dropdown */}
      {loadList && (
        <div style={{
          position: 'absolute', top: 44, right: 160, background: '#1a2332', border: '1px solid #2d3748',
          borderRadius: 8, minWidth: 240, zIndex: 100, boxShadow: '0 4px 20px #00000060',
        }}>
          <div style={{ padding: '8px 14px 6px', fontSize: 10, color: '#718096', borderBottom: '1px solid #2d3748', textTransform: 'uppercase', letterSpacing: 0.5 }}>
            Saved Teams
          </div>
          {loadList.length === 0 ? (
            <div style={{ padding: '10px 14px', color: '#718096', fontSize: 12 }}>No saved workflows yet</div>
          ) : loadList.map((w) => {
            const isActive = w.id === graph.id
            return (
              <div key={w.id} style={{
                display: 'flex', alignItems: 'center',
                borderBottom: '1px solid #2d3748',
                background: isActive ? '#1a2d42' : 'transparent',
              }}
                onMouseEnter={(e) => { if (!isActive) e.currentTarget.style.background = '#243447' }}
                onMouseLeave={(e) => { if (!isActive) e.currentTarget.style.background = isActive ? '#1a2d42' : 'transparent' }}
              >
                <div onClick={() => handleLoadSelect(w.id)} style={{
                  flex: 1, padding: '8px 14px', cursor: 'pointer', fontSize: 12,
                  color: isActive ? '#63b3ed' : '#e2e8f0',
                }}>
                  <div style={{ fontWeight: 600 }}>{w.name}</div>
                  <div style={{ fontSize: 10, color: '#718096', marginTop: 2 }}>{w.updated_at.slice(0, 16).replace('T', ' ')}</div>
                </div>
                {isActive && <span style={{ fontSize: 10, color: '#63b3ed', paddingRight: 8 }}>active</span>}
                <button
                  onClick={async (e) => {
                    e.stopPropagation()
                    if (!confirm(`Delete "${w.name}"?`)) return
                    await deleteFromServer(w.id)
                    const updated = await listFromServer()
                    setLoadList(updated)
                  }}
                  title="Delete"
                  style={{ background: 'none', border: 'none', color: '#718096', cursor: 'pointer', padding: '0 10px', fontSize: 13, lineHeight: 1 }}
                  onMouseEnter={(e) => (e.currentTarget.style.color = '#e53e3e')}
                  onMouseLeave={(e) => (e.currentTarget.style.color = '#718096')}
                >✕</button>
              </div>
            )
          })}
          <div
            onClick={() => { newWorkflow(); setLoadList(null) }}
            style={{ padding: '8px 14px', cursor: 'pointer', fontSize: 12, color: '#a0aec0', borderTop: '1px solid #2d3748' }}
            onMouseEnter={(e) => (e.currentTarget.style.background = '#243447')}
            onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
          >
            + New Team
          </div>
        </div>
      )}

      <div style={{ flex: 1 }} />

      <button onClick={handleSave} disabled={saving} style={ghostBtn}>
        {saving ? '...' : 'Save'}
      </button>
      <button onClick={handleLoadClick} style={{ ...ghostBtn, background: loadList ? '#243447' : undefined }}>
        Load
      </button>

      <div style={{
        fontSize: 11,
        color: '#718096',
        padding: '3px 8px',
        borderRadius: 10,
        background: '#0f1117',
      }}>
        {workflowState}
      </div>

      <div style={{
        width: 8,
        height: 8,
        borderRadius: '50%',
        background: connected ? '#38a169' : '#e53e3e',
        boxShadow: `0 0 6px ${connected ? '#38a16980' : '#e53e3e80'}`,
      }} title={connected ? 'Connected' : 'Disconnected'} />

      <button
        onClick={() => openChat()}
        style={{
          background: '#2b6cb0',
          border: 'none',
          borderRadius: 8,
          padding: '6px 16px',
          color: '#fff',
          fontSize: 12,
          cursor: 'pointer',
          fontWeight: 600,
        }}
      >
        💬 团队沟通
      </button>
    </div>
  )
}

const ghostBtn: React.CSSProperties = {
  background: 'transparent',
  border: '1px solid #2d3748',
  borderRadius: 6,
  padding: '4px 10px',
  color: '#a0aec0',
  fontSize: 11,
  cursor: 'pointer',
  fontWeight: 500,
}
