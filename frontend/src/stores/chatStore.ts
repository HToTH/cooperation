import { create } from 'zustand'
import type { ChatMessage } from '../lib/api'

interface ChatState {
  isOpen: boolean
  activeAgentId: string | null
  /** conversations keyed by agentId */
  messages: Record<string, ChatMessage[]>
  pending: Record<string, boolean>
  open: (agentId?: string) => void
  close: () => void
  setActiveAgent: (id: string) => void
  addMessage: (agentId: string, msg: ChatMessage) => void
  setPending: (agentId: string, value: boolean) => void
}

export const useChatStore = create<ChatState>((set) => ({
  isOpen: false,
  activeAgentId: null,
  messages: {},
  pending: {},

  open: (agentId) => set((s) => ({
    isOpen: true,
    activeAgentId: agentId ?? s.activeAgentId,
  })),

  close: () => set({ isOpen: false }),

  setActiveAgent: (id) => set({ activeAgentId: id }),

  addMessage: (agentId, msg) =>
    set((s) => ({
      messages: {
        ...s.messages,
        [agentId]: [...(s.messages[agentId] ?? []), msg],
      },
    })),

  setPending: (agentId, value) =>
    set((s) => ({ pending: { ...s.pending, [agentId]: value } })),
}))
