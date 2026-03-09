import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

function normalizeId(id: string) {
  return id.replaceAll('\\', '/')
}

export default defineConfig({
  plugins: [react()],
  build: {
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
    proxy: {
      '/api': 'http://localhost:8080',
    },
  },
})
