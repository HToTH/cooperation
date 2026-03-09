import { create } from 'zustand'
import { wsClient } from '../lib/wsClient'

interface WsState {
  connected: boolean
  connect: () => void
  disconnect: () => void
}

let statusPollTimer: ReturnType<typeof setInterval> | null = null
let disconnectTimer: ReturnType<typeof setTimeout> | null = null

export const useWsStore = create<WsState>((set) => ({
  connected: false,

  connect: () => {
    if (disconnectTimer) {
      clearTimeout(disconnectTimer)
      disconnectTimer = null
    }

    wsClient.connect()
    set({ connected: wsClient.isConnected })

    if (statusPollTimer) return

    statusPollTimer = setInterval(() => {
      set({ connected: wsClient.isConnected })
    }, 500)
  },

  disconnect: () => {
    if (disconnectTimer) {
      clearTimeout(disconnectTimer)
    }

    disconnectTimer = setTimeout(() => {
      if (statusPollTimer) {
        clearInterval(statusPollTimer)
        statusPollTimer = null
      }
      wsClient.disconnect()
      set({ connected: false })
      disconnectTimer = null
    }, 100)
  },
}))
