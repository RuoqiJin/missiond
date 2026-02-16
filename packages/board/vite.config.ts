import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';
import { createProxyMiddleware } from './server/proxy';

const WS_PORT = process.env.MISSION_WS_PORT || '9120';

export default defineConfig({
  plugins: [
    react(),
    {
      name: 'missiond-api-proxy',
      configureServer(server) {
        server.middlewares.use(createProxyMiddleware());
      },
      configurePreviewServer(server) {
        server.middlewares.use(createProxyMiddleware());
      },
    },
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    proxy: {
      '/ws': {
        target: `ws://localhost:${WS_PORT}`,
        ws: true,
        rewrite: (path) => path.replace(/^\/ws/, ''),
      },
    },
  },
});
