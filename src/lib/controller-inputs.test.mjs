import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from './test-module-loader.mjs';

const { controllerSourceId, loadControllerInputs } = (await loadModules(['./controller-inputs'])).get('./controller-inputs');
const source = (overrides = {}) => ({ id: 'registry-id', name: 'Stick', role: 'joystick', origin: 'host-controller', backend: 'hid',
  vendorId: 1, productId: 2, osgHid: false, online: true, generation: '7', sequence: '2', controlCount: 33,
  baselineReady: true, identityConfidence: 'serial', ...overrides });
const control = id => ({ capability: { id, kind: 'axis', usagePage: 1, usage: 48, reportId: 1, collection: 0, dataIndex: 0, bits: 16, min: -32768, max: 32767, hasNull: false }, value: { native: 0, normalized: 0 } });
const page = (offset, controls, overrides = {}) => ({ source: source(), controls, offset, total: 33, limit: 32, ...overrides });
const resource = connections => ({ id: 'profile-id', name: 'Stick', resourceType: 'host-controller', availability: 'online', generation: '7',
  capabilityRevision: 'caps', portCount: 0, cachedPortCount: 0, attributes: {}, diagnostics: [], connections });
const connection = (scope, transport, key) => ({ scope, key, route: { id: key, transport, providerGeneration: '7', attributes: {}, baselineReady: true, writable: false, priority: 0 } });

test('controller source uses the unique host-controller HID connection key', () => {
  assert.equal(controllerSourceId(resource([connection('host-controllers', 'hid', 'exact-device-key')])), 'exact-device-key');
  assert.equal(controllerSourceId(undefined), '');
  assert.equal(controllerSourceId({ ...resource([connection('host-controllers', 'hid', 'wrong-type')]), resourceType: 'osg-module' }), '');
  assert.equal(controllerSourceId(resource([connection('other-scope', 'hid', 'other'), connection('host-controllers', 'usb', 'usb')])), '');
  assert.equal(controllerSourceId(resource([connection('host-controllers', 'hid', 'one'), connection('host-controllers', 'hid', 'two')])), '');
});

test('loads every controller page using contiguous offsets and returns the combined snapshot', async () => {
  const calls = [];
  const send = async request => {
    calls.push(request);
    return request.offset === 0 ? page(0, Array.from({ length: 32 }, (_, i) => control(`control-${i}`)))
      : page(32, [control('control-32')]);
  };
  const result = await loadControllerInputs('registry-id', send);
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [
    { command: 'controller_inspect', sourceId: 'registry-id', offset: 0 },
    { command: 'controller_inspect', sourceId: 'registry-id', offset: 32 },
  ]);
  assert.equal(result.controls.length, 33);
  assert.equal(result.controls[32].capability.id, 'control-32');
});

test('rejects mismatched identity, generation, count, offset, stalled pages, and duplicate capabilities', async t => {
  const first = page(0, Array.from({ length: 32 }, (_, i) => control(`control-${i}`)));
  const changed = [
    ['identity', page(32, [control('last')], { source: source({ id: 'other' }) })],
    ['generation', page(32, [control('last')], { source: source({ generation: '8' }) })],
    ['count', page(32, [control('last')], { total: 34 })],
    ['offset', page(31, [control('last')])],
    ['empty page', page(32, [])],
    ['duplicate capability', page(32, [control('control-0')])],
  ];
  for (const [label, second] of changed) await t.test(label, async () => {
    let calls = 0;
    await assert.rejects(loadControllerInputs('registry-id', async () => ++calls === 1 ? first : second));
  });
});

test('bounds reported control counts and rejects invalid first-page progress', async () => {
  await assert.rejects(loadControllerInputs('registry-id', async () => page(0, [], { total: 8193 })), /too many/);
  await assert.rejects(loadControllerInputs('registry-id', async () => page(0, [], { total: 1 })), /progress/);
  await assert.rejects(loadControllerInputs('', async () => page(0, [])), /Choose an exact/);
});

test('cancellation before or after a request returns null without reading later pages', async () => {
  let active = false, calls = 0;
  assert.equal(await loadControllerInputs('registry-id', async () => { calls++; return page(0, []); }, () => active), null);
  assert.equal(calls, 0);
  active = true;
  const result = await loadControllerInputs('registry-id', async () => { calls++; active = false; return page(0, []); }, () => active);
  assert.equal(result, null);
  assert.equal(calls, 1);
});

test('offline snapshots clear input values without fetching additional pages', async () => {
  let calls = 0;
  const result = await loadControllerInputs('registry-id', async () => {
    calls++;
    return page(0, [control('stale')], { source: source({ online: false }) });
  });
  assert.equal(result.controls.length, 0);
  assert.equal(calls, 1);
});
