import assert from 'node:assert/strict';
import {setTimeout as delay} from 'node:timers/promises';

export function dcsDesktopWakes(request, until) {
  const status = async () => (await request({command: 'profile_desktop_status'})).desktop;
  let preferences, started = false;
  return {
    async prepare(document) {
      preferences = await status();
      await request({command: 'profile_desktop_configure', enabled: true, scope: preferences.scope});
      const contract = {delivery: 'state', schema: {type: 'boolean'}, freshnessMs: 1000, fallback: false};
      const port = (id, direction) => ({id, name: id, direction, contract});
      document.processingNodes.push({
        id: 'desktop-neutral', name: 'Neutral', operation: {kind: 'constant', value: false},
        inputs: [], outputs: [port('out', 'output')],
      });
      (document.desktopOutputs ??= []).push({
        id: 'keyboard', name: 'Keyboard', required: true, scope: {kind: 'desktop-wide'},
        ports: [port('hold', 'input')], actions: [{portId: 'hold', action: {kind: 'key-hold', key: 4}}],
      });
      document.bindings.push({
        id: 'desktop-neutral', from: {nodeId: 'desktop-neutral', portId: 'out'},
        to: {nodeId: 'keyboard', portId: 'hold'},
      });
    },
    async verify() {
      const initial = await until(async () => {
        const {runtime} = await status();
        return runtime?.scopeNotifications?.nativeHealthy && runtime.helperScope?.live && runtime;
      }, 'DCS profile did not establish the native desktop scope');
      started = true;
      assert.equal(initial.serviceIntervalMs, null);
      assert.deepEqual(initial.backend.held, []);
      await delay(1100);
      const quiet = (await status()).runtime;
      assert.equal(quiet.serviceIntervalMs, null);
      assert.equal(quiet.helperScope.live, true);
      assert.deepEqual(quiet.helperScope.admission, initial.helperScope.admission);
      assert.equal(quiet.backend.uncertain, false);
      assert.deepEqual(quiet.backend.held, []);
      return {initial, quiet, noGatewayReadsMs: 1100, authoredNonNeutralDesktopInput: false};
    },
    async close() {
      if (!preferences) return;
      const {runtime} = await status();
      await request({command: 'profile_desktop_configure', enabled: preferences.enabled, scope: preferences.scope});
      if (!started && !runtime) return;
      assert.ok(runtime, 'Desktop cleanup evidence must remain inspectable');
      assert.equal(runtime.helperScope, null);
      assert.equal(runtime.backend.cleanupComplete, true);
      assert.deepEqual(runtime.backend.held, []);
    },
  };
}
