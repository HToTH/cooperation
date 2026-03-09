import { Suspense, lazy, useCallback, useEffect, useRef, useState } from 'react'
import { useChatStore } from '../../stores/chatStore'
import { useGroupChatStore } from '../../stores/groupChatStore'
import { usePtyStore } from '../../stores/ptyStore'
import { useWorkflowStore } from '../../stores/workflowStore'
import { useExecutionStore } from '../../stores/executionStore'
import { loadGroupChatHistory, sendChatMessage, sendGroupChatMessage } from '../../lib/api'
import type { GroupAttachment, GroupHitlMessage, GroupTaskMessage } from '../../lib/api'
import type { AgentNode } from '../../lib/types'
import { wsClient } from '../../lib/wsClient'

const LazyPtyTerminal = lazy(() =>
  import('./PtyTerminal').then(({ PtyTerminal }) => ({ default: PtyTerminal })),
)

const NATIVE_KINDS = new Set(['claude_code', 'gemini_cli', 'codex'])

/** Map AgentKind → CLI program name */
function programForKind(kind: string): string {
  if (kind === 'claude_code') return 'claude'
  if (kind === 'gemini_cli') return 'gemini'
  if (kind === 'codex') return 'codex'
  return kind
}

const KIND_ICONS: Record<string, string> = {
  raw_llm: '⚙',
  claude_code: '🖥',
  gemini_cli: '✦',
  codex: '◈',
}

const COLORS = ['#3182ce', '#38a169', '#d69e2e', '#e53e3e', '#805ad5', '#dd6b20']
function agentColor(id: string) {
  let h = 0
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0
  return COLORS[h % COLORS.length]
}

// ─── Individual chat ──────────────────────────────────────────────────────────

