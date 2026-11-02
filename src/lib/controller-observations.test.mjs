import test from 'node:test';
import assert from 'node:assert/strict';
import { ControllerInputObservations, observeControllerInputs } from './controller-observations.ts';
import { SharedObservations } from './streams/shared.ts';

const id = '11111111-1111-4111-8111-111111111111';
const otherId = '22222222-2222-4222-8222-222222222222';
function controller(overrides = {}) {
  return { id, name: 'Pedals', role: 'pedals', origin: 'host-controller', backend: 'windows',
    vendorId: 1, productId: 2, osgHid: false, online: true, generation: '1', sequence: '1',
    controlCount: 1, baselineReady: true, identityConfidence: 'local-path', ...overrides };
}
function control(overrides = {}) {
  return { capability: { id: 'axis', kind: 'axis', usagePage: 1, usage: 48, reportId: 0,
    collection: 0, dataIndex: 0, bits: 16, min: 0, max: 100, hasNull: false, ...overrides },
  value: { native: 0, normalized: 0 } };
}
function snapshot(overrides = {}) {
  return { session: 'registry', source: controller(), identity: { path: 'usb', collection: '0',
    serial: null, fingerprint: 'fingerprint' }, diagnostics: [], controls: [control()],
  offset: 0, total: 1, limit: 256, cursor: '1', capturedUs: '10', ...overrides };
}
function change(overrides = {}) {
  return { session: 'registry', source: controller({ sequence: '2' }), reason: 'change',
    values: { axis: { native: 100, normalized: 32767 } }, cursor: '2', capturedUs: '11', ...overrides };
}
function setup() {
  const opens = [], states = [];
  const observations = new SharedObservations({ open(topic, observer, signal) { opens.push({ topic, observer, signal }); } });
  const client = new ControllerInputObservations(observations);
  const off = client.observe(id, state => states.push(state));
  let delivery = 0;
  function send(payload = snapshot(), options = {}, open = opens.at(-1)) {
    delivery++;
    const kind = options.kind ?? (delivery === 1 ? 'snapshot' : 'change');
    open.observer.frame({ subscriptionId: 'token', delivery: String(delivery), frame: {
      topic: open.topic, source: 'controller-input', session: 'runtime', generation: '2', revision: String(delivery),
      capturedUs: payload?.capturedUs ?? '11', clockId: `controllers:${payload?.session ?? 'registry'}`,
      kind, continuity: kind === 'snapshot' ? 'baseline' : kind === 'loss' ? 'lost' : 'continuous', payload, ...options,
    } }, async () => {});
  }
  return { client, observations, opens, states, off, send };
}

test('shares reduction, late snapshot, clone isolation and reference-counted cancellation', () => {
  const { client, states, opens, off, send } = setup();
  const seen = [];
  const second = client.observe(id, state => { seen.push(state); if (state.detail) state.detail.controls[0].value.native = 999; });
  assert.equal(opens.length, 1);
  assert.equal(opens[0].topic, `controllers.input.${id}`);
  send();
  assert.equal(states.at(-1).detail.controls[0].value.native, 0);
  const late = [];
  const third = client.observe(id, state => late.push(state));
  assert.equal(late.at(-1).detail.controls[0].value.native, 0);
  send(change());
  assert.equal(states.at(-1).detail.controls[0].value.native, 100);
  off(); off(); second();
  assert.equal(opens[0].signal.aborted, false);
  third();
  assert.equal(opens[0].signal.aborted, true);
  const before = states.length;
  opens[0].observer.error('late');
  send(change());
  assert.equal(states.length, before);
});

test('partial baselines clear all values, complete report restores them and metadata preserves inputs', () => {
  const { states, send } = setup();
  send(snapshot({ source: controller({ baselineReady: false }), controls: [{ ...control(), value: null }] }));
  assert.equal(states.at(-1).status, 'ready');
  assert.equal(states.at(-1).detail.controls[0].value, null);
  send(change({ reason: 'baseline', source: controller({ baselineReady: false, sequence: '2' }), values: {} }));
  assert.equal(states.at(-1).status, 'ready');
  assert.equal(states.at(-1).detail.source.baselineReady, false);
  assert.equal(states.at(-1).detail.source.online, true, 'reader loss cannot invent a native disconnect');
  assert.equal(states.at(-1).detail.controls[0].value, null);
  send(change({ reason: 'baseline' }));
  assert.equal(states.at(-1).detail.controls[0].value.native, 100);
  send(change({ reason: 'metadata', source: controller({ name: 'Renamed', sequence: '2' }), values: {} }));
  assert.equal(states.at(-1).detail.source.name, 'Renamed');
  assert.equal(states.at(-1).detail.controls[0].value.native, 100);
  send(change({ reason: 'disconnect', source: controller({ online: false, baselineReady: false, sequence: '3' }), values: {} }));
  assert.equal(states.at(-1).detail.controls[0].value, null);
  assert.equal(states.at(-1).detail.source.online, false);
});

