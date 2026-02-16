import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';
import { createProxyMiddleware } from './server/proxy';

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
});