function IndividualChat({ agents }: { agents: AgentNode[] }) {
  const graph = useWorkflowStore((s) => s.graph)
  const { activeAgentId, setActiveAgent, messages, pending, addMessage, setPending } = useChatStore()
  const { createSession, closeSession, getSession } = usePtyStore()

  const activeAgent: AgentNode | undefined = activeAgentId ? graph.nodes[activeAgentId] : agents[0]
  const activeId = activeAgent?.id ?? ''
  const chat = messages[activeId] ?? []
  const isPending = pending[activeId] ?? false
  const isNative = activeAgent ? NATIVE_KINDS.has(activeAgent.kind) : false
  const ptySession = activeAgent ? getSession(activeAgent.id) : undefined

  const [input, setInput] = useState('')
  const bottomRef = useRef<HTMLDivElement>(null)
  // Bump this to force a new PTY session after the old one exits
  const [ptyRevision, setPtyRevision] = useState(0)

  // Lazily create PTY session when switching to a native CLI agent tab, or after reconnect
  useEffect(() => {
    if (!activeAgent || !isNative) return
    if (getSession(activeAgent.id)) return
    const program = programForKind(activeAgent.kind)
    createSession(activeAgent.id, program, []).catch(console.error)
  }, [activeId, isNative, ptyRevision])

  const handlePtyExit = () => {
    if (activeAgent) void closeSession(activeAgent.id)
    // Do NOT auto-reconnect — let the user decide when to restart
  }

  const handlePtyRestart = () => {
    if (activeAgent) void closeSession(activeAgent.id)
    setPtyRevision((v) => v + 1)
  }

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [chat.length])

  const handleSend = async () => {
    const text = input.trim()
    if (!text || !activeAgent || isPending) return
    setInput('')
    addMessage(activeId, { role: 'user', content: text, timestamp: Date.now() })
    setPending(activeId, true)
    try {
      const response = await sendChatMessage(graph.id, activeId, activeAgent, text)
      addMessage(activeId, { role: 'assistant', content: response, timestamp: Date.now() })
    } catch (e) {
      addMessage(activeId, {
        role: 'assistant',
        content: `[Error: ${e instanceof Error ? e.message : String(e)}]`,
        timestamp: Date.now(),
      })
    } finally {
      setPending(activeId, false)
    }
  }

  return (
    <>
      {/* Agent tabs */}
      <div style={{
        display: 'flex', overflowX: 'auto', borderBottom: '1px solid #1e2533',
        padding: '8px 10px', gap: 6, flexShrink: 0,
      }}>
        {agents.length === 0 ? (
          <div style={{ fontSize: 12, color: '#718096', padding: '4px 8px' }}>先在画布上添加 Agent 节点</div>
        ) : agents.map((agent) => {
          const isActive = agent.id === activeId
          return (
            <button key={agent.id} onClick={() => setActiveAgent(agent.id)} style={{
              padding: '5px 12px', borderRadius: 20, border: 'none', cursor: 'pointer',
              fontSize: 11, fontWeight: 600, whiteSpace: 'nowrap', flexShrink: 0,
              background: isActive ? '#2b6cb0' : '#1e2533',
              color: isActive ? '#fff' : '#a0aec0',
            }}>
              {KIND_ICONS[agent.kind] ?? '⚙'} {agent.label}
              {messages[agent.id]?.length ? (
                <span style={{ marginLeft: 4, opacity: 0.7 }}>({messages[agent.id].length / 2 | 0})</span>
              ) : null}
            </button>
          )
        })}
      </div>

      {/* Agent info */}
      {activeAgent && (
        <div style={{
          padding: '8px 14px', borderBottom: '1px solid #1e2533',
          fontSize: 11, color: '#718096', flexShrink: 0,
          display: 'flex', alignItems: 'center', gap: 8,
        }}>
          <span style={{ color: '#a0aec0', fontWeight: 600 }}>{activeAgent.role}</span>
          <span>·</span>
          <span>{activeAgent.kind === 'raw_llm' ? `${activeAgent.model.provider} / ${activeAgent.model.model}` : activeAgent.kind}</span>
          {isNative && (
            <span style={{
              marginLeft: 'auto', fontSize: 10, padding: '2px 7px',
              borderRadius: 8, background: '#276749', color: '#9ae6b4', fontWeight: 600,
            }}>PTY</span>
          )}
          {isNative && ptySession && (
            <button
              onClick={handlePtyRestart}
              title="重启终端会话"
              style={{ background: 'none', border: 'none', color: '#718096', cursor: 'pointer', fontSize: 11 }}
            >重启</button>
          )}
        </div>
      )}

      {/* PTY terminal for native CLI agents */}
      {isNative ? (
        ptySession
          ? (
              <Suspense fallback={<TerminalLoadingFallback />}>
                <LazyPtyTerminal
                  sessionId={ptySession.sessionId}
                  onExit={handlePtyExit}
                />
              </Suspense>
            )
          : <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', color: '#4a5568', fontSize: 13 }}>
              <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 10 }}>
                <div>正在启动终端…</div>
                <button
                  onClick={handlePtyRestart}
                  style={{
                    background: '#1e2533',
                    border: '1px solid #2d3748',
                    borderRadius: 8,
                    padding: '6px 12px',
                    color: '#a0aec0',
                    cursor: 'pointer',
                    fontSize: 12,
                  }}
                >
                  重新启动
                </button>
              </div>
            </div>
      ) : (
        <>
          {/* Bubble messages for raw_llm */}
          <div style={{ flex: 1, overflowY: 'auto', padding: '12px 14px', display: 'flex', flexDirection: 'column', gap: 10 }}>
            {chat.length === 0 && (
              <div style={{ color: '#4a5568', fontSize: 13, textAlign: 'center', marginTop: 40 }}>
                和 {activeAgent?.label ?? 'agent'} 开始对话吧
              </div>
            )}
            {chat.map((msg, i) => (
              <div key={i} style={{ display: 'flex', flexDirection: 'column', alignItems: msg.role === 'user' ? 'flex-end' : 'flex-start' }}>
                <div style={{
                  maxWidth: '85%', padding: '8px 12px', borderRadius: 12,
                  fontSize: 13, lineHeight: 1.5, whiteSpace: 'pre-wrap', wordBreak: 'break-word',
                  background: msg.role === 'user' ? '#2b6cb0' : '#1a2d42',
                  color: msg.role === 'user' ? '#fff' : '#e2e8f0',
                  borderBottomRightRadius: msg.role === 'user' ? 4 : 12,
                  borderBottomLeftRadius: msg.role === 'assistant' ? 4 : 12,
                }}>
                  {msg.content}
                </div>
              </div>
            ))}
            {isPending && (
              <div style={{ display: 'flex', alignItems: 'flex-start' }}>
                <div style={{ padding: '8px 14px', background: '#1a2d42', borderRadius: 12, borderBottomLeftRadius: 4 }}>
                  <Dots />
                </div>
              </div>
            )}
            <div ref={bottomRef} />
          </div>

          {/* Input */}
          <div style={{ padding: '10px 14px', borderTop: '1px solid #1e2533', display: 'flex', gap: 8, flexShrink: 0 }}>
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSend() }
              }}
              placeholder={agents.length === 0 ? '先添加 Agent…' : '输入消息… (Enter 发送，Shift+Enter 换行)'}
              disabled={agents.length === 0 || isPending}
              rows={2}
              style={{
                flex: 1, background: '#1e2533', border: '1px solid #2d3748', borderRadius: 8,
                padding: '8px 10px', color: '#e2e8f0', fontSize: 13, resize: 'none',
                outline: 'none', lineHeight: 1.4, fontFamily: 'inherit',
              }}
            />
            <button
              onClick={handleSend}
              disabled={!input.trim() || agents.length === 0 || isPending}
              style={{
                background: input.trim() && !isPending ? '#2b6cb0' : '#1e2533',
                border: 'none', borderRadius: 8, padding: '0 14px',
                color: input.trim() && !isPending ? '#fff' : '#4a5568',
                cursor: input.trim() && !isPending ? 'pointer' : 'not-allowed',
                fontSize: 18, transition: 'all 0.15s',
              }}
            >➤</button>
          </div>
        </>
      )}
    </>
  )
}

