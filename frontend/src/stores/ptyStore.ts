import { create } from 'zustand'

export interface PtySession {
  sessionId: string
  agentId: string
}

interface PtyState {
  /** agentId → active PTY session */
  sessions: Record<string, PtySession>
  createSession: (agentId: string, program: string, args: string[]) => Promise<PtySession>
  closeSession: (agentId: string) => Promise<void>
  getSession: (agentId: string) => PtySession | undefined
}

const API_BASE = 'http://localhost:8080'

/** Deduplicates concurrent createSession calls for the same agentId */
const inFlight = new Map<string, Promise<PtySession>>()

export const usePtyStore = create<PtyState>((set, get) => ({
  sessions: {},

  getSession: (agentId) => get().sessions[agentId],

  createSession: async (agentId, program, args) => {
    const existing = get().sessions[agentId]
    if (existing) return existing

    // Return the already-in-flight promise to avoid duplicate spawns
    const pending = inFlight.get(agentId)
    if (pending) return pending

    const promise = (async () => {
      try {
        const res = await fetch(`${API_BASE}/api/pty`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ agent_id: agentId, program, args, cols: 220, rows: 50 }),
        })
        if (!res.ok) {
          const body = await res.text().catch(() => '')
          throw new Error(`Failed to create PTY session: ${res.status} — ${body}`)
        }
        const data = await res.json()
        const session: PtySession = { sessionId: data.session_id, agentId }
        set((s) => ({ sessions: { ...s.sessions, [agentId]: session } }))
        return session
      } finally {
        inFlight.delete(agentId)
      }
    })()

    inFlight.set(agentId, promise)
    return promise
  },

  closeSession: async (agentId) => {
    const session = get().sessions[agentId]
    if (!session) return

    await fetch(`${API_BASE}/api/pty/${session.sessionId}`, { method: 'DELETE' }).catch(() => {})

    set((s) => {
      const next = { ...s.sessions }
      delete next[agentId]
      return { sessions: next }
    })
  },
}))
