import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
const capabilities = {};
vm.runInNewContext(ts.transpileModule(await readFile(new URL('./gateway-capabilities.ts', import.meta.url), 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText, { exports: capabilities });
const require = name => { if (name === './gateway-capabilities') return capabilities; throw new Error(`Unexpected import ${name}`); };
const exported = {};
vm.runInNewContext(ts.transpileModule(await readFile(new URL('./dcs-profile.ts', import.meta.url), 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText, { exports: exported, TextEncoder, require }, { filename: new URL('./dcs-profile.ts', import.meta.url).pathname });
const { newDcsProfile, dcsProfileEqual, boundDcsTargets, dcsTargetKey, dcsProfileError, physicalDcsControls,
  dcsInputInterfaces, dcsOutputIndex, dcsBindingStatusLabel, dcsBindingState, dcsOutputActions, dcsEndpointLabel, dcsProfileStartError, managedDcsActions, dcsBindingValue } = exported;
const plain = value => JSON.parse(JSON.stringify(value));
const gatewayId = 'stm32:00112233445566778899aabb', moduleId = '00112233445566778899aabbccddeeff';
const legacy = { schemaVersion: 1, cataloguePath: '/installed/doc/json', gatewayId, moduleId, button: 0,
  ledActionId: 'led.status.set_mode', commandHost: '127.0.0.1', commandPort: 7778, listenAddress: '0.0.0.0', listenPort: 5010,
  multicastGroup: '239.255.50.10', multicastInterface: '127.0.0.1', freshnessMs: 1500 };
const profile = plain(newDcsProfile(legacy));
const button = profile.inputs[0], output = profile.outputs[0];
const events = { boot: 7, buttonCount: 8, contactButtonCount: 3, held: [false,false,false,false,false,false,true,false],
  maintainedButtons: [2], encoderButtons: [3,4], axisCount: 2, axes: [100,200], axisAvailable: [true,false] };
const led = { id: output.actionId, readable: true, semantics: 'setter', parameters: [{ type: 'enum', options: ['auto','on','off'] }] };
const display = { id: 'display.radio.set_text', readable: true, semantics: 'setter', parameters: [{ type: 'string', maxBytes: 16 }], display: { columns: 16, rows: 1, encoding: 'ascii' } };
const actions = { boot: 7, status: 'ready', actions: [led,display] };
const module = { id: moduleId, online: true, boot: 7, buttonCount: 8, enabled: false, published: false, directUsb: false };
const target = { gatewayId, module, label: 'Radio panel', state: { backend: 'stm32', connected: true, moduleActions: { [moduleId]: actions }, moduleEvents: { [moduleId]: events } } };

test('legacy master caution upgrades to two named bindings without changing saved data', () => {
  const migrated = plain(newDcsProfile(legacy));
  assert.equal(migrated.schemaVersion, 2); assert.equal(migrated.aircraftModule, 'A-10C');
  assert.equal(migrated.inputs[0].command, 'UFC_MASTER_CAUTION');
  assert.equal(migrated.outputs[0].control, 'MASTER_CAUTION'); assert.equal(migrated.outputs[0].actionId, legacy.ledActionId);
  assert.equal(migrated.commandPort, legacy.commandPort);
  assert.equal(legacy.schemaVersion, 1);
  const copy = newDcsProfile(migrated); copy.inputs[0].control = 2; copy.outputs[0].format.kind = 'text';
  assert.equal(migrated.inputs[0].control, 0); assert.equal(migrated.outputs[0].format.kind, 'boolean');
  assert.equal(newDcsProfile().inputs.length, 0); assert.equal(newDcsProfile(null).outputs.length, 0);
});
test('saved equality preserves array order and compares nested values rather than key order', () => {
  const reordered = Object.fromEntries(Object.entries(profile).reverse());
  assert.equal(dcsProfileEqual(profile, reordered), true);
  assert.equal(dcsProfileEqual(profile), false); assert.equal(dcsProfileEqual(profile, legacy), true);
  assert.equal(dcsProfileEqual({ ...profile, cataloguePath: '/another/catalogue' }, legacy), false);
  assert.equal(dcsProfileEqual(profile, { ...profile, outputs: [{ ...output, format: { kind: 'text' } }] }), false);
  assert.equal(dcsProfileEqual(profile, { ...profile, inputs: [{ ...button, control: 1 }] }), false);
  assert.equal(dcsProfileEqual({ ...profile, inputs: [{ ...button, step: undefined }] }, profile), true);
});
test('module targets stay scoped to gateway identity and are deduplicated independently', () => {
  assert.deepEqual(plain(boundDcsTargets(profile)), [{ gatewayId, moduleId }]);
  const other = { ...output, id: 'other', gatewayId: 'stm32:ffeeddccbbaa998877665544' };
  assert.equal(boundDcsTargets({ ...profile, outputs: [output,other] }).length, 2);
  assert.equal(dcsTargetKey(other), `${other.gatewayId}/${moduleId}`);
});
test('form accepts defaults but rejects malformed networks, identities and bounded binding counts', () => {
  assert.equal(dcsProfileError(profile), '');
  for (const change of [{ schemaVersion: 1 }, { aircraftModule: '../F-16' }, { cataloguePath: '' }, { cataloguePath: '/bad\npath' },
    { inputs: [], outputs: [] }, { inputs: Array(17).fill(button) }, { outputs: Array(17).fill(output) },
    { commandPort: 0 }, { listenPort: 65536 }, { freshnessMs: 99 }, { freshnessMs: 10001 },
    { commandHost: '' }, { listenAddress: 'bad address' }, { multicastInterface: ' ' }, { multicastGroup: ' bad' }]) assert.ok(dcsProfileError({ ...profile, ...change }), JSON.stringify(change));
  for (const change of [{ id: '' }, { id: 'x'.repeat(32) }, { gatewayId: 'other' }, { moduleId: 'bad' }]) assert.ok(dcsProfileError({ ...profile, inputs: [{ ...button, ...change }] }));
  assert.match(dcsProfileError({ ...profile, outputs: [{ ...output, id: button.id }] }), /unique/);
  const modules = Array.from({ length: 9 }, (_, i) => ({ ...button, id: `input-${i}`, moduleId: i.toString(16).padStart(32,'0') }));
  assert.match(dcsProfileError({ ...profile, inputs: modules, outputs: [] }), /eight/);
  const gateways = Array.from({ length: 5 }, (_, i) => ({ ...button, id: `input-${i}`, gatewayId: `stm32:${i.toString(16).padStart(24,'0')}` }));
  assert.match(dcsProfileError({ ...profile, inputs: gateways, outputs: [] }), /four/);
});
test('input form constrains physical kinds, catalogue interfaces, steps and target units', () => {
  for (const change of [{ kind: 'hat' }, { control: -1 }, { kind: 'axis', control: 8 }, { kind: 'encoder', control: 8 },
    { command: '' }, { command: 'bad\ncommand' }, { interface: 'fixed_step' }, { kind: 'encoder', interface: 'set_state' },
    { pressedValue: 65536 }, { releasedValue: -1 }, { deadband: NaN }, { step: 0 }]) assert.ok(dcsProfileError({ ...profile, inputs: [{ ...button, ...change }] }), JSON.stringify(change));
  assert.equal(dcsProfileError({ ...profile, inputs: [{ ...button, kind: 'encoder', interface: 'variable_step', step: 65535 }] }), '');
  assert.match(dcsProfileError({ ...profile, inputs: [button, { ...button, id: 'another', command: 'ANOTHER_CONTROL' }] }), /only once/);
  assert.match(dcsProfileError({ ...profile, inputs: [button, { ...button, id: 'another', control: 1 }] }), /only once/);
});
test('display format bounds preserve whole UTF-8 byte limits and supported formatting policies', () => {
  for (const change of [{ control: '' }, { actionId: '' }, { output: -1 }, { output: 64 },
    { format: { kind: 'bad' } }, { format: { kind: 'fixed', scale: Infinity } }, { format: { kind: 'fixed', scale: 1000001 } },
    { format: { kind: 'fixed', decimals: 7 } }, { format: { kind: 'text', prefix: 'é'.repeat(9) } },
    { format: { kind: 'text', suffix: '\n' } }, { format: { kind: 'text', unknownText: 'x'.repeat(64) } },
    { format: { kind: 'text', staleText: '\u0080' } }, { format: { kind: 'text', align: 'center' } },
    { format: { kind: 'text', overflow: 'wrap' } }]) assert.ok(dcsProfileError({ ...profile, outputs: [{ ...output, ...change }] }), JSON.stringify(change));
  assert.equal(dcsProfileError({ ...profile, outputs: [{ ...output, format: { kind: 'fixed', scale: -1000000, decimals: 6, prefix: 'é'.repeat(8), suffix: '', align: 'right', overflow: 'reject', unknownText: '?', staleText: '-' } }] }), '');
  assert.match(dcsProfileError({ ...profile, outputs: [output, { ...output, id: 'another-output' }] }), /only once/);
});
test('physical choices distinguish momentary contacts, maintained switches, encoder ordinals and calibrated axes', () => {
  assert.deepEqual(plain(physicalDcsControls('button', events)).map(v => v.value), [0,1]);
  assert.deepEqual(plain(physicalDcsControls('switch', events)).map(v => v.value), [2]);
  assert.deepEqual(plain(physicalDcsControls('encoder', events)), [{ value: 0, label: 'Encoder 0 · buttons 3/4' }]);
  assert.equal(physicalDcsControls('axis', events).length, 2);
  assert.equal(physicalDcsControls('axis', { axes: [0] }).length, 1);
  assert.deepEqual(plain(physicalDcsControls('button')), []);
  assert.deepEqual(plain(physicalDcsControls('button', { ...events, contactButtonCount: undefined })), []);
});
test('catalogue interfaces and output actions only offer compatible typed bindings', () => {
  const control = { inputs: [{ interface: 'set_state' }, { interface: 'fixed_step' }, { interface: 'variable_step' }, { interface: 'action' }, { interface: 'set_state' }] };
  assert.deepEqual(plain(dcsInputInterfaces('button', control)), ['set_state']);
  assert.deepEqual(plain(dcsInputInterfaces('encoder', control)), ['fixed_step','variable_step']);
  assert.deepEqual(plain(dcsInputInterfaces('axis')), []);
  const bool = { ...led, id: 'boolean', parameters: [{ type: 'boolean' }] };
  const generic = { ...display, id: 'string', display: undefined };
  const invalid = [{ ...bool, readable: false }, { ...display, semantics: 'oneShot' }, { ...display, parameters: [] }, { ...led, parameters: [{ type: 'enum', options: ['on'] }] }];
  assert.deepEqual(plain(dcsOutputActions('boolean', { ...actions, actions: [led,bool,display,...invalid] })).map(v => v.id), [led.id,bool.id]);
  assert.deepEqual(plain(dcsOutputActions('text', { ...actions, actions: [led,display,generic,...invalid] })).map(v => v.id), [display.id,generic.id]);
  assert.deepEqual(plain(dcsOutputActions('integer', { ...actions, status: 'uncertain' })), []);
  assert.deepEqual(plain(dcsOutputActions('fixed')), []);
  assert.match(dcsEndpointLabel(display), /16 × 1 cells · ASCII · 16 bytes/);
  assert.match(dcsEndpointLabel(generic), /UTF-8 · 16 bytes/);
  assert.match(dcsEndpointLabel(led), /on\/off/); assert.match(dcsEndpointLabel(), /Discover/);
});
test('normalized catalogue outputs keep their original wire index when unsupported fields are omitted', () => {
  const control = { outputs: [{ index: 2, type: 'string', maxLength: 8 }, { index: 5, type: 'integer', maxValue: 65535 }] };
  assert.equal(dcsOutputIndex('text', control), 2);
  for (const kind of ['boolean', 'integer', 'fixed']) assert.equal(dcsOutputIndex(kind, control), 5);
  assert.equal(dcsOutputIndex('text', { outputs: [{ index: 4, type: 'integer' }] }), 0);
  assert.equal(dcsOutputIndex('text'), 0);
});
test('legacy preset retains applied LED and release-before-arm diagnostics without fabricating new rows', () => {
  const status = { running: true, config: legacy, feedbackFresh: true, armed: false, needsBaseline: false, ledValue: true, error: null, commandsSent: 2 };
  assert.equal(dcsBindingState(status, 'input', 'master-caution-button').status, 'waitingRelease');
  assert.equal(dcsBindingState({ ...status, armed: true }, 'input', 'master-caution-button').ready, true);
  assert.equal(dcsBindingState({ ...status, needsBaseline: true }, 'input', 'master-caution-button').status, 'waiting');
  assert.equal(dcsBindingState({ ...status, error: 'route lost' }, 'input', 'master-caution-button').status, 'error');
  assert.equal(dcsBindingState(status, 'output', 'master-caution-led').applied, true);
  assert.equal(dcsBindingState({ ...status, feedbackFresh: false }, 'output', 'master-caution-led').status, 'stale');
  for (const change of [{ running: false }, { ledValue: null }]) assert.equal(dcsBindingState({ ...status, ...change }, 'output', 'master-caution-led').status, 'unknown');
  assert.equal(dcsBindingState({ ...status, running: false }, 'input', 'master-caution-button').status, 'waiting');
  assert.equal(dcsBindingState(status, 'output', 'new-output'), undefined);
  assert.equal(dcsBindingState(undefined, 'input', 'master-caution-button'), undefined);
  const row = { id: 'master-caution-led', applied: 'native row', status: 'ready' };
  assert.equal(dcsBindingState({ ...status, config: profile, outputs: [row] }, 'output', row.id), row);
  assert.equal(dcsBindingState({ ...status, config: profile }, 'input', 'master-caution-button'), undefined);
});
test('start checks every scoped route, input ownership and current advertised capabilities', () => {
  assert.equal(dcsProfileStartError(profile, [target]), '');
  assert.match(dcsProfileStartError({ ...profile, cataloguePath: '' }, [target]), /folder/);
  assert.match(dcsProfileStartError(profile, []), /Reconnect/);
  for (const moduleChange of [{ online: false }, { boot: 0 }, { directUsb: true }, { enabled: true }, { published: true }]) assert.ok(dcsProfileStartError(profile, [{ ...target, module: { ...module, ...moduleChange } }]));
  for (const stateChange of [{ connected: false }, { reconnecting: true }, { layoutPending: true }, { claimedModules: [moduleId] }]) assert.ok(dcsProfileStartError(profile, [{ ...target, state: { ...target.state, ...stateChange } }]));
  assert.match(dcsProfileStartError(profile, [{ ...target, busy: true }]), /Wait/);
  assert.match(dcsProfileStartError(profile, [{ ...target, state: { ...target.state, moduleEvents: {} } }]), /capabilities/);
  assert.match(dcsProfileStartError(profile, [{ ...target, state: { ...target.state, moduleActions: {} } }]), /compatible/);
  assert.match(dcsProfileStartError({ ...profile, inputs: [{ ...button, kind: 'switch', control: 0 }] }, [target]), /switch/);
  assert.equal(dcsProfileStartError({ ...profile, inputs: [] }, [{ ...target, module: { ...module, enabled: true, published: true, directUsb: true } }]), '', 'Output-only bindings do not remove valid HID input ownership');
  const older = { ...target, state: { ...target.state, moduleEvents: {} } };
  assert.equal(dcsProfileStartError(profile, [older], legacy), '', 'Unchanged legacy profiles can start without new typed input metadata');
  assert.match(dcsProfileStartError(profile, [{ ...older, module: { ...module, buttonCount: 0 } }], legacy), /momentary/);
  assert.match(dcsProfileStartError(profile, [{ ...target, state: { ...target.state, moduleEvents: { [moduleId]: { ...events, encoderButtons: [0,1] } } } }], legacy), /momentary/);
});
test('manual guards scope all managed actions and live value labels retain unknown values', () => {
  assert.deepEqual(plain(managedDcsActions({ running: true, config: legacy }, gatewayId, moduleId)), [led.id]);
  assert.deepEqual(plain(managedDcsActions({ running: true, config: profile }, gatewayId, moduleId)), [led.id]);
  for (const status of [undefined, { running: false, config: profile }, { running: true, config: null }]) assert.deepEqual(plain(managedDcsActions(status, gatewayId, moduleId)), []);
  assert.deepEqual(plain(managedDcsActions({ running: true, config: legacy }, 'other', moduleId)), []);
  assert.deepEqual(plain(managedDcsActions({ running: true, config: profile }, gatewayId, 'other')), []);
  assert.equal(dcsBindingValue(undefined), 'Unknown'); assert.equal(dcsBindingValue(null), 'Unknown');
  assert.equal(dcsBindingValue(true), 'On'); assert.equal(dcsBindingValue(false), 'Off');
  assert.equal(dcsBindingValue([1,'123.450',true]), '1 · 123.450 · On');
  assert.equal(dcsBindingStatusLabel({ ready: true, armed: false, status: 'waitingRelease' }), 'Waiting for release');
  assert.equal(dcsBindingStatusLabel({ ready: true }), 'Ready');
  assert.equal(dcsBindingStatusLabel(), 'Unknown');
  assert.equal(dcsBindingStatusLabel({ status: 'stale' }), 'Stale');
  assert.equal(dcsBindingStatusLabel({ status: 'future-status' }), 'future-status');
});
