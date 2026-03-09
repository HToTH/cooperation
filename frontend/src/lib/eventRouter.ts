import type { WsEvent } from './types'
import { wsClient } from './wsClient'
import { useExecutionStore } from '../stores/executionStore'
import { useChatStore } from '../stores/chatStore'
import { useGroupChatStore } from '../stores/groupChatStore'
import { useMemoryStore } from '../stores/memoryStore'

export function initEventRouter() {
  return wsClient.subscribe((event: WsEvent) => {
    const execution = useExecutionStore.getState()
    const chat = useChatStore.getState()
    const groupChat = useGroupChatStore.getState()
    const memory = useMemoryStore.getState()

    switch (event.type) {
      case 'workflow_state_changed':
        execution.handleWorkflowStateChanged(event.payload.workflow_id, event.payload.state)
        break

      case 'node_state_changed':
        execution.handleNodeStateChanged(event.payload.workflow_id, event.payload.node_id, event.payload.state)
        break

      case 'agent_message_sent':
        execution.handleAgentMessage(event.payload.workflow_id, event.payload.message)
        break

      case 'hitl_paused':
        chat.open()
        groupChat.mergeMessages([{
          type: 'hitl',
          id: `hitl_${event.payload.workflow_id}_${event.payload.node_id}`,
          workflow_id: event.payload.workflow_id,
          node_id: event.payload.node_id,
          context: event.payload.context,
          description: event.payload.description,
          status: 'pending',
          timestamp: Date.now(),
        }])
        break

      case 'workflow_completed':
        execution.handleWorkflowCompleted(event.payload.workflow_id, event.payload.summary, event.payload.results)
        break

      case 'workflow_aborted':
        execution.handleWorkflowAborted(event.payload.workflow_id, event.payload.reason)
        break

      case 'global_memory_query_result':
        memory.handleQueryResult(event.payload.results)
        break

      case 'error':
        console.error('[cooperation Error]', event.payload.code, event.payload.message)
        break

      default:
        console.warn('[WS] Unknown event type:', (event as { type: string }).type)
    }
  })
}
