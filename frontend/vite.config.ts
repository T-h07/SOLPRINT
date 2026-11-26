/// <reference types="vitest/config" />
import { fileURLToPath, URL } from 'node:url';

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// The browser always talks to its own origin. In development Vite proxies
// /api and /health to the native backend; in containers nginx does the same.
const backend = process.env.SOLPRINT_BACKEND_URL ?? 'http://127.0.0.1:8080';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: backend, changeOrigin: false },
      '/health': { target: backend, changeOrigin: false },
    },
  },
  preview: {
    host: '127.0.0.1',
    port: 4173,
    strictPort: true,
    proxy: {
      '/api': { target: backend, changeOrigin: false },
      '/health': { target: backend, changeOrigin: false },
    },
  },
  build: {
    target: 'es2023',
    sourcemap: false,
    reportCompressedSize: true,
  },
  test: {
    environment: 'jsdom',
    include: ['tests/unit/**/*.test.{ts,tsx}'],
    setupFiles: ['tests/unit/setup.ts'],
    css: false,
    restoreMocks: true,
  },
});
