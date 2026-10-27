import { resolve } from 'node:path'
import { defineConfig } from 'vite'

// Каждое окно программы это отдельная страница
export default defineConfig({
  root: 'src',
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true
  },
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    target: 'es2022',
    rollupOptions: {
      input: {
        overlay: resolve(__dirname, 'src/overlay.html'),
        pin: resolve(__dirname, 'src/pin.html'),
        settings: resolve(__dirname, 'src/settings.html'),
        rec: resolve(__dirname, 'src/rec.html'),
        frame: resolve(__dirname, 'src/frame.html')
      }
    }
  }
})