// ─── Group chat ───────────────────────────────────────────────────────────────

function GroupChat({ agents }: { agents: AgentNode[] }) {
  const graph = useWorkflowStore((s) => s.graph)
  const loadGraph = useWorkflowStore((s) => s.loadGraph)
  const { messages, pending, setMessages, mergeMessages, addSystemMessage, setPending, resolveHitlMessage } = useGroupChatStore()

  const [input, setInput] = useState('')
  const [attachments, setAttachments] = useState<GroupAttachment[]>([])
  const [mentionQuery, setMentionQuery] = useState<string | null>(null)
  const [mentionStart, setMentionStart] = useState(0)

  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const bottomRef = useRef<HTMLDivElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages.length])

  useEffect(() => {
    let cancelled = false

    loadGroupChatHistory(graph.id)
      .then((history) => {
        if (!cancelled) setMessages(history)
      })
      .catch((error) => {
        if (!cancelled) {
          addSystemMessage(`加载团队沟通历史失败: ${error instanceof Error ? error.message : String(error)}`, 'warning')
        }
      })

    return () => {
      cancelled = true
    }
  }, [graph.id, setMessages, addSystemMessage])

  const parsedMentions = (): string[] => {
    const found = new Set<string>()
    const candidates = [...agents].sort((left, right) => right.label.length - left.label.length)

    for (let i = 0; i < input.length; i += 1) {
      if (input[i] !== '@') continue
      if (i > 0 && !/\s/.test(input[i - 1])) continue

      const remaining = input.slice(i + 1)
      const remainingLower = remaining.toLowerCase()
      const match = candidates.find((agent) => {
        const label = agent.label.toLowerCase()
        if (!remainingLower.startsWith(label)) return false

        const nextChar = remaining[label.length]
        return nextChar === undefined || /[\s,.!?;:，。！？；：、()[\]{}"']/.test(nextChar)
      })

      if (match) found.add(match.id)
    }

    return [...found]
  }

  const handleInputChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const val = e.target.value
    setInput(val)
    const cursor = e.target.selectionStart ?? val.length
    const textBefore = val.slice(0, cursor)
    const atIdx = textBefore.lastIndexOf('@')
    if (atIdx !== -1 && (atIdx === 0 || /\s/.test(textBefore[atIdx - 1]))) {
      const query = textBefore.slice(atIdx + 1)
      if (!query.includes(' ')) { setMentionQuery(query); setMentionStart(atIdx); return }
    }
    setMentionQuery(null)
  }

  const insertMention = (label: string) => {
    const before = input.slice(0, mentionStart)
    const after = input.slice(textareaRef.current?.selectionStart ?? input.length)
    setInput(`${before}@${label} ${after}`)
    setMentionQuery(null)
    setTimeout(() => textareaRef.current?.focus(), 0)
  }

  const filteredAgents = mentionQuery !== null
    ? agents.filter((a) => a.label.toLowerCase().startsWith(mentionQuery.toLowerCase()))
    : []

  const readFileAsBase64 = (file: File): Promise<GroupAttachment> =>
    new Promise((resolve, reject) => {
      const reader = new FileReader()
      reader.onload = () => {
        const data = (reader.result as string).split(',')[1] ?? ''
        resolve({ name: file.name, content_type: file.type || 'application/octet-stream', data })
      }
      reader.onerror = reject
      reader.readAsDataURL(file)
    })

  const addFiles = async (files: File[]) => {
    const atts = await Promise.all(files.map(readFileAsBase64))
    setAttachments((prev) => [...prev, ...atts])
  }

  const handlePaste = useCallback(async (e: React.ClipboardEvent) => {
    const files = Array.from(e.clipboardData.items)
      .filter((i) => i.kind === 'file')
      .map((i) => i.getAsFile())
      .filter((f): f is File => f !== null)
    if (files.length > 0) { e.preventDefault(); await addFiles(files) }
  }, [])

  const handleSend = async () => {
    const text = input.trim()
    const mentions = parsedMentions()
    if ((!text && attachments.length === 0) || pending) return

    setInput('')
    setAttachments([])
    setPending(true)

    try {
      const response = await sendGroupChatMessage(
        graph.id,
        text,
        mentions,
        graph,
        attachments.length > 0 ? attachments : undefined,
      )
      if (response.workflow_graph) {
        loadGraph(response.workflow_graph)
      }
      mergeMessages(response.messages)
    } catch (e) {
      addSystemMessage(`团队沟通失败: ${e instanceof Error ? e.message : String(e)}`, 'warning')
    } finally {
      setPending(false)
    }
  }

  const handleHitlDecision = (message: GroupHitlMessage, approved: boolean) => {
    const reason = approved
      ? undefined
      : (window.prompt('输入拒绝原因', 'User rejected') ?? 'User rejected')

    wsClient.send({
      type: 'hitl_resume',
      payload: {
        workflow_id: message.workflow_id,
        node_id: message.node_id,
        decision: approved ? 'approved' : { rejected: { reason: reason ?? 'User rejected' } },
      },
    })

    resolveHitlMessage(
      message.workflow_id,
      message.node_id,
      approved ? 'approved' : 'rejected',
      reason,
    )
  }

  return (
    <>
      {/* Agent roster */}
      <div style={{
        display: 'flex', flexWrap: 'wrap', gap: 6, padding: '8px 12px',
        borderBottom: '1px solid #1e2533', flexShrink: 0,
      }}>
        {agents.length === 0 ? (
          <span style={{ fontSize: 11, color: '#718096' }}>先在画布上添加 Agent 节点</span>
        ) : agents.map((a) => (
          <span key={a.id} style={{
            fontSize: 10, padding: '2px 8px', borderRadius: 10,
            background: agentColor(a.id) + '33', color: agentColor(a.id),
            border: `1px solid ${agentColor(a.id)}66`, fontWeight: 600,
          }}>
            {KIND_ICONS[a.kind] ?? '⚙'} {a.label}
          </span>
        ))}
      </div>

      {/* Messages */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '12px 14px', display: 'flex', flexDirection: 'column', gap: 8 }}>
        {messages.length === 0 && (
          <div style={{ color: '#4a5568', fontSize: 13, textAlign: 'center', marginTop: 40 }}>
            使用 @名称 提及 Agent，或输入 /help 查看聊天命令
          </div>
        )}
        {messages.map((msg, i) => {
          if (msg.type === 'user') {
            return (
              <div key={i} style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 4 }}>
                {msg.attachments.length > 0 && (
                  <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', justifyContent: 'flex-end' }}>
                    {msg.attachments.map((att, ai) => <AttachmentThumb key={ai} att={att} />)}
                  </div>
                )}
                <div style={{
                  maxWidth: '85%', padding: '8px 12px', borderRadius: 12, borderBottomRightRadius: 4,
                  background: '#2b6cb0', color: '#fff', fontSize: 13, lineHeight: 1.5,
                  whiteSpace: 'pre-wrap', wordBreak: 'break-word',
                }}>
                  <MentionHighlight text={msg.content} agents={agents} />
                </div>
                {msg.mentioned_agent_ids.length > 0 && (
                  <div style={{ fontSize: 10, color: '#90cdf4' }}>
                    发送给 {msg.mentioned_agent_ids.map((id) => graph.nodes[id]?.label ?? id).join('、')}
                  </div>
                )}
              </div>
            )
          }
          if (msg.type === 'system') {
            return (
              <div key={i} style={{ display: 'flex', justifyContent: 'center' }}>
                <div style={{
                  maxWidth: '90%',
                  padding: '8px 12px',
                  borderRadius: 10,
                  background: msg.level === 'warning' ? '#74421033' : '#1e2533',
                  border: `1px solid ${msg.level === 'warning' ? '#d69e2e66' : '#2d3748'}`,
                  color: msg.level === 'warning' ? '#f6e05e' : '#a0aec0',
                  fontSize: 12,
                  lineHeight: 1.5,
                  whiteSpace: 'pre-wrap',
                  wordBreak: 'break-word',
                }}>
                  {msg.content}
                </div>
              </div>
            )
          }
          if (msg.type === 'task') {
            return <TaskStatusCard key={i} msg={msg} graph={graph} />
          }
          if (msg.type === 'hitl') {
            return <HitlCard key={i} msg={msg} onDecision={handleHitlDecision} />
          }

          const agent = graph.nodes[msg.agent_id]
          const color = agentColor(msg.agent_id)
          return (
            <div key={i} style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 2 }}>
              <span style={{ fontSize: 10, color, fontWeight: 600, paddingLeft: 4 }}>
                {KIND_ICONS[agent?.kind ?? ''] ?? '⚙'} {agent?.label ?? msg.agent_id}
              </span>
              <div style={{
                maxWidth: '85%', padding: '8px 12px', borderRadius: 12, borderBottomLeftRadius: 4,
                background: color + '22', border: `1px solid ${color}44`,
                color: '#e2e8f0', fontSize: 13, lineHeight: 1.5,
                whiteSpace: 'pre-wrap', wordBreak: 'break-word',
              }}>
                {msg.content}
              </div>
            </div>
          )
        })}
        {pending && (
          <div style={{ display: 'flex' }}>
            <div style={{ padding: '8px 14px', background: '#1a2d42', borderRadius: 12, borderBottomLeftRadius: 4 }}>
              <Dots />
            </div>
          </div>
        )}
        <div ref={bottomRef} />
      </div>

      {/* Attachment preview strip */}
      {attachments.length > 0 && (
        <div style={{ display: 'flex', gap: 8, padding: '8px 14px', borderTop: '1px solid #1e2533', flexWrap: 'wrap', flexShrink: 0 }}>
          {attachments.map((att, i) => (
            <div key={i} style={{ position: 'relative' }}>
              <AttachmentThumb att={att} />
              <button onClick={() => setAttachments((p) => p.filter((_, j) => j !== i))} style={{
                position: 'absolute', top: -4, right: -4, width: 16, height: 16,
                background: '#e53e3e', border: 'none', borderRadius: '50%',
                color: '#fff', fontSize: 9, cursor: 'pointer',
                display: 'flex', alignItems: 'center', justifyContent: 'center',
              }}>✕</button>
            </div>
          ))}
        </div>
      )}

      {/* @mention autocomplete */}
      {mentionQuery !== null && filteredAgents.length > 0 && (
        <div style={{
          position: 'absolute', bottom: 80, left: 14, right: 14,
          background: '#1a2332', border: '1px solid #2d3748',
          borderRadius: 8, zIndex: 300, boxShadow: '0 4px 20px #00000060',
        }}>
          {filteredAgents.map((a) => (
            <div key={a.id} onClick={() => insertMention(a.label)} style={{
              padding: '7px 12px', cursor: 'pointer', fontSize: 12,
              color: '#e2e8f0', display: 'flex', alignItems: 'center', gap: 6,
            }}
              onMouseEnter={(e) => (e.currentTarget.style.background = '#243447')}
              onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
            >
              <span style={{ color: agentColor(a.id) }}>{KIND_ICONS[a.kind] ?? '⚙'}</span>
              <span style={{ fontWeight: 600 }}>{a.label}</span>
              <span style={{ color: '#718096', fontSize: 10 }}>{a.role}</span>
            </div>
          ))}
        </div>
      )}

      {/* Input */}
      <div style={{ padding: '10px 14px', borderTop: '1px solid #1e2533', flexShrink: 0 }}>
        <div style={{ display: 'flex', gap: 8, alignItems: 'flex-end' }}>
          <button onClick={() => fileInputRef.current?.click()} title="上传文件" style={{
            background: '#1e2533', border: '1px solid #2d3748', borderRadius: 8,
            padding: '8px 10px', color: '#a0aec0', cursor: 'pointer', fontSize: 15, flexShrink: 0,
          }}>📎</button>
          <input ref={fileInputRef} type="file" multiple style={{ display: 'none' }}
            onChange={(e) => e.target.files && addFiles(Array.from(e.target.files))} />

          <textarea
            ref={textareaRef}
            value={input}
            onChange={handleInputChange}
            onPaste={handlePaste}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSend() }
              if (e.key === 'Escape') setMentionQuery(null)
            }}
            placeholder={agents.length === 0 ? '输入 /add-agent 创建第一个 Agent，或 /help' : '不 @ 默认发给全部 Agent，@名称 只发给指定 Agent，/run 执行当前 workflow'}
            disabled={pending}
            rows={2}
            style={{
              flex: 1, background: '#1e2533', border: '1px solid #2d3748', borderRadius: 8,
              padding: '8px 10px', color: '#e2e8f0', fontSize: 13, resize: 'none',
              outline: 'none', lineHeight: 1.4, fontFamily: 'inherit',
            }}
          />
          <button onClick={handleSend}
            disabled={(!input.trim() && attachments.length === 0) || pending}
            style={{
              background: (input.trim() || attachments.length > 0) && !pending ? '#2b6cb0' : '#1e2533',
              border: 'none', borderRadius: 8, padding: '0 14px', alignSelf: 'stretch',
              color: (input.trim() || attachments.length > 0) && !pending ? '#fff' : '#4a5568',
              cursor: (input.trim() || attachments.length > 0) && !pending ? 'pointer' : 'not-allowed',
              fontSize: 18, flexShrink: 0,
            }}
          >➤</button>
        </div>
        <div style={{ fontSize: 10, color: '#4a5568', marginTop: 4 }}>
          可粘贴文件或图片 · Shift+Enter 换行 · 不 @ 默认广播 · @名称 定向发送 · /run 执行 · /help 查看命令
        </div>
      </div>
    </>
  )
}

