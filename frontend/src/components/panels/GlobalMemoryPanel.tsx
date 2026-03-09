import { useMemoryStore } from '../../stores/memoryStore'
import { useWorkflowStore } from '../../stores/workflowStore'

export function GlobalMemoryPanel() {
  const entries = useMemoryStore((s) => s.entries)
  const query = useMemoryStore((s) => s.query)
  const setQuery = useMemoryStore((s) => s.setQuery)
  const search = useMemoryStore((s) => s.search)
  const workflowId = useWorkflowStore((s) => s.graph.id)

  const handleSearch = () => search(workflowId)

  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      height: '100%',
      background: '#0f1117',
    }}>
      <div style={{ padding: '10px 14px', borderBottom: '1px solid #1e2533' }}>
        <span style={{ fontWeight: 700, fontSize: 12, color: '#e2e8f0' }}>Global Memory</span>
      </div>

      <div style={{ padding: '10px 14px', borderBottom: '1px solid #1e2533', display: 'flex', gap: 6 }}>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
          placeholder="Search memory..."
          style={{
            flex: 1,
            background: '#1a2332',
            border: '1px solid #2d3748',
            borderRadius: 6,
            padding: '6px 10px',
            color: '#e2e8f0',
            fontSize: 12,
            outline: 'none',
          }}
        />
        <button
          onClick={handleSearch}
          style={{
            background: '#2b6cb0',
            border: 'none',
            borderRadius: 6,
            padding: '6px 12px',
            color: '#fff',
            fontSize: 12,
            cursor: 'pointer',
          }}
        >
          Search
        </button>
      </div>

      <div style={{ flex: 1, overflowY: 'auto', padding: 8 }}>
        {entries.length === 0 && (
          <div style={{ color: '#4a5568', fontSize: 12, textAlign: 'center', padding: 24 }}>
            No memory entries yet.
          </div>
        )}
        {entries.map((entry, i) => (
          <div key={i} style={{
            background: '#1a2332',
            borderRadius: 8,
            padding: 10,
            marginBottom: 6,
            fontSize: 11,
            color: '#a0aec0',
            wordBreak: 'break-all',
          }}>
            <pre style={{ whiteSpace: 'pre-wrap', margin: 0 }}>
              {JSON.stringify(entry, null, 2)}
            </pre>
          </div>
        ))}
      </div>
    </div>
  )
}
