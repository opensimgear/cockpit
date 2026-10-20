import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModules } from './test-module-loader.mjs';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';

const { NativeProfileActivationWindow } = (await loadModules(['./profile-activation-window'])).get('./profile-activation-window');
const progress = operationId => ({ operationId, profileName: 'Flight rig', title: 'Activating profile', phase: 'preparing', message: 'Preparing companions', details: [], canCancel: true });
const plain = value => JSON.parse(JSON.stringify(value));
const flush = async () => { for (let index = 0; index < 12; index++) await Promise.resolve(); };
const deferred = () => { let resolve; const promise = new Promise(yes => { resolve = yes; }); return { promise, resolve }; };

function harness({ native = true, invoke: custom } = {}) {
  const calls = [], timers = new Map();
  let snapshot = null, next = 0;
  const adapter = new NativeProfileActivationWindow({
    isNative: () => native,
    invoke: async (command, args) => {
      calls.push(plain({ command, ...(args ?? {}) }));
      if (custom) return custom(command, args);
      if (command === 'profile_activation_window_state') return snapshot;
      return true;
    },
    setInterval(callback) { const id = ++next; timers.set(id, callback); return id; },
    clearInterval(id) { timers.delete(id); },
  });
  return { adapter, calls, timers, setSnapshot(value) { snapshot = value; }, async poll() { for (const callback of timers.values()) callback(); await flush(); } };
}

test('browser mode and native creation errors allow the parent to use its local progress dialog', async () => {
  const browser = harness({ native: false });
  assert.equal(await browser.adapter.show(progress('operation'), () => {}), false);
  assert.deepEqual(browser.calls, []);
  const failed = harness({ invoke: async () => { throw new Error('Window unavailable'); } });
  assert.equal(await failed.adapter.show(progress('operation'), () => {}), false);
  assert.equal(failed.timers.size, 0);
});

