import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    host: '127.0.0.1',
    port: 5173,
    proxy: {
      '/api': 'http://127.0.0.1:3030',
      '/auth': 'http://127.0.0.1:3030',
      '/callback': 'http://127.0.0.1:3030',
    },
  },
})
