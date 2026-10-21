import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';

// 前端与后端联调：开发模式下把 /api 与 /healthz 代理到 Rust 服务，
// 这样浏览器始终同源访问，不需要额外的跨域配置。
const BACKEND = process.env.SALARY_BACKEND ?? 'http://localhost:5001';

export default defineConfig({
  plugins: [react()],

  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },

  server: {
    port: 3000,
    strictPort: true,
    proxy: {
      '/api': { target: BACKEND, changeOrigin: true },
      '/healthz': { target: BACKEND, changeOrigin: true },
    },
  },

  build: {
    target: 'esnext',
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      output: {
        manualChunks: {
          vendor: ['react', 'react-dom', 'react-router-dom'],
        },
      },
    },
  },

  preview: {
    port: 4173,
    strictPort: true,
  },
});
