import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Tauri expects a fixed dev server port and ignores its own Rust workspace
// during file watching. See https://tauri.app/develop/#vite
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/apps/desktop/**', '**/target/**'],
    },
  },
})
