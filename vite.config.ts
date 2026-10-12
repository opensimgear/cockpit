import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { picoGateway } from './simulator/pico-bridge.mjs';
import { nativeEmulatorBridge } from './scripts/native-emulator-bridge.mjs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig({
  plugins: [svelte(), ...(process.env.TAURI_ENV_PLATFORM ? [] : [process.env.OSG_NATIVE_EMULATOR ? nativeEmulatorBridge() : picoGateway()])],
  clearScreen: false,
  server: { port: process.env.TAURI_ENV_PLATFORM ? 1420 : 5173, strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
  build: {
    rollupOptions: {
      input: { main: resolve(root, 'index.html'), pico: resolve(root, 'pico-flashing.html') },
    },
  },
});
