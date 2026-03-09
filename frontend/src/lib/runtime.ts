const DEFAULT_API_BASE = 'http://127.0.0.1:8080'

interface CooperationRuntimeConfig {
  apiBase?: string
  wsBase?: string
}

declare global {
  interface Window {
    __COOPERATION_RUNTIME__?: CooperationRuntimeConfig
  }
}

function trimSlash(value: string): string {
  return value.replace(/\/+$/, '')
}

function toWebSocketBase(apiBase: string): string {
  return apiBase.replace(/^http/i, 'ws')
}

const runtimeConfig =
  typeof window !== 'undefined' ? window.__COOPERATION_RUNTIME__ : undefined

const apiBase = runtimeConfig?.apiBase?.trim() || import.meta.env.VITE_API_BASE?.trim()
const wsBase = runtimeConfig?.wsBase?.trim() || import.meta.env.VITE_WS_BASE?.trim()

export const API_BASE = trimSlash(apiBase || DEFAULT_API_BASE)
export const WS_BASE = trimSlash(wsBase || toWebSocketBase(API_BASE))

export {}