// ─── Root panel ───────────────────────────────────────────────────────────────

export function ChatPanel() {
  const { isOpen, close, activeAgentId, setActiveAgent } = useChatStore()
  const graph = useWorkflowStore((s) => s.graph)
  const activityCount = useExecutionStore((s) => s.activityLog.length)
  const clearLog = useExecutionStore((s) => s.clearLog)
  const [tab, setTab] = useState<'individual' | 'group'>('group')

  const agents = Object.values(graph.nodes).filter((n) => n.role !== 'human_in_loop')

  // Auto-select first agent when panel opens
  useEffect(() => {
    if (isOpen && !activeAgentId && agents.length > 0) {
      setActiveAgent(agents[0].id)
    }
  }, [isOpen, agents.length])

  if (!isOpen) return null

  return (
    <div style={{
      position: 'fixed', right: 0, top: 0, bottom: 0, width: 420,
      background: '#0f1117', borderLeft: '1px solid #1e2533',
      display: 'flex', flexDirection: 'column', zIndex: 200,
      boxShadow: '-4px 0 24px #00000080',
    }}>
      {/* Header */}
      <div style={{
        height: 48, borderBottom: '1px solid #1e2533',
        display: 'flex', alignItems: 'center', padding: '0 14px', gap: 8,
      }}>
        <span style={{ fontWeight: 700, fontSize: 14, color: '#63b3ed', flex: 1 }}>团队沟通</span>
        <button
          onClick={clearLog}
          disabled={activityCount === 0}
          style={{
            background: 'transparent',
            border: '1px solid #2d3748',
            borderRadius: 6,
            padding: '4px 8px',
            color: activityCount > 0 ? '#a0aec0' : '#4a5568',
            cursor: activityCount > 0 ? 'pointer' : 'not-allowed',
            fontSize: 11,
            fontWeight: 600,
          }}
        >
          Clear Log
        </button>
        <button onClick={close} style={{ background: 'none', border: 'none', color: '#718096', cursor: 'pointer', fontSize: 18, lineHeight: 1 }}>✕</button>
      </div>

      {/* Mode tabs */}
      <div style={{ display: 'flex', borderBottom: '1px solid #1e2533', flexShrink: 0 }}>
        {(['individual', 'group'] as const).map((t) => (
          <button key={t} onClick={() => setTab(t)} style={{
            flex: 1, padding: '8px 0', border: 'none', cursor: 'pointer',
            background: 'transparent', fontSize: 12, fontWeight: 600,
            color: tab === t ? '#63b3ed' : '#718096',
            borderBottom: tab === t ? '2px solid #63b3ed' : '2px solid transparent',
          }}>
            {t === 'individual' ? '单聊' : '群聊'}
          </button>
        ))}
      </div>

      {/* Content */}
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden', position: 'relative' }}>
        {tab === 'individual'
          ? <IndividualChat agents={agents} />
          : <GroupChat agents={agents} />}
      </div>
    </div>
  )
}