test('default browser timers keep their Window receiver while forwarding cancellation and closing', async () => {
  const filename = new URL('./profile-activation-window.ts', import.meta.url);
  const code = ts.transpileModule(await readFile(filename, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const timers = new Map(), exports = {}; let next = 0, browserGlobal;
  const context = vm.createContext({ exports,
    setInterval(callback) {
      assert.equal(this, browserGlobal, 'WebView2 rejects timer calls with an adapter receiver');
      const id = ++next; timers.set(id, callback); return id;
    },
    clearInterval(id) {
      assert.equal(this, browserGlobal, 'WebView2 rejects timer cleanup with an adapter receiver');
      timers.delete(id);
    },
  });
  browserGlobal = vm.runInContext('globalThis', context);
  vm.runInContext(code, context, { filename: filename.pathname });
  let cancelled = 0;
  const adapter = new exports.NativeProfileActivationWindow({ isNative: () => true,
    invoke: async command => command === 'profile_activation_window_state' ? { progress: progress('operation'), cancelRequested: true } : true,
  });
  assert.equal(await adapter.show(progress('operation'), () => { cancelled++; }), true);
  assert.equal(timers.size, 1);
  for (const callback of timers.values()) callback();
  await flush(); assert.equal(cancelled, 1);
  await adapter.close('operation'); assert.equal(timers.size, 0);
});

test('polling setup failure closes the just-created exact native window before using fallback', async () => {
  const calls = [];
  const adapter = new NativeProfileActivationWindow({ isNative: () => true,
    invoke: async (command, args) => { calls.push(plain({ command, ...args })); return true; },
    setInterval() { throw new TypeError('Timer unavailable'); }, clearInterval() {},
  });
  assert.equal(await adapter.show(progress('operation'), () => {}), false);
  assert.deepEqual(calls.map(call => call.command), ['profile_activation_window_open', 'profile_activation_window_close']);
  assert.equal(calls[1].operationId, 'operation');
  assert.equal(await adapter.update(progress('operation')), false);
});

test('a transient progress update exception closes its exact native window before fallback', async () => {
  const h = harness({ invoke: async command => {
    if (command === 'profile_activation_window_update') throw new Error('IPC temporarily unavailable');
    return true;
  } });
  assert.equal(await h.adapter.show(progress('operation'), () => {}), true);
  assert.equal(h.timers.size, 1);
  assert.equal(await h.adapter.update({ ...progress('operation'), message: 'Downloading images' }), false);
  assert.equal(h.timers.size, 0);
  assert.deepEqual(h.calls.map(call => call.command), ['profile_activation_window_open', 'profile_activation_window_update', 'profile_activation_window_close']);
  assert.equal(h.calls.at(-1).operationId, 'operation');
  const count = h.calls.length;
  assert.equal(await h.adapter.update(progress('operation')), false);
  assert.equal(h.calls.length, count);
});

test('a disappeared child clears polling and tolerates native cleanup reporting already closed', async () => {
  const h = harness({ invoke: async command => command === 'profile_activation_window_open' });
  assert.equal(await h.adapter.show(progress('operation'), () => {}), true);
  assert.equal(await h.adapter.update(progress('operation')), false);
  assert.equal(h.timers.size, 0);
  assert.equal(h.calls.at(-1).command, 'profile_activation_window_close');
  assert.equal(h.calls.at(-1).operationId, 'operation');
});

test('an old failed update cannot close a newer activation or remove its polling', async () => {
  const pending = deferred();
  const h = harness({ invoke: async command => command === 'profile_activation_window_update' ? pending.promise : true });
  await h.adapter.show(progress('old-operation'), () => {});
  const updating = h.adapter.update(progress('old-operation'));
  const opening = h.adapter.show(progress('new-operation'), () => {});
  pending.resolve(false);
  assert.equal(await updating, false);
  assert.equal(await opening, true);
  assert.equal(h.timers.size, 1);
  assert.equal(h.calls.some(call => call.command === 'profile_activation_window_close'), false);
  h.adapter.dispose(); await flush();
  assert.equal(h.calls.at(-1).operationId, 'new-operation');
});

test('cancel intent is delivered once and only for the exact currently displayed operation', async () => {
  const h = harness(); let cancelled = 0;
  assert.equal(await h.adapter.show(progress('operation'), () => { cancelled++; }), true);
  h.setSnapshot({ progress: progress('other-operation'), cancelRequested: true });
  await h.poll(); assert.equal(cancelled, 0);
  h.setSnapshot({ progress: progress('operation'), cancelRequested: true });
  await h.poll(); await h.poll(); assert.equal(cancelled, 1);
  await h.adapter.close('operation'); assert.equal(h.timers.size, 0);
});

test('updates and stale close requests cannot dismiss a newer activation or its cancel polling', async () => {
  const h = harness(); let cancelled = 0;
  await h.adapter.show(progress('old-operation'), () => { throw new Error('Stale cancellation'); });
  await h.adapter.show(progress('new-operation'), () => { cancelled++; });
  const count = h.calls.length;
  assert.equal(await h.adapter.update(progress('old-operation')), false);
  assert.equal(h.calls.length, count);
  await h.adapter.close('old-operation'); assert.equal(h.timers.size, 1);
  assert.equal(await h.adapter.update({ ...progress('new-operation'), message: 'Rendering panel' }), true);
  h.setSnapshot({ progress: progress('new-operation'), cancelRequested: true });
  await h.poll(); assert.equal(cancelled, 1);
  h.adapter.dispose(); await flush(); assert.equal(h.timers.size, 0);
  assert.equal(h.calls.at(-1).command, 'profile_activation_window_close');
  assert.equal(h.calls.at(-1).operationId, 'new-operation');
});

test('a cancellation reply arriving after a newer operation is displayed is ignored', async () => {
  const pending = deferred(); let cancelled = 0;
  const h = harness({ invoke: async command => command === 'profile_activation_window_state' ? pending.promise : true });
  await h.adapter.show(progress('old-operation'), () => { cancelled++; });
  const polling = h.poll();
  await h.adapter.show(progress('new-operation'), () => { cancelled++; });
  pending.resolve({ progress: progress('old-operation'), cancelRequested: true });
  await polling; await flush(); assert.equal(cancelled, 0);
  h.adapter.dispose(); await flush();
});

test('show, update and close serialize native mutations across a slow creation', async () => {
  const pending = deferred();
  const h = harness({ invoke: async (command, args) => command === 'profile_activation_window_open' && args.progress.operationId === 'old-operation' ? pending.promise : true });
  const first = h.adapter.show(progress('old-operation'), () => {});
  const closing = h.adapter.close('old-operation');
  const second = h.adapter.show(progress('new-operation'), () => {});
  await flush(); assert.equal(h.calls.length, 1);
  pending.resolve(true);
  assert.equal(await first, false); await closing; assert.equal(await second, true);
  assert.deepEqual(h.calls.map(call => call.command), ['profile_activation_window_open', 'profile_activation_window_close', 'profile_activation_window_open']);
  h.adapter.dispose(); await flush();
});

test('a failed cancel request releases its intent so the user can retry the same activation', async () => {
  let snapshot = { progress: progress('operation'), cancelRequested: true }, cancelled = 0;
  const h = harness({ invoke: async (command, args) => {
    if (command === 'profile_activation_window_state') return snapshot;
    if (command === 'profile_activation_window_update' && args.resetCancelRequest) snapshot = { progress: args.progress, cancelRequested: false };
    return true;
  } });
  await h.adapter.show(progress('operation'), () => { cancelled++; return cancelled > 1; });
  await h.poll(); assert.equal(cancelled, 1);
  assert.equal(snapshot.cancelRequested, false);
  await h.poll(); assert.equal(cancelled, 1);
  snapshot = { progress: progress('operation'), cancelRequested: true };
  await h.poll(); assert.equal(cancelled, 2);
  h.adapter.dispose(); await flush();
});

test('automatic activation continues to game launch in its existing native window', async () => {
  const h = harness();
  await h.adapter.show(progress('automatic-operation'), () => {});
  const launching = { ...progress('automatic-operation'), phase: 'launching', message: 'Launching Assetto Corsa Rally', canCancel: false };
  assert.equal(await h.adapter.update(launching), true);
  assert.deepEqual(h.calls.map(call => call.command), ['profile_activation_window_open', 'profile_activation_window_update']);
  assert.deepEqual(h.calls.at(-1).progress, launching);
  await h.adapter.close('automatic-operation');
  assert.equal(h.timers.size, 0);
  assert.equal(h.calls.at(-1).operationId, 'automatic-operation');
});
