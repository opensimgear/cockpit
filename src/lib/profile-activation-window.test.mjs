import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModules } from './test-module-loader.mjs';

const { NativeProfileActivationWindow } = (await loadModules(['./profile-activation-window'])).get('./profile-activation-window');
const progress = operationId => ({ operationId, profileName: 'Flight rig', title: 'Activating profile', phase: 'preparing', message: 'Preparing companions', details: [], canCancel: true });
const plain = value => JSON.parse(JSON.stringify(value));
const flush = async () => { for (let index = 0; index < 12; index++) await Promise.resolve(); };
const deferred = () => { let resolve; const promise = new Promise(yes => { resolve = yes; }); return { promise, resolve }; };

function harness({ native = true, invoke: custom, observeError } = {}) {
  const calls = [], listeners = new Set();
  const adapter = new NativeProfileActivationWindow({
    isNative: () => native,
    invoke: async (command, args) => {
      calls.push(plain({ command, ...(args ?? {}) }));
      return custom ? custom(command, args) : true;
    },
    observe(listener) {
      if (observeError) throw observeError;
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  });
  return {
    adapter, calls, listeners,
    async publish(snapshot, error) { for (const listener of listeners) listener(snapshot, error); await flush(); },
  };
}

test('browser mode and native creation errors allow the parent to use its local progress dialog', async () => {
  const browser = harness({ native: false });
  assert.equal(await browser.adapter.show(progress('operation'), () => {}), false);
  assert.deepEqual(browser.calls, []);
  const failed = harness({ invoke: async () => { throw new Error('Window unavailable'); } });
  assert.equal(await failed.adapter.show(progress('operation'), () => {}), false);
  assert.equal(failed.listeners.size, 0);
});

test('observation setup failure closes the just-created exact native window before using fallback', async () => {
  const h = harness({ observeError: new TypeError('Observation unavailable') });
  assert.equal(await h.adapter.show(progress('operation'), () => {}), false);
  assert.deepEqual(h.calls.map(call => call.command), ['profile_activation_window_open', 'profile_activation_window_close']);
  assert.equal(h.calls[1].operationId, 'operation');
  assert.equal(await h.adapter.update(progress('operation')), false);
});

test('observation loss closes the exact popup on the next update so the caller can fall back', async () => {
  const h = harness();
  assert.equal(await h.adapter.show(progress('operation'), () => {}), true);
  await h.publish(null, 'Observation lost');
  assert.equal(await h.adapter.update(progress('operation')), false);
  assert.equal(h.listeners.size, 0);
  assert.deepEqual(h.calls.map(call => call.command), ['profile_activation_window_open', 'profile_activation_window_close']);
  assert.equal(h.calls.at(-1).operationId, 'operation');
});

test('a transient progress update exception closes its exact native window before fallback', async () => {
  const h = harness({ invoke: async command => {
    if (command === 'profile_activation_window_update') throw new Error('IPC temporarily unavailable');
    return true;
  } });
  assert.equal(await h.adapter.show(progress('operation'), () => {}), true);
  assert.equal(h.listeners.size, 1);
  assert.equal(await h.adapter.update({ ...progress('operation'), message: 'Downloading images' }), false);
  assert.equal(h.listeners.size, 0);
  assert.deepEqual(h.calls.map(call => call.command), ['profile_activation_window_open', 'profile_activation_window_update', 'profile_activation_window_close']);
  assert.equal(h.calls.at(-1).operationId, 'operation');
  const count = h.calls.length;
  assert.equal(await h.adapter.update(progress('operation')), false);
  assert.equal(h.calls.length, count);
});

test('a disappeared child releases its observer and tolerates native cleanup reporting already closed', async () => {
  const h = harness({ invoke: async command => command === 'profile_activation_window_open' });
  assert.equal(await h.adapter.show(progress('operation'), () => {}), true);
  assert.equal(await h.adapter.update(progress('operation')), false);
  assert.equal(h.listeners.size, 0);
  assert.equal(h.calls.at(-1).command, 'profile_activation_window_close');
  assert.equal(h.calls.at(-1).operationId, 'operation');
});

test('an old failed update cannot close a newer activation or remove its observer', async () => {
  const pending = deferred();
  const h = harness({ invoke: async command => command === 'profile_activation_window_update' ? pending.promise : true });
  await h.adapter.show(progress('old-operation'), () => {});
  const updating = h.adapter.update(progress('old-operation'));
  const opening = h.adapter.show(progress('new-operation'), () => {});
  pending.resolve(false);
  assert.equal(await updating, false);
  assert.equal(await opening, true);
  assert.equal(h.listeners.size, 1);
  assert.equal(h.calls.some(call => call.command === 'profile_activation_window_close'), false);
  h.adapter.dispose(); await flush();
  assert.equal(h.calls.at(-1).operationId, 'new-operation');
});

test('cancel intent is delivered once and only for the exact currently displayed operation', async () => {
  const h = harness(); let cancelled = 0;
  assert.equal(await h.adapter.show(progress('operation'), () => { cancelled++; }), true);
  await h.publish({ progress: progress('other-operation'), cancelRequested: true }); assert.equal(cancelled, 0);
  await h.publish({ progress: progress('operation'), cancelRequested: true }); await h.publish({ progress: progress('operation'), cancelRequested: true }); assert.equal(cancelled, 1);
  await h.adapter.close('operation'); assert.equal(h.listeners.size, 0);
});

test('updates and stale close requests cannot dismiss a newer activation or its cancel observer', async () => {
  const h = harness(); let cancelled = 0;
  await h.adapter.show(progress('old-operation'), () => { throw new Error('Stale cancellation'); });
  await h.adapter.show(progress('new-operation'), () => { cancelled++; });
  const count = h.calls.length;
  assert.equal(await h.adapter.update(progress('old-operation')), false);
  assert.equal(h.calls.length, count);
  await h.adapter.close('old-operation'); assert.equal(h.listeners.size, 1);
  assert.equal(await h.adapter.update({ ...progress('new-operation'), message: 'Rendering panel' }), true);
  await h.publish({ progress: progress('new-operation'), cancelRequested: true }); assert.equal(cancelled, 1);
  h.adapter.dispose(); await flush(); assert.equal(h.listeners.size, 0);
  assert.equal(h.calls.at(-1).command, 'profile_activation_window_close');
  assert.equal(h.calls.at(-1).operationId, 'new-operation');
});

test('a cancellation notification from a released observer cannot reach a replacement', async () => {
  let cancelled = 0;
  const h = harness();
  await h.adapter.show(progress('old-operation'), () => { cancelled++; });
  const [oldListener] = h.listeners;
  await h.adapter.show(progress('new-operation'), () => { cancelled++; });
  oldListener({ progress: progress('old-operation'), cancelRequested: true });
  oldListener({ progress: progress('new-operation'), cancelRequested: true });
  await flush(); assert.equal(cancelled, 0);
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
  const h = harness();
  let cancelled = 0;
  const snapshot = { progress: progress('operation'), cancelRequested: true };
  await h.adapter.show(progress('operation'), () => { cancelled++; return cancelled > 1; });
  await h.publish(snapshot); assert.equal(cancelled, 1);
  assert.equal(h.calls.at(-1).resetCancelRequest, true);
  await h.publish({ ...snapshot, cancelRequested: false }); assert.equal(cancelled, 1);
  await h.publish(snapshot); assert.equal(cancelled, 2);
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
  assert.equal(h.listeners.size, 0);
  assert.equal(h.calls.at(-1).operationId, 'automatic-operation');
});

test('cleanup and switching phases pass through to the compact native window and reject queued cancellation', async () => {
  for (const phase of ['deactivating','recovering','switching']) {
    const h = harness(); let cancelled = 0;
    const cleanup = { ...progress('cleanup'), phase, canCancel: true, message: 'Unhiding peripherals' };
    await h.adapter.show(cleanup, () => { cancelled++; });
    await h.publish({ progress: cleanup, cancelRequested: true }); assert.equal(cancelled, 0);
    assert.deepEqual(h.calls[0].progress, cleanup);
    assert.equal(await h.adapter.update({ ...cleanup, message: 'Releasing keyboard and mouse bindings' }), true);
    assert.equal(h.calls.at(-1).progress.phase, phase);
    await h.adapter.close('cleanup'); assert.equal(h.listeners.size, 0);
  }
});

test('a stale activation notification cannot abort cleanup of the same operation', async () => {
  let cancelled = 0;
  const h = harness();
  await h.adapter.show(progress('operation'), () => { cancelled++; });
  await h.adapter.update({ ...progress('operation'), phase: 'recovering', message: 'Releasing bindings', canCancel: false });
  await h.publish({ progress: progress('operation'), cancelRequested: true });
  assert.equal(cancelled, 0);
  h.adapter.dispose(); await flush();
});
