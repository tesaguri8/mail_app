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
    // Rondine の枝番 N=12（main-proxy のポート台帳）→ フロント dev 枠 3100+N=3112。
    // Tauri devUrl（src-tauri/tauri.conf.json）と一致させる。他アプリ（Primadoc=1420 等）と衝突しない。
    port: 3112,
    strictPort: true,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  css: {
    postcss: path.resolve(__dirname, './postcss.config.js'),
  },
});
