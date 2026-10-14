import assert from 'node:assert/strict';
import test from 'node:test';
import { loadModules } from './test-module-loader.mjs';
const modules = await loadModules(['./gateway-capabilities', './dcs-profile', './dcs-test-presets']);
const { newDcsProfile, dcsProfileError, dcsProfileStartError } = modules.get('./dcs-profile');
const { dcsTestPreset, dcsTestPresetQueries } = modules.get('./dcs-test-presets');
const plain = value => JSON.parse(JSON.stringify(value));
const gatewayId = 'pico:OSG-PICO-0123456789ABCDEF', moduleId = '00112233445566778899aabbccddeeff';
const profile = { ...newDcsProfile(), cataloguePath: '/installed/doc/json' };
const events = { boot: 7, contactButtonCount: 3, maintainedButtons: [0,1], encoderButtons: [3,4,5,6], axisCount: 1 };
const led = { id: 'led.status.set_mode', label: 'Status LED', endpoint: 'led', readable: true, semantics: 'setter', parameters: [{ type: 'enum', options: ['auto','on','off'] }] };
const displays = ['frequency','preset','text'].map(endpoint => ({ id: `display.${endpoint}.set_text`, label: endpoint, endpoint, readable: true, semantics: 'setter', parameters: [{ type: 'string', maxBytes: 63 }], display: { columns: 16, rows: 1, encoding: 'ascii' } }));
const actions = { boot: 7, status: 'ready', actions: [led,...displays] };
const module = { id: moduleId, online: true, boot: 7, enabled: false, published: false };
const target = { gatewayId, module, label: 'Radio panel', state: { connected: true, backend: 'pico', protocolVersion: 3, canProtocol: 3, moduleActions: { [moduleId]: actions }, moduleEvents: { [moduleId]: events } } };
const catalogue = [...new Set([...dcsTestPresetQueries.radio, ...dcsTestPresetQueries['master-caution']])].map(id => ({ id,
  inputs: id === 'UHF_PRESET_SEL' || id === 'UHF_10MHZ_SEL' ? [{ interface: 'fixed_step' }] : id === 'MASTER_CAUTION' || id === 'UHF_FREQUENCY' || id === 'UHF_PRESET' ? [] : [{ interface: 'set_state' }],
  outputs: [{ index: 2, type: ['UHF_FREQUENCY','UHF_PRESET'].includes(id) ? 'string' : 'integer' }],
}));
const apply = result => ({ ...profile, inputs: result.inputs, outputs: result.outputs });

