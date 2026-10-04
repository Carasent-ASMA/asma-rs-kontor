/// <reference types="vitest/config" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { configDefaults } from 'vitest/config'

export default defineConfig({
  plugins: [react()],
  server: {
    port: 1420,
    strictPort: true,
    proxy: {
      // The renderer lives in Kontor; the current daily pipeline continues to own live report data.
      '^/tokenomics/data\\.(json|js)$': {
        target: process.env.TOKENOMICS_DATA_ORIGIN ?? 'http://127.0.0.1:8791',
        rewrite: (path) => path.replace(/^\/tokenomics/, ''),
      },
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    exclude: [...configDefaults.exclude, 'e2e/**'],
  },
})
