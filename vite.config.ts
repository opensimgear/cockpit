import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { gatewaySimulator } from './simulator/bridge.mjs';
import { picoGateway } from './simulator/pico-bridge.mjs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig({
  plugins: [svelte(), gatewaySimulator(), picoGateway()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: {
    rollupOptions: {
      input: { main: resolve(root, 'index.html'), pico: resolve(root, 'pico-flashing.html') },
    },
  },
});
