import { useState } from 'react'
import { useHitlStore } from '../../stores/hitlStore'

export function HitlModal() {
  const pending = useHitlStore((s) => s.pending)
  const approve = useHitlStore((s) => s.approve)
  const reject = useHitlStore((s) => s.reject)
  const [rejectReason, setRejectReason] = useState('')
  const [showRejectInput, setShowRejectInput] = useState(false)

  if (!pending) return null

  const handleReject = () => {
    if (showRejectInput) {
      reject(rejectReason || 'User rejected')
      setRejectReason('')
      setShowRejectInput(false)
    } else {
      setShowRejectInput(true)
    }
  }

  return (
    <div style={{
      position: 'fixed',
      inset: 0,
      background: 'rgba(0,0,0,0.7)',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      zIndex: 1000,
    }}>
      <div style={{
        background: '#1a2332',
        border: '1px solid #d69e2e',
        borderRadius: 12,
        padding: 28,
        maxWidth: 520,
        width: '90%',
        boxShadow: '0 20px 60px rgba(0,0,0,0.5)',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 16 }}>
          <span style={{ fontSize: 24 }}>🧑‍⚖️</span>
          <h2 style={{ color: '#e2e8f0', fontSize: 16, fontWeight: 700 }}>Human Review Required</h2>
        </div>

        <p style={{ color: '#a0aec0', fontSize: 13, lineHeight: 1.6, marginBottom: 16 }}>
          {pending.description}
        </p>

        <div style={{
          background: '#0f1117',
          borderRadius: 8,
          padding: 12,
          marginBottom: 20,
          maxHeight: 200,
          overflowY: 'auto',
        }}>
          <div style={{ fontSize: 11, color: '#718096', marginBottom: 4 }}>Context:</div>
          <pre style={{
            fontSize: 11,
            color: '#a0aec0',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-all',
            margin: 0,
          }}>
            {JSON.stringify(pending.context, null, 2)}
          </pre>
        </div>

        {showRejectInput && (
          <input
            autoFocus
            value={rejectReason}
            onChange={(e) => setRejectReason(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleReject()}
            placeholder="Reason for rejection (optional)"
            style={{
              width: '100%',
              marginBottom: 12,
              background: '#0f1117',
              border: '1px solid #e53e3e',
              borderRadius: 6,
              padding: '8px 12px',
              color: '#e2e8f0',
              fontSize: 13,
              outline: 'none',
            }}
          />
        )}

        <div style={{ display: 'flex', gap: 10, justifyContent: 'flex-end' }}>
          <button
            onClick={handleReject}
            style={{
              background: showRejectInput ? '#e53e3e' : 'transparent',
              border: '1px solid #e53e3e',
              borderRadius: 8,
              padding: '8px 20px',
              color: showRejectInput ? '#fff' : '#e53e3e',
              fontSize: 13,
              cursor: 'pointer',
              fontWeight: 600,
            }}
          >
            {showRejectInput ? 'Confirm Reject' : 'Reject'}
          </button>
          <button
            onClick={approve}
            style={{
              background: '#38a169',
              border: 'none',
              borderRadius: 8,
              padding: '8px 20px',
              color: '#fff',
              fontSize: 13,
              cursor: 'pointer',
              fontWeight: 600,
            }}
          >
            Approve
          </button>
        </div>
      </div>
    </div>
  )
}
