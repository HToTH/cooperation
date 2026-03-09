import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

function normalizeId(id: string) {
  return id.replaceAll('\\', '/')
}

const tauriHost = process.env.TAURI_DEV_HOST

export default defineConfig({
  clearScreen: false,
  plugins: [react()],
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    rollupOptions: {
      output: {
        manualChunks(id) {
          const normalized = normalizeId(id)

          if (!normalized.includes('/node_modules/')) return
          if (normalized.includes('/@xterm/')) return 'terminal-vendor'
          if (normalized.includes('/@xyflow/') || normalized.includes('/d3-')) return 'flow-vendor'
          if (
            normalized.includes('/react/') ||
            normalized.includes('/react-dom/') ||
            normalized.includes('/scheduler/') ||
            normalized.includes('/zustand/') ||
            normalized.includes('/use-sync-external-store/')
          ) {
            return 'react-vendor'
          }
          return 'vendor'
        },
      },
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    host: tauriHost || false,
    hmr: tauriHost
      ? {
          protocol: 'ws',
          host: tauriHost,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
    proxy: {
      '/api': 'http://127.0.0.1:8080',
    },
  },
})
