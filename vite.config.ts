import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';

// Tauri + React + Vite。レンダラーのルートは src/renderer。
export default defineConfig({
  plugins: [react()],
  root: './src/renderer',
  base: './',
  build: {
    outDir: '../../dist',
    emptyOutDir: true,
    target: process.env.TAURI_PLATFORM === 'windows' ? 'chrome105' : 'safari15',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
  resolve: {
    dedupe: ['react', 'react-dom'],
    alias: {
      '@': path.resolve(__dirname, './src'),
      '@renderer': path.resolve(__dirname, './src/renderer'),
      '@components': path.resolve(__dirname, './src/renderer/components'),
      '@stores': path.resolve(__dirname, './src/renderer/stores'),
      '@services': path.resolve(__dirname, './src/renderer/services'),
      '@config': path.resolve(__dirname, './src/renderer/config'),
      '@utils': path.resolve(__dirname, './src/renderer/utils'),
      '@locales': path.resolve(__dirname, './src/renderer/locales'),
      '@bindings': path.resolve(__dirname, './src/bindings'),
    },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  css: {
    postcss: path.resolve(__dirname, './postcss.config.js'),
  },
});