test('valid raw out-of-range samples and unsupported diagnostic descriptors are preserved truthfully', () => {
  const { states, send } = setup();
  const raw = control({ kind: 'diagnostic', min: 99, max: -1, bits: 0 });
  raw.value = { native: 4294967295, normalized: null };
  send(snapshot({ source: controller({ controlCount: 2 }), controls: [raw, control({ id: 'routable' })], total: 2 }));
  assert.equal(states.at(-1).status, 'ready');
  assert.deepEqual(states.at(-1).detail.controls[0], raw);
  send(snapshot({ controls: [control()] }), { kind: 'snapshot' });
  send(change({ values: { axis: { native: 101, normalized: null } } }));
  assert.equal(states.at(-1).status, 'ready');
  assert.equal(states.at(-1).detail.controls[0].value.normalized, null);
});

test('unsupported diagnostic descriptors do not prevent a complete routable baseline', () => {
  const { states, send } = setup();
  const raw = { ...control({ id: 'unsupported', kind: 'diagnostic', bits: 64, min: 99, max: -1 }), value: null };
  send(snapshot({ source: controller({ controlCount: 2, baselineReady: false }), controls: [{ ...control(), value: null }, raw], total: 2 }));
  send(change({ reason: 'baseline', source: controller({ controlCount: 2, sequence: '2' }) }));
  assert.equal(states.at(-1).status, 'ready');
  assert.equal(states.at(-1).detail.controls[0].value.native, 100);
  assert.equal(states.at(-1).detail.controls[1].value, null);
});

test('loss withdraws values and replacement generation requires its own descriptors', () => {
  const { states, send } = setup();
  send();
  send(undefined, { kind: 'loss', generation: '3', reason: 'replacement' });
  assert.equal(states.at(-1).status, 'lost');
  assert.equal(states.at(-1).detail.controls[0].value, null);
  assert.equal(states.at(-1).detail.source.baselineReady, false);
  send(change(), { generation: '2' });
  assert.equal(states.at(-1).status, 'lost');
  send(snapshot({ source: controller({ generation: '2' }), controls: [control({ id: 'new-axis' })] }), { kind: 'snapshot', generation: '3' });
  assert.equal(states.at(-1).status, 'ready');
  assert.equal(states.at(-1).detail.controls[0].capability.id, 'new-axis');
  send(change({ source: controller({ generation: '2' }) }), { generation: '3' });
  assert.equal(states.at(-1).status, 'error');
});

test('malformed update is atomic and cannot recover through deltas', () => {
  const { states, send } = setup();
  const controls = [control(), control({ id: 'second' })];
  send(snapshot({ source: controller({ controlCount: 2 }), controls, total: 2 }));
  send(change({ source: controller({ controlCount: 2 }), values: { axis: { native: 100, normalized: 32767 }, second: { native: 1.5, normalized: 0 } } }));
  assert.equal(states.at(-1).status, 'error');
  assert.ok(states.at(-1).detail.controls.every(control => control.value === null));
  assert.equal(states.at(-2).detail.controls[0].value.native, 0);
  send(change({ source: controller({ controlCount: 2 }) }));
  assert.equal(states.at(-1).status, 'error');
  assert.ok(states.at(-1).detail.controls.every(control => control.value === null));
  send(snapshot(), { kind: 'snapshot' });
  assert.equal(states.at(-1).status, 'ready');
});

test('source changes open fresh exact-topic subscriptions and retire old callbacks', () => {
  const { client, states, send, off, opens } = setup();
  send();
  off();
  const next = [];
  const stop = client.observe(otherId, state => next.push(state));
  assert.equal(next.at(-1).status, 'connecting');
  send(snapshot({ source: controller({ id: otherId }) }), { kind: 'snapshot' });
  assert.equal(next.at(-1).status, 'ready');
  send(change(), {}, opens[0]);
  assert.equal(states.length, 3); // Initial client and shared connecting, then baseline.
  stop();
  client.observe(id, state => states.push(state));
  assert.equal(states.at(-1).status, 'connecting');
  assert.equal(opens.length, 3);
  const invalid = [];
  client.observe('not-a-uuid', state => invalid.push(state));
  assert.equal(invalid[0].status, 'error');
});

