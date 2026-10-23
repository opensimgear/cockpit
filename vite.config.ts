import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { browserCompanionDev } from './scripts/browser-companion-dev.mjs';
import { nativeGatewayBridge } from './scripts/native-gateway-bridge.mjs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig(() => ({
  plugins: [svelte(), browserCompanionDev(), ...(process.env.TAURI_ENV_PLATFORM ? [] : [nativeGatewayBridge()])],
  clearScreen: false,
  optimizeDeps: { exclude: ['@jsquash/png', '@jsquash/jpeg', '@jsquash/webp'] },
  server: { port: process.env.TAURI_ENV_PLATFORM ? 1420 : 5173, strictPort: true, watch: { ignored: ['**/src-tauri/**', '**/apps/browser-companion/dist/**', '**/.research/**', '**/.tmp/**', '**/.test-tmp/**', '**/target/**'] } },
  build: {
    rollupOptions: {
      input: { main: resolve(root, 'index.html') },
    },
  },
}));
