import { useExecutionStore } from '../../stores/executionStore'

const typeColors = {
  state_change: '#3182ce',
  message: '#805ad5',
  error: '#e53e3e',
  completed: '#38a169',
}

const typeIcons = {
  state_change: '⚡',
  message: '💬',
  error: '❌',
  completed: '✅',
}

export function ActivityLog() {
  const log = useExecutionStore((s) => s.activityLog)
  const workflowState = useExecutionStore((s) => s.workflowState)
  const clearLog = useExecutionStore((s) => s.clearLog)
  const canClear = log.length > 0

  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      height: '100%',
      background: '#0f1117',
      borderLeft: '1px solid #1e2533',
    }}>
      <div style={{
        padding: '10px 14px',
        borderBottom: '1px solid #1e2533',
        display: 'flex',
        justifyContent: 'space-between',
        alignItems: 'center',
      }}>
        <div>
          <span style={{ fontWeight: 700, fontSize: 12, color: '#e2e8f0' }}>Activity Log</span>
          <span style={{
            marginLeft: 8,
            fontSize: 10,
            padding: '2px 6px',
            borderRadius: 10,
            background: '#1e2533',
            color: '#718096',
          }}>
            {workflowState}
          </span>
        </div>
        <button
          onClick={clearLog}
          disabled={!canClear}
          style={{
            background: 'none',
            border: 'none',
            color: canClear ? '#4a5568' : '#2d3748',
            cursor: canClear ? 'pointer' : 'not-allowed',
            fontSize: 11,
          }}
        >
          Clear Log
        </button>
      </div>

      <div style={{ flex: 1, overflowY: 'auto', padding: '8px 0' }}>
        {log.length === 0 && (
          <div style={{ padding: '24px 14px', color: '#4a5568', fontSize: 12, textAlign: 'center' }}>
            No activity yet. Start a workflow to see events.
          </div>
        )}
        {[...log].reverse().map((entry) => (
          <div key={entry.id} style={{
            padding: '6px 14px',
            borderBottom: '1px solid #1a1f2e',
            display: 'flex',
            gap: 8,
            alignItems: 'flex-start',
          }}>
            <span style={{ fontSize: 12 }}>{typeIcons[entry.type]}</span>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 11, color: typeColors[entry.type], wordBreak: 'break-word' }}>
                {entry.content}
              </div>
              <div style={{ fontSize: 10, color: '#4a5568', marginTop: 2 }}>
                {new Date(entry.timestamp).toLocaleTimeString()}
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
