import { Suspense, lazy, useEffect, useState } from 'react'
import { AgentFlowCanvas } from './components/canvas/AgentFlowCanvas'
import { ActivityLog } from './components/panels/ActivityLog'
import { GlobalMemoryPanel } from './components/panels/GlobalMemoryPanel'
import { NodeConfigPanel } from './components/panels/NodeConfigPanel'
import { NodeEditPanel } from './components/panels/NodeEditPanel'
import { Toolbar } from './components/Toolbar'
import { initEventRouter } from './lib/eventRouter'
import { useChatStore } from './stores/chatStore'
import { useWsStore } from './stores/wsStore'
import { useWorkflowStore } from './stores/workflowStore'

const LazyChatPanel = lazy(() =>
  import('./components/panels/ChatPanel').then(({ ChatPanel }) => ({ default: ChatPanel })),
)

export function App() {
  const connect = useWsStore((s) => s.connect)
  const disconnect = useWsStore((s) => s.disconnect)
  const selectedNodeId = useWorkflowStore((s) => s.selectedNodeId)

  useEffect(() => {
    connect()
    const unsubscribe = initEventRouter()
    return () => {
      unsubscribe()
      disconnect()
    }
  }, [connect, disconnect])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', background: '#0f1117' }}>
      <Toolbar />

      <div style={{ display: 'flex', flex: 1, overflow: 'hidden' }}>
        {/* Left sidebar: add node or edit selected node */}
        <div style={{ width: 240, display: 'flex', flexDirection: 'column', borderRight: '1px solid #1e2533', overflow: 'auto' }}>
          {selectedNodeId ? <NodeEditPanel /> : <NodeConfigPanel />}
        </div>

        {/* Main canvas */}
        <div style={{ flex: 1, position: 'relative' }}>
          <AgentFlowCanvas />
        </div>

        {/* Right sidebar: logs + memory */}
        <div style={{
          width: 300,
          display: 'flex',
          flexDirection: 'column',
          borderLeft: '1px solid #1e2533',
        }}>
          <div style={{ flex: 1, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}>
            <ActivityLog />
          </div>
          <div style={{ height: 280, borderTop: '1px solid #1e2533', display: 'flex', flexDirection: 'column' }}>
            <GlobalMemoryPanel />
          </div>
        </div>
      </div>
      <DeferredChatPanel />
    </div>
  )
}

function DeferredChatPanel() {
  const isOpen = useChatStore((s) => s.isOpen)
  const [hasOpened, setHasOpened] = useState(false)

  useEffect(() => {
    if (isOpen) setHasOpened(true)
  }, [isOpen])

  if (!hasOpened && !isOpen) return null

  return (
    <Suspense fallback={isOpen ? <ChatPanelFallback /> : null}>
      <LazyChatPanel />
    </Suspense>
  )
}

function ChatPanelFallback() {
  return (
    <div style={{
      position: 'fixed', right: 0, top: 0, bottom: 0, width: 420,
      background: '#0f1117', borderLeft: '1px solid #1e2533',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      color: '#63b3ed', zIndex: 200, boxShadow: '-4px 0 24px #00000080',
    }}>
      正在加载团队沟通…
    </div>
  )
}