test('valid signed axes, buttons and hats retain native ranges and normalized reports', () => {
  for (const [descriptor, value] of [
    [{ min: -100, max: 100 }, { native: -100, normalized: -32768 }],
    [{ kind: 'button', min: 0, max: 1, bits: 1 }, { native: 1, normalized: 1 }],
    [{ kind: 'button', min: 0, max: 1, bits: 1 }, { native: 0, normalized: 0 }],
    [{ kind: 'hat', min: 1, max: 8 }, { native: 3, normalized: 2 }],
    [{ kind: 'hat', min: 0, max: 3 }, { native: 2, normalized: 4 }],
  ]) {
    const { states, send } = setup();
    const input = control(descriptor); input.value = value;
    send(snapshot({ controls: [input] }));
    assert.equal(states.at(-1).status, 'ready', JSON.stringify(input));
    assert.deepEqual(states.at(-1).detail.controls[0], input);
  }
});

test('invalid payload, identity, summaries, descriptors and samples are fully rejected', () => {
  const cases = [
    null, {}, snapshot({ session: '' }), snapshot({ cursor: '01' }), snapshot({ capturedUs: 10 }),
    snapshot({ offset: 1 }), snapshot({ limit: 32 }), snapshot({ total: 257 }), snapshot({ total: 0 }),
    snapshot({ controls: null }), snapshot({ controls: [control(), control()] }),
    snapshot({ identity: null }), snapshot({ identity: {} }), snapshot({ diagnostics: [3] }),
    ...['id', 'name', 'role', 'origin', 'backend', 'vendorId', 'productId', 'osgHid', 'online',
      'generation', 'sequence', 'controlCount', 'baselineReady', 'identityConfidence'].map(field => snapshot({ source: controller({ [field]: null }) })),
    snapshot({ source: controller({ online: false }) }), snapshot({ source: controller({ generation: '9' }) }),
    ...['id', 'kind', 'usagePage', 'usage', 'reportId', 'collection', 'dataIndex', 'bits', 'min', 'max', 'hasNull'].map(field => snapshot({ controls: [control({ [field]: null })] })),
    ...[{ min: 2, max: 1 }, { bits: 0 }, { bits: 33 }, { id: '' }, { kind: 'diagnostic' },
      { kind: 'hat' }, { kind: 'button' }].map(descriptor => {
      const input = control(descriptor); input.value = { native: 20, normalized: 6553 };
      return snapshot({ controls: [input] });
    }),
    ...[{ native: -2147483649, normalized: null }, { native: 4294967296, normalized: null },
      { native: 0.5, normalized: 0 }, { native: 101, normalized: 32767 }, { native: 0, normalized: 1 },
      { native: 0, normalized: 32768 }, { native: 0, normalized: -32769 }].map(value => snapshot({ controls: [{ ...control(), value }] })),
    snapshot({ source: controller({ baselineReady: false }) }),
  ];
  for (const payload of cases) {
    const { states, send } = setup();
    send(payload);
    assert.equal(states.at(-1).status, 'error', JSON.stringify(payload));
    assert.equal(states.at(-1).detail, undefined);
  }
});

test('invalid change identities, reasons, baseline completeness and unknown IDs withdraw input', () => {
  for (const payload of [change({ session: 'other' }), change({ source: controller({ generation: '2' }) }),
    change({ cursor: '0' }), change({ capturedUs: '9' }), change({ source: controller({ sequence: '0' }) }),
    change({ reason: 'unknown' }), change({ values: null }), change({ values: { unknown: { native: 1, normalized: 1 } } }),
    change({ reason: 'baseline', values: {} }), change({ reason: 'gap' }), change({ reason: 'disconnect' }),
    change({ source: controller({ baselineReady: false }) }),
  ]) {
    const { states, send } = setup(); send(); send(payload);
    assert.equal(states.at(-1).status, 'error', JSON.stringify(payload));
    assert.equal(states.at(-1).detail.controls[0].value, null);
  }
});

test('wire errors and clock/source mismatch never expose values; throwing listeners remain isolated', () => {
  for (const overrides of [{ source: 'other' }, { clockId: 'other' }, { capturedUs: '12' }]) {
    const { states, send } = setup(); send(snapshot(), overrides);
    assert.equal(states.at(-1).status, 'error');
  }
  const { client, states, send, opens } = setup();
  const stop = client.observe(id, () => { throw Error('consumer failed'); });
  send();
  opens[0].observer.error('transport failed');
  assert.equal(states.at(-1).status, 'error');
  assert.equal(states.at(-1).error, 'transport failed');
  assert.equal(states.at(-1).detail.controls[0].value, null);
  stop();
  const invalid = [];
  observeControllerInputs('', state => invalid.push(state))();
  observeControllerInputs('', () => { throw Error('invalid source consumer'); })();
  assert.equal(invalid[0].status, 'error');
});
