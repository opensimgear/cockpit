import assert from 'node:assert/strict';
import test from 'node:test';
import { browserPage, nativePage, until } from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_CAN_INPUTS_BROWSER_URL;
const transport = browserUrl ? 'browser-dev' : 'native';
const gatewayId = 'stm32:00112233445566778899aabb';
const moduleId = '00112233445566778899aabbccddeeff';
const boundTopic = `can.inputs.${gatewayId.slice(6)}.${moduleId}`;
const openPage = () => browserUrl ? browserPage(browserUrl) : nativePage();
const ready = page => until(() => page.evaluate(
  `document.title === 'OpenSimGear Cockpit' && !!document.querySelector('nav[aria-label="Main navigation"]') && (${Boolean(browserUrl)} || !!window.__TAURI_INTERNALS__)`,
), Boolean, 40000);

test(`${transport} Cockpit admits only known passive CAN input topics`, { timeout: 60000 }, async () => {
  const page = await openPage();
  try {
    await ready(page);
    await page.evaluate(`(async () => {
      const { SharedObservations } = await import('/src/lib/streams/shared.ts');
      const { canInputTopic } = await import('/src/lib/can-input-observations.ts');
      const errors = {};
      let transport;
      if (${Boolean(browserUrl)}) {
        const { createBrowserObservationTransport } = await import('/src/lib/streams/browser.ts');
        transport = createBrowserObservationTransport({ async fetch(path, init) {
          const reply = await fetch(path, init);
          if (path.endsWith('/subscribe') && !reply.ok) {
            const { topic } = JSON.parse(init.body);
            errors[topic] = (await reply.clone().json()).error;
          }
          return reply;
        } });
      }
      const observations = new SharedObservations(transport);
      const topic = canInputTopic('${gatewayId}', '${moduleId}');
      const owned = window.__canInputSmoke = { observations, rejected: {}, errors };
      for (const [id, key] of [['malformed', 'can.inputs.invalid'], ['unconnected', topic]]) {
        observations.subscribe(key, state => { if (state.status === 'error') owned.rejected[id] = state; });
      }
    })()`);
    const rejected = await until(() => page.evaluate(`window.__canInputSmoke.rejected`),
      value => value.malformed && value.unconnected);
    if (browserUrl) {
      const errors = await page.evaluate('window.__canInputSmoke.errors');
      assert.match(errors['can.inputs.invalid'], /Invalid CAN input observation topic/);
      assert.match(errors[boundTopic], /Connect the bound gateway first/);
      assert.match(rejected.malformed.error, /Observation subscribe failed \(400\)/);
      assert.match(rejected.unconnected.error, /Observation subscribe failed \(400\)/);
    } else {
      assert.match(rejected.malformed.error, /Invalid CAN input observation topic/);
      assert.match(rejected.unconnected.error, /Connect the bound gateway first/);
    }
    assert.equal(rejected.malformed.frame, undefined);
    assert.equal(rejected.unconnected.frame, undefined);
  } finally {
    await page.evaluate(`window.__canInputSmoke?.observations.close(); delete window.__canInputSmoke;`).catch(() => {});
    await page.close();
  }
});

test(`${transport} calibrated CAN preview reveals the current authored input`, { timeout: 60000 }, async () => {
  const page = await openPage();
  const visibility = hidden => page.evaluate(`window.__authoredCan.visibility(${hidden})`);
  const displayed = value => until(() => page.evaluate(`document.querySelector('#authored-can .axis > span')?.textContent.trim()`),
    text => text === `Calibrated: ${value}`);
  const counts = () => page.evaluate(`window.__authoredCan.counts`);
  try {
    await ready(page);
    await page.evaluate(`(async () => {
      const svelteUrl = performance.getEntriesByType('resource')
        .find(entry => new URL(entry.name).pathname.endsWith('/deps/svelte.js'))?.name;
      if (!svelteUrl) throw Error('Running dev Cockpit did not load the Svelte client.');
      const [{ mount, unmount }, { default: DebugControls }, { SharedObservations }, { canInputTopic }] = await Promise.all([
        import(svelteUrl), import('/src/lib/DebugControls.svelte'), import('/src/lib/streams/shared.ts'),
        import('/src/lib/can-input-observations.ts'),
      ]);
      const gatewayId = '${gatewayId}', id = '${moduleId}';
      const topic = canInputTopic(gatewayId, id);
      const route = { epoch: 1, node: 2, sourceNode: 3, boot: 4, lease: 5, configurationGeneration: 6 };
      const counts = { open: 0, retire: 0 };
      let axis = 120, publish;
      const subscribe = SharedObservations.prototype.subscribe;
      SharedObservations.prototype.subscribe = function (key, listener) {
        if (key !== topic) return subscribe.call(this, key, listener);
        counts.open++;
        publish = () => listener({ status: 'ready', frame: { topic, source: 'can-inputs', payload: {
          status: 'ready', route, held: [], axes: [axis, 0, 0, 0, 0, 0, 0, 0],
          axisAvailable: [true, false, false, false, false, false, false, false],
        } } });
        publish();
        return () => { counts.retire++; publish = undefined; };
      };
      const visibility = hidden => {
        Object.defineProperty(document, 'hidden', { configurable: true, value: hidden });
        document.dispatchEvent(new Event('visibilitychange'));
      };
      const definition = { name: 'Authored panel', buttons: [], axes: [[0, 48, 0, 4095, 0, 0, 0]],
        hats: [], encoders: [], selectors: [], expanderMask: 0 };
      const target = document.createElement('div');
      target.id = 'authored-can';
      document.body.append(target);
      let component;
      window.__authoredCan = {
        counts, visibility,
        setAxis(value) { axis = value; publish?.(); },
        async close() {
          const mounted = component;
          component = undefined;
          if (mounted) await unmount(mounted);
          target.remove();
          SharedObservations.prototype.subscribe = subscribe;
          delete document.hidden;
          document.dispatchEvent(new Event('visibilitychange'));
        },
      };
      visibility(false);
      component = mount(DebugControls, { target, props: {
        state: { gatewayId, epoch: route.epoch, connected: true, panelStreamSupported: true,
          debug: { devices: [{ id, boot: route.boot, online: true, axes: [], contacts: [], definition }] } },
        module: { id, ...route }, request: async () => true, read: async () => true,
      } });
    })()`);
    await displayed(120);
    await page.evaluate('window.__authoredCan.setAxis(240)');
    await displayed(240);
    await visibility(true);
    await displayed('—');
    assert.deepEqual(await counts(), { open: 1, retire: 1 });
    await page.evaluate('window.__authoredCan.setAxis(480)');
    await displayed('—');
    await visibility(false);
    await displayed(480);
    assert.deepEqual(await counts(), { open: 2, retire: 1 });
    await page.evaluate('window.__authoredCan.close()');
    assert.deepEqual(await counts(), { open: 2, retire: 2 });
  } finally {
    await page.evaluate('(async () => { await window.__authoredCan?.close(); delete window.__authoredCan; })()').catch(() => {});
    await page.close();
  }
});
