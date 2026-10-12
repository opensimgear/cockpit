import { spawnSync } from 'node:child_process';
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
if (process.platform === 'darwin') {
  const configured = process.env.OSG_HID_HELPER;
  if (configured && !configured.startsWith('/')) throw new Error('OSG_HID_HELPER must be an absolute path.');
  if (!configured) {
    const moduleCache = resolve(root, 'simulator/.build/module-cache');
    await mkdir(moduleCache, { recursive: true });
    const build = spawnSync('xcrun', ['swift', 'build', '-c', 'release', '--package-path', resolve(root, 'simulator/macos')], {
      stdio: 'inherit', timeout: 120_000,
      env: { ...process.env, CLANG_MODULE_CACHE_PATH: moduleCache, SWIFTPM_MODULECACHE_OVERRIDE: moduleCache },
    });
    if (build.error || build.status !== 0) throw build.error ?? new Error('CoreHID helper compilation failed.');
  }
  const output = resolve(root, 'src-tauri/resources/corehid');
  await mkdir(output, { recursive: true });
  await copyFile(configured ?? resolve(root, 'simulator/macos/.build/release/osg-gateway-simulator'), resolve(output, 'osg-gateway-simulator'));
}
