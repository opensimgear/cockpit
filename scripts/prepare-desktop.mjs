import { copyFile, mkdir, rm } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const firmwareOutput = resolve(root, 'src-tauri/resources/firmware');
await mkdir(firmwareOutput, { recursive: true });
try {
  await copyFile(resolve(root, 'public/firmware/osg-pico-gateway.uf2'), resolve(firmwareOutput, 'osg-pico-gateway.uf2'));
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
  await rm(resolve(firmwareOutput, 'osg-pico-gateway.uf2'), { force: true });
}
