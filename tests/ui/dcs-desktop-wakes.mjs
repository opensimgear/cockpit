import assert from 'node:assert/strict';
import {setTimeout as delay} from 'node:timers/promises';

export function dcsDesktopWakes(request, until, scopeKind) {
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
        id: 'keyboard', name: 'Keyboard', required: true, scope: {kind: scopeKind},
        ports: [port('hold', 'input')], actions: [{portId: 'hold', action: {kind: 'key-hold', key: 4}}],
      });
      document.bindings.push({
        id: 'desktop-neutral', from: {nodeId: 'desktop-neutral', portId: 'out'},
        to: {nodeId: 'keyboard', portId: 'hold'},
      });
    },
    async verify(pid) {
      const initial = await until(async () => {
        const {runtime} = await status();
        return runtime?.scopeNotifications?.nativeHealthy && runtime.helperScope?.live && runtime;
      }, 'DCS profile did not establish the native desktop scope');
      started = true;
      assert.equal(initial.serviceIntervalMs, null);
      assert.deepEqual(initial.backend.held, []);
      if (scopeKind === 'selected-game') {
        assert.equal(initial.scope.kind, 'application');
        assert.equal(initial.scope.identity.pid, pid);
        assert.match(initial.scope.identity.executable, /DCS\.exe$/i);
      }
      await delay(1100);
      const quiet = (await status()).runtime;
      assert.equal(quiet.serviceIntervalMs, null);
      assert.equal(quiet.helperScope.live, true);
      assert.deepEqual(quiet.helperScope.admission, initial.helperScope.admission);
      assert.deepEqual(quiet.scope, initial.scope);
      assert.equal(quiet.backend.uncertain, false);
      assert.deepEqual(quiet.backend.held, []);
      return {initial, quiet, noGatewayReadsMs: 1100, authoredNonNeutralDesktopInput: false};
    },
    async verifyExit() {
      if (scopeKind !== 'selected-game') return;
      return await until(async () => {
        const {runtime} = await status();
        return runtime?.state === 'waiting' && runtime.helperScope === null
          && runtime.backend.cleanupComplete && runtime;
      }, 'Exited process kept its selected-game desktop scope');
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