test('radio test preset binds four physical kinds and LED plus three bounded displays', () => {
  const result = dcsTestPreset(profile, target, 'radio', catalogue);
  assert.equal(result.error, ''); assert.equal(result.inputs.length, 5); assert.equal(result.outputs.length, 4);
  assert.deepEqual(plain(result.inputs).map(input => [input.kind,input.control,input.command]), [['button',2,'UHF_TEST'],['switch',0,'UHF_SQUELCH'],['encoder',0,'UHF_PRESET_SEL'],['encoder',1,'UHF_10MHZ_SEL'],['axis',0,'UHF_VOL']]);
  assert.deepEqual(plain(result.outputs).map(output => output.actionId), ['display.frequency.set_text','display.preset.set_text','display.text.set_text',led.id]);
  assert.ok(result.outputs.every(output => output.output === 2));
  assert.equal(dcsProfileError(apply(result)), ''); assert.equal(dcsProfileStartError(apply(result), [target]), '');
  assert.equal(profile.inputs.length, 0, 'Preset generation does not save or mutate the existing profile');
});
test('master-caution uses the advertised momentary input and readable LED', () => {
  const result = dcsTestPreset(profile, target, 'master-caution', catalogue);
  assert.equal(result.error, ''); assert.equal(result.inputs.length, 1); assert.equal(result.outputs.length, 1);
  assert.equal(result.inputs[0].control, 2); assert.equal(result.outputs[0].actionId, led.id);
  const existing = { ...profile, inputs: [{ id: 'master-caution-button-1', gatewayId: 'other', moduleId: 'other', kind: 'button', control: 0, command: 'OTHER' }] };
  assert.equal(dcsTestPreset(existing, target, 'master-caution', catalogue).inputs[0].id, 'master-caution-button-2');
});
test('preset checks the online boot and capabilities before producing any rows', () => {
  for (const moduleChange of [{ online: false }, { boot: 0 }, { boot: 9 }]) assert.match(dcsTestPreset(profile, { ...target, module: { ...module, ...moduleChange } }, 'radio', catalogue).error, /Refresh/);
  for (const stateChange of [{ moduleEvents: {} }, { moduleActions: {} }, { moduleEvents: undefined }, { moduleActions: undefined }]) assert.match(dcsTestPreset(profile, { ...target, state: { ...target.state, ...stateChange } }, 'radio', catalogue).error, /Refresh/);
  assert.match(dcsTestPreset(profile, { ...target, state: null }, 'radio', catalogue).error, /Refresh/);
  const noEncoders = { ...target, state: { ...target.state, moduleEvents: { [moduleId]: { ...events, encoderButtons: [] } } } };
  assert.match(dcsTestPreset(profile, noEncoders, 'radio', catalogue).error, /encoder 1/);
});
test('catalogue names and supported interfaces are required without guessed addresses', () => {
  assert.match(dcsTestPreset(profile, target, 'radio', []).error, /UHF_TEST/);
  const invalid = catalogue.map(control => control.id === 'UHF_PRESET_SEL' ? { ...control, inputs: [{ interface: 'set_state' }] } : control);
  assert.match(dcsTestPreset(profile, target, 'radio', invalid).error, /fixed_step/);
  assert.match(dcsTestPreset(profile, target, 'radio', catalogue.filter(control => control.id !== 'UHF_FREQUENCY')).error, /feedback/);
  const wrong = catalogue.map(control => control.id === 'MASTER_CAUTION' ? { ...control, outputs: [{ index: 0, type: 'string' }] } : control);
  assert.match(dcsTestPreset(profile, target, 'master-caution', wrong).error, /feedback/);
});
test('advertised endpoint types are checked before adding all-or-nothing presets', () => {
  const missing = { ...target, state: { ...target.state, moduleActions: { [moduleId]: { ...actions, actions: [led] } } } };
  const result = dcsTestPreset(profile, missing, 'radio', catalogue);
  assert.match(result.error, /frequency output/); assert.equal(result.inputs.length, 0); assert.equal(result.outputs.length, 0);
  const unreadable = { ...target, state: { ...target.state, moduleActions: { [moduleId]: { ...actions, status: 'uncertain' } } } };
  assert.match(dcsTestPreset(profile, unreadable, 'master-caution', catalogue).error, /led output/);
  const endpointFree = displays.map(display => ({ ...display, endpoint: 'generic' }));
  assert.equal(dcsTestPreset(profile, { ...target, state: { ...target.state, moduleActions: { [moduleId]: { ...actions, actions: [led,...endpointFree] } } } }, 'radio', catalogue).error, '');
});
test('presets preserve existing bindings and refuse duplicate commands, source controls and actions', () => {
  const first = dcsTestPreset(profile, target, 'radio', catalogue);
  assert.match(dcsTestPreset(apply(first), target, 'radio', catalogue).error, /already bound/);
  assert.match(dcsTestPreset({ ...profile, inputs: [{ ...first.inputs[0], command: 'OTHER' }] }, target, 'radio', catalogue).error, /already bound/);
  assert.match(dcsTestPreset({ ...profile, outputs: [first.outputs[0]] }, target, 'radio', catalogue).error, /already bound/);
  const otherTarget = { ...target, gatewayId: 'pico:OSG-PICO-ANOTHER' };
  assert.match(dcsTestPreset(apply(first), otherTarget, 'radio', catalogue).error, /already bound/);
});
test('preset identifier allocation and bounded binding counts remain reviewable', () => {
  const occupied = { id: 'other', gatewayId: 'another', moduleId: 'another', kind: 'button', control: 0, command: 'OTHER' };
  const tooManyInputs = { ...profile, inputs: Array.from({ length: 12 }, (_, i) => ({ ...occupied, id: `other-${i}`, command: `OTHER_${i}` })) };
  assert.match(dcsTestPreset(tooManyInputs, target, 'radio', catalogue).error, /limit/);
  const tooManyOutputs = { ...profile, outputs: Array.from({ length: 13 }, (_, i) => ({ id: `other-${i}`, gatewayId: 'another', moduleId: 'another', actionId: 'other' })) };
  assert.match(dcsTestPreset(tooManyOutputs, target, 'radio', catalogue).error, /limit/);
});
test('Pico profile identities validate only stable serials and negotiated production contracts', () => {
  const result = dcsTestPreset(profile, target, 'master-caution', catalogue), active = apply(result);
  assert.equal(dcsProfileError(active), '');
  assert.match(dcsProfileError({ ...active, inputs: [{ ...active.inputs[0], gatewayId: 'pico:unrelated' }] }), /supported gateway/);
  for (const change of [{ protocolVersion: 1 }, { canProtocol: 2 }, { backend: 'unknown' }]) assert.match(dcsProfileStartError(active, [{ ...target, state: { ...target.state, ...change } }]), /Reconnect/);
});
