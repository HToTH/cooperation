/**
 * PtyTerminal — xterm.js wired to a backend PTY session over WebSocket.
 *
 * Protocol:
 *   WS binary frame → xterm write (PTY output)
 *   xterm onData   → WS binary send (keystrokes / paste)
 *   text frame "resize:{cols},{rows}" → PTY resize
 */
import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { WS_BASE } from '../../lib/runtime'

interface Props {
  sessionId: string
  /** Called when the WS connection is lost (PTY exited) */
  onExit?: () => void
}

export function PtyTerminal({ sessionId, onExit }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const termRef = useRef<Terminal | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const [status, setStatus] = useState<'connecting' | 'connected' | 'closed'>('connecting')

  useEffect(() => {
    if (!containerRef.current) return
    let intentionalClose = false

    // ── Create xterm instance ────────────────────────────────────────────
    const term = new Terminal({
      fontFamily: '"Cascadia Code", "Fira Code", "JetBrains Mono", monospace',
      fontSize: 13,
      lineHeight: 1.4,
      theme: {
        background: '#0f1117',
        foreground: '#e2e8f0',
        cursor: '#63b3ed',
        selectionBackground: '#2b6cb055',
        black:         '#1a202c', red:     '#fc8181',
        green:         '#68d391', yellow:  '#fbd38d',
        blue:          '#63b3ed', magenta: '#b794f4',
        cyan:          '#76e4f7', white:   '#e2e8f0',
        brightBlack:   '#4a5568', brightRed:     '#feb2b2',
        brightGreen:   '#9ae6b4', brightYellow:  '#fefcbf',
        brightBlue:    '#bee3f8', brightMagenta: '#e9d8fd',
        brightCyan:    '#c6f6d5', brightWhite:   '#ffffff',
      },
      cursorBlink: true,
      scrollback: 5000,
      convertEol: true,
    })

    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(containerRef.current)
    fit.fit()

    termRef.current = term
    fitRef.current = fit

    // ── Connect WebSocket ────────────────────────────────────────────────
    const ws = new WebSocket(`${WS_BASE}/ws/pty/${sessionId}`)
    ws.binaryType = 'arraybuffer'

    ws.onopen = () => {
      setStatus('connected')
      // Send initial size
      const { cols, rows } = term
      ws.send(`resize:${cols},${rows}`)
    }

    ws.onmessage = (e) => {
      if (e.data instanceof ArrayBuffer) {
        term.write(new Uint8Array(e.data))
      }
    }

    ws.onclose = () => {
      if (intentionalClose) return
      setStatus('closed')
      term.write('\r\n\x1b[90m[session ended]\x1b[0m\r\n')
      onExit?.()
    }

    ws.onerror = () => {
      if (intentionalClose) return
      term.write('\r\n\x1b[31m[connection error]\x1b[0m\r\n')
    }

    // ── Keyboard input → WS ─────────────────────────────────────────────
    term.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) {
        const encoded = new TextEncoder().encode(data)
        ws.send(encoded)
      }
    })

    // ── Resize observer → PTY resize ────────────────────────────────────
    const observer = new ResizeObserver(() => {
      fit.fit()
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(`resize:${term.cols},${term.rows}`)
      }
    })
    if (containerRef.current) observer.observe(containerRef.current)

    return () => {
      intentionalClose = true
      observer.disconnect()
      // Avoid "WebSocket closed before connection established" warning:
      // if still connecting, defer close until open fires, then close immediately.
      if (ws.readyState === WebSocket.CONNECTING) {
        ws.onopen = () => ws.close()
        ws.onmessage = null
      } else {
        ws.close()
      }
      term.dispose()
    }
  }, [sessionId])

  return (
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', background: '#0f1117', overflow: 'hidden' }}>
      {status !== 'connected' && (
        <div style={{
          padding: '6px 14px', fontSize: 11, flexShrink: 0,
          color: status === 'closed' ? '#fc8181' : '#63b3ed',
          borderBottom: '1px solid #1e2533',
        }}>
          {status === 'connecting' ? '⏳ 正在连接终端…' : '● 会话已结束'}
        </div>
      )}
      <div
        ref={containerRef}
        style={{ flex: 1, padding: '6px 4px', overflow: 'hidden' }}
      />
    </div>
  )
}
