import assert from 'node:assert/strict';
import test from 'node:test';
import { nativePage, until } from '../../scripts/reactive/cdp.mjs';

test('running Cockpit admits only known passive CAN input topics', { timeout: 60000 }, async () => {
  const page = await nativePage();
  try {
    await until(() => page.evaluate(`document.title === 'OpenSimGear Cockpit' && !!window.__TAURI_INTERNALS__`), Boolean, 40000);
    await page.evaluate(`(async () => {
      const { SharedObservations } = await import('/src/lib/streams/shared.ts');
      const { canInputTopic } = await import('/src/lib/can-input-observations.ts');
      const observations = new SharedObservations();
      const topic = canInputTopic('stm32:00112233445566778899aabb', '00112233445566778899aabbccddeeff');
      const owned = window.__canInputSmoke = { observations, rejected: {} };
      for (const [id, key] of [['malformed', 'can.inputs.invalid'], ['unconnected', topic]]) {
        observations.subscribe(key, state => { if (state.status === 'error') owned.rejected[id] = state; });
      }
    })()`);
    const rejected = await until(() => page.evaluate(`window.__canInputSmoke.rejected`),
      value => value.malformed && value.unconnected);
    assert.match(rejected.malformed.error, /Invalid CAN input observation topic/);
    assert.match(rejected.unconnected.error, /Connect the bound gateway first/);
    assert.equal(rejected.malformed.frame, undefined);
    assert.equal(rejected.unconnected.frame, undefined);
  } finally {
    await page.evaluate(`window.__canInputSmoke?.observations.close(); delete window.__canInputSmoke;`).catch(() => {});
    page.close();
  }
});
