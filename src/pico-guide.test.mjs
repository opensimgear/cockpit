import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';

const source = await readFile(new URL('./pico-guide.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;

function guide({ desktop = true, invoke = async () => '/Users/test/Downloads/osg-pico-gateway.uf2' } = {}) {
  const listeners = {};
  const attributes = new Map([['download', ''], ['href', '/firmware/osg-pico-gateway.uf2']]);
  const download = {
    removeAttribute: name => attributes.delete(name),
    setAttribute: (name, value) => attributes.set(name, value),
    addEventListener: (name, callback) => { listeners[name] = callback; },
  };
  const status = { hidden: true, dataset: {}, textContent: '' };
  const calls = [];
  let prevented = 0;
  vm.runInNewContext(compiled, {
    exports: {},
    document: { querySelector: selector => selector === '#firmware-download' ? download : status },
    require: name => {
      assert.equal(name, '@tauri-apps/api/core');
      return { isTauri: () => desktop, invoke: (...args) => { calls.push(args); return invoke(...args); } };
    },
  });
  return { attributes, listeners, status, calls, click: () => listeners.click({ preventDefault: () => { prevented++; } }), prevented: () => prevented };
}

test('native firmware download invokes the fixed export and displays the saved file path', async () => {
  const page = guide();
  await page.click();
  assert.equal(page.prevented(), 1);
  assert.deepEqual(page.calls, [['export_pico_firmware']]);
  assert.equal(page.status.hidden, false);
  assert.match(page.status.textContent, /Saved firmware to \/Users\/test\/Downloads\/osg-pico-gateway.uf2/);
  assert.equal(page.attributes.has('aria-busy'), false);
});

test('native export failure is visible and the download can be retried', async () => {
  const page = guide({ invoke: async () => { throw 'Build the firmware first.'; } });
  await page.click();
  assert.equal(page.status.dataset.error, 'true');
  assert.equal(page.status.textContent, 'Build the firmware first.');
  assert.equal(page.attributes.has('aria-disabled'), false);
  await page.click();
  assert.equal(page.calls.length, 2);
});

test('repeated native clicks cannot create simultaneous exports', async () => {
  let finish;
  const page = guide({ invoke: () => new Promise(resolve => { finish = resolve; }) });
  const first = page.click();
  assert.equal(page.status.textContent, 'Saving gateway firmware to Downloads…');
  assert.equal(page.attributes.get('aria-disabled'), 'true');
  await page.click();
  assert.equal(page.calls.length, 1);
  finish('/Users/test/Downloads/osg-pico-gateway-1.uf2');
  await first;
  assert.match(page.status.textContent, /osg-pico-gateway-1\.uf2/);
});

test('browser guide preserves the normal download anchor without native IPC', () => {
  const page = guide({ desktop: false });
  assert.equal(page.attributes.has('download'), true);
  assert.equal(page.attributes.get('href'), '/firmware/osg-pico-gateway.uf2');
  assert.equal(page.listeners.click, undefined);
  assert.deepEqual(page.calls, []);
});
