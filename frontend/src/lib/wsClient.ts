import type { WsCommand, WsEvent } from './types'
import { WS_BASE } from './runtime'

type EventHandler = (event: WsEvent) => void

class WebSocketClient {
  private ws: WebSocket | null = null
  private handlers: Set<EventHandler> = new Set()
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private reconnectDelay = 1000
  private url: string
  private intentionalClose = false

  constructor(url: string) {
    this.url = url
  }

  connect() {
    if (
      this.ws?.readyState === WebSocket.OPEN ||
      this.ws?.readyState === WebSocket.CONNECTING
    ) {
      return
    }

    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }

    this.intentionalClose = false

    const ws = new WebSocket(this.url)
    this.ws = ws

    ws.onopen = () => {
      console.log('[WS] Connected to cooperation backend')
      this.reconnectDelay = 1000
      if (this.reconnectTimer) {
        clearTimeout(this.reconnectTimer)
        this.reconnectTimer = null
      }
    }

    ws.onmessage = (e) => {
      try {
        const event = JSON.parse(e.data as string) as WsEvent
        this.handlers.forEach((h) => h(event))
      } catch (err) {
        console.error('[WS] Failed to parse event:', err)
      }
    }

    ws.onclose = () => {
      if (this.ws === ws) {
        this.ws = null
      }
      if (this.intentionalClose) {
        this.intentionalClose = false
        return
      }
      console.warn('[WS] Disconnected, reconnecting in', this.reconnectDelay, 'ms')
      this.scheduleReconnect()
    }

    ws.onerror = (err) => {
      console.error('[WS] Error:', err)
    }
  }

  disconnect() {
    this.intentionalClose = true
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    this.ws?.close()
    this.ws = null
  }

  send(cmd: WsCommand) {
    if (this.ws?.readyState !== WebSocket.OPEN) {
      console.error('[WS] Cannot send — not connected')
      return
    }
    this.ws.send(JSON.stringify(cmd))
  }

  subscribe(handler: EventHandler) {
    this.handlers.add(handler)
    return () => this.handlers.delete(handler)
  }

  get isConnected() {
    return this.ws?.readyState === WebSocket.OPEN
  }

  private scheduleReconnect() {
    if (this.intentionalClose) return
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null
      this.connect()
      this.reconnectDelay = Math.min(this.reconnectDelay * 2, 30000)
    }, this.reconnectDelay)
  }
}

export const wsClient = new WebSocketClient(`${WS_BASE}/ws`)
