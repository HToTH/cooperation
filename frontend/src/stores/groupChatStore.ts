import { create } from 'zustand'
import type { GroupChatMessage, GroupHitlMessage, GroupHitlStatus } from '../lib/api'

interface GroupChatState {
  messages: GroupChatMessage[]
  pending: boolean
  setMessages: (messages: GroupChatMessage[]) => void
  mergeMessages: (messages: GroupChatMessage[]) => void
  addSystemMessage: (content: string, level?: 'info' | 'warning') => void
  resolveHitlMessage: (
    workflowId: string,
    nodeId: string,
    status: Exclude<GroupHitlStatus, 'pending'>,
    reason?: string,
  ) => void
  setPending: (value: boolean) => void
}

function messageKey(message: GroupChatMessage): string {
  if (message.type === 'task') return `task:${message.task_id}`
  if (message.type === 'hitl') return `hitl:${message.workflow_id}:${message.node_id}`
  return `id:${message.id}`
}

function mergeGroupMessages(
  existing: GroupChatMessage[],
  incoming: GroupChatMessage[],
): GroupChatMessage[] {
  const merged = new Map<string, GroupChatMessage>()

  for (const message of [...existing, ...incoming]) {
    const key = messageKey(message)
    const prev = merged.get(key)
    if (!prev || prev.timestamp <= message.timestamp) {
      merged.set(key, message)
    }
  }

  return Array.from(merged.values()).sort((a, b) => a.timestamp - b.timestamp)
}

export const useGroupChatStore = create<GroupChatState>((set) => ({
  messages: [],
  pending: false,

  setMessages: (messages) => set({ messages: mergeGroupMessages([], messages) }),

  mergeMessages: (messages) =>
    set((state) => ({ messages: mergeGroupMessages(state.messages, messages) })),

  addSystemMessage: (content, level = 'info') =>
    set((state) => ({
      messages: mergeGroupMessages(state.messages, [
        {
          type: 'system',
          id: `local_${Date.now()}`,
          content,
          level,
          timestamp: Date.now(),
        },
      ]),
    })),

  resolveHitlMessage: (workflowId, nodeId, status, reason) =>
    set((state) => {
      const existing = state.messages.find(
        (message): message is GroupHitlMessage =>
          message.type === 'hitl' &&
          message.workflow_id === workflowId &&
          message.node_id === nodeId,
      )
      if (!existing) return state

      const updated: GroupHitlMessage = {
        ...existing,
        status,
        reason: reason ?? existing.reason,
        timestamp: Date.now(),
      }

      return { messages: mergeGroupMessages(state.messages, [updated]) }
    }),

  setPending: (value) => set({ pending: value }),
}))
