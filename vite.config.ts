import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { nativeGatewayBridge } from './scripts/native-gateway-bridge.mjs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig(() => ({
  plugins: [svelte(), ...(process.env.TAURI_ENV_PLATFORM ? [] : [nativeGatewayBridge()])],
  clearScreen: false,
  server: { port: process.env.TAURI_ENV_PLATFORM ? 1420 : 5173, strictPort: true, watch: { ignored: ['**/src-tauri/**', '**/target/**'] } },
  build: {
    rollupOptions: {
      input: { main: resolve(root, 'index.html') },
    },
  },
}));
