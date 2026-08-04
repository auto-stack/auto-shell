import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

// Tauri-friendly Vite config:
// - fixed dev port matches tauri.conf.json devUrl
// - don't auto-open browser when Tauri drives the dev server
// - @/ alias mirrors the Auto Vue generator output conventions
// - fixed asset filenames so Tauri's bundle can locate them
const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': resolve(__dirname, './src'),
    },
  },
  // Tauri requires a deterministic port + host (mobile / WSL).
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: 'ws', host, port: 1421 }
      : undefined,
    open: !process.env.TAURI_ENV,
  },
  build: {
    // Tauri supports es2021
    target: ['es2021', 'chrome100', 'safari13'],
    rollupOptions: {
      output: {
        entryFileNames: 'assets/[name].js',
        chunkFileNames: 'assets/[name].js',
        assetFileNames: 'assets/[name].[ext]',
      },
    },
  },
})