// ─── Shared helpers ───────────────────────────────────────────────────────────

function MentionHighlight({ text, agents }: { text: string; agents: { id: string; label: string }[] }) {
  return (
    <>
      {text.split(/(@\S+)/g).map((part, i) => {
        if (part.startsWith('@')) {
          const label = part.slice(1)
          const found = agents.find((a) => a.label.toLowerCase() === label.toLowerCase())
          if (found) return <span key={i} style={{ background: '#ffffff22', borderRadius: 4, padding: '0 3px', fontWeight: 700 }}>{part}</span>
        }
        return <span key={i}>{part}</span>
      })}
    </>
  )
}

function TaskStatusCard({ msg, graph }: { msg: GroupTaskMessage; graph: ReturnType<typeof useWorkflowStore.getState>['graph'] }) {
  const agent = graph.nodes[msg.agent_id]
  const color = agentColor(msg.agent_id)
  const statusLabel: Record<GroupTaskMessage['status'], string> = {
    queued: '排队中',
    running: '处理中',
    completed: '已完成',
    failed: '失败',
    blocked: '待授权',
  }
  const statusColor: Record<GroupTaskMessage['status'], string> = {
    queued: '#90cdf4',
    running: '#63b3ed',
    completed: '#68d391',
    failed: '#fc8181',
    blocked: '#f6ad55',
  }

  return (
    <div style={{
      border: `1px solid ${color}55`,
      background: '#111827',
      borderRadius: 12,
      padding: '10px 12px',
      display: 'flex',
      flexDirection: 'column',
      gap: 6,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <span style={{ fontSize: 10, color, fontWeight: 700 }}>
          {KIND_ICONS[agent?.kind ?? ''] ?? '⚙'} {agent?.label ?? msg.agent_id}
        </span>
        <span style={{
          marginLeft: 'auto',
          fontSize: 10,
          color: statusColor[msg.status],
          background: `${statusColor[msg.status]}22`,
          border: `1px solid ${statusColor[msg.status]}44`,
          borderRadius: 999,
          padding: '2px 8px',
          fontWeight: 700,
        }}>
          {statusLabel[msg.status]}
        </span>
      </div>
      <div style={{ fontSize: 12, color: '#cbd5e0', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
        {msg.command}
      </div>
      {msg.summary && (
        <div style={{ fontSize: 11, color: '#94a3b8', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
          {msg.summary}
        </div>
      )}
    </div>
  )
}

function HitlCard({
  msg,
  onDecision,
}: {
  msg: GroupHitlMessage
  onDecision: (message: GroupHitlMessage, approved: boolean) => void
}) {
  const statusColor: Record<GroupHitlMessage['status'], string> = {
    pending: '#f6ad55',
    approved: '#68d391',
    rejected: '#fc8181',
  }
  const statusLabel: Record<GroupHitlMessage['status'], string> = {
    pending: '待审批',
    approved: '已批准',
    rejected: '已拒绝',
  }

  return (
    <div style={{
      border: `1px solid ${statusColor[msg.status]}55`,
      background: '#111827',
      borderRadius: 12,
      padding: '12px',
      display: 'flex',
      flexDirection: 'column',
      gap: 8,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <span style={{ fontSize: 12, fontWeight: 700, color: '#e2e8f0' }}>Human Review</span>
        <span style={{
          marginLeft: 'auto',
          fontSize: 10,
          color: statusColor[msg.status],
          background: `${statusColor[msg.status]}22`,
          border: `1px solid ${statusColor[msg.status]}44`,
          borderRadius: 999,
          padding: '2px 8px',
          fontWeight: 700,
        }}>
          {statusLabel[msg.status]}
        </span>
      </div>
      <div style={{ fontSize: 12, color: '#cbd5e0', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
        {msg.description}
      </div>
      <pre style={{
        margin: 0,
        padding: '8px 10px',
        borderRadius: 8,
        background: '#0f1117',
        color: '#94a3b8',
        fontSize: 11,
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
      }}>
        {JSON.stringify(msg.context, null, 2)}
      </pre>
      {msg.reason && (
        <div style={{ fontSize: 11, color: '#fca5a5' }}>
          原因: {msg.reason}
        </div>
      )}
      {msg.status === 'pending' && (
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <button
            onClick={() => onDecision(msg, false)}
            style={{
              background: 'transparent',
              border: '1px solid #e53e3e',
              color: '#fc8181',
              borderRadius: 8,
              padding: '6px 12px',
              cursor: 'pointer',
              fontSize: 12,
              fontWeight: 600,
            }}
          >
            Reject
          </button>
          <button
            onClick={() => onDecision(msg, true)}
            style={{
              background: '#2f855a',
              border: 'none',
              color: '#fff',
              borderRadius: 8,
              padding: '6px 12px',
              cursor: 'pointer',
              fontSize: 12,
              fontWeight: 600,
            }}
          >
            Approve
          </button>
        </div>
      )}
    </div>
  )
}

function AttachmentThumb({ att }: { att: GroupAttachment }) {
  if (att.content_type.startsWith('image/')) {
    return (
      <img src={`data:${att.content_type};base64,${att.data}`} alt={att.name}
        style={{ width: 56, height: 56, objectFit: 'cover', borderRadius: 6, border: '1px solid #2d3748' }} />
    )
  }
  return (
    <div style={{
      width: 56, height: 56, background: '#1e2533', borderRadius: 6,
      border: '1px solid #2d3748', display: 'flex', flexDirection: 'column',
      alignItems: 'center', justifyContent: 'center',
    }}>
      <span style={{ fontSize: 18 }}>📄</span>
      <span style={{ fontSize: 8, color: '#718096', textAlign: 'center', wordBreak: 'break-all', lineHeight: 1.1, marginTop: 2, padding: '0 2px' }}>
        {att.name.slice(0, 10)}
      </span>
    </div>
  )
}

function Dots() {
  return (
    <div style={{ display: 'flex', gap: 4, padding: '2px 0' }}>
      {[0, 1, 2].map((i) => (
        <div key={i} style={{
          width: 6, height: 6, borderRadius: '50%', background: '#63b3ed',
          animation: `pulse 1.2s ease-in-out ${i * 0.2}s infinite`,
        }} />
      ))}
      <style>{`@keyframes pulse { 0%,80%,100%{opacity:.2;transform:scale(1)} 40%{opacity:1;transform:scale(1.2)} }`}</style>
    </div>
  )
}

function TerminalLoadingFallback() {
  return (
    <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', color: '#4a5568', fontSize: 13 }}>
      正在加载终端组件…
    </div>
  )
}
