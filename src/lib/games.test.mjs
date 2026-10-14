import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./runtime-config', './integration-profile', './games']);
const { visibleGames, defaultGameProfile, newGameProfile, cloneGameProfile, gameProfileEqual, gameDeviceKey, profileChangesBindings, gameProfileError, gameSessionLabel } = modules.get('./games');
const plain = value => JSON.parse(JSON.stringify(value));
const gatewayId = 'stm32:111111111111111111111111', moduleId = '0123456789abcdef0123456789abcdef';
const configuration = () => ({ name: 'Pedals', expanderMask: 0, buttons: [[0, 0, 0]], axes: [[0, 48, 0, 4095, 32768, 0, 100]], hats: [], encoders: [], selectors: [] });
const profile = () => plain(newGameProfile('dcs'));
const device = () => ({ gatewayId, moduleId, controller: 'keep' });
const integration = () => ({
  document: { schemaVersion: 1, id: 'rally-panel', name: 'Rally rev lights', provider: 'assetto-corsa-rally', context: {}, roles: [{ id: 'panel', label: 'Panel' }], inputs: [], outputs: [{ id: 'rev', role: 'panel', endpoint: 'led', actionId: 'led.set', channel: 'rev_ratio', output: 0, format: { kind: 'threshold', threshold: 0.8, comparison: 'gte' } }] },
  connection: { freshnessMs: 1000, source: 'shared-memory', processId: 123 }, assignments: [{ role: 'panel', gatewayId, moduleId }],
});

test('game library defaults to installations and shows all supported games on request', () => {
  const catalogue = [{ id: 'dcs', installed: true }, { id: 'assetto-corsa-rally', installed: false }];
  assert.deepEqual(plain(visibleGames(catalogue, false)), [catalogue[0]]);
  assert.deepEqual(plain(visibleGames(catalogue, true)), catalogue);
  assert.deepEqual(plain(visibleGames([], false)), []);
  assert.equal(catalogue.length, 2);
});

test('default selection remains scoped to a game and tolerates removed or missing defaults', () => {
  const first = profile(), second = { ...profile(), id: 'dcs-2', name: 'Jet' }, other = plain(newGameProfile('assetto-corsa-rally'));
  const state = { profiles: [first, second, other], defaultProfiles: { dcs: second.id } };
  assert.equal(defaultGameProfile(state, 'dcs'), second);
  assert.equal(defaultGameProfile(state, 'assetto-corsa-rally'), other);
  state.defaultProfiles.dcs = 'removed'; assert.equal(defaultGameProfile(state, 'dcs'), first);
  state.defaultProfiles.dcs = other.id; assert.equal(defaultGameProfile(state, 'dcs'), first);
  assert.equal(defaultGameProfile({ profiles: [], defaultProfiles: {} }, 'dcs'), undefined);
});

test('new profiles get unique IDs without mutating or sharing saved configuration', () => {
  const first = profile(), second = plain(newGameProfile('dcs', [first]));
  assert.equal(first.name, 'Default'); assert.equal(second.name, 'Profile 2'); assert.notEqual(first.id, second.id);
  const third = plain(newGameProfile('dcs', [first, second])); assert.equal(third.id, 'dcs-3');
  first.devices.push({ ...device(), config: configuration() });
  const copy = cloneGameProfile(first); copy.devices[0].config.axes[0][5] = 1; copy.name = 'Inverted';
  assert.equal(first.name, 'Default'); assert.equal(first.devices[0].config.axes[0][5], 0);
  assert.equal(gameProfileEqual(first, copy), false); assert.equal(gameProfileEqual(first, null), false); assert.equal(gameProfileEqual(first), false);
  const reordered = { name: first.name, devices: [{ config: first.devices[0].config, controller: 'keep', moduleId, gatewayId }], gameId: 'dcs', id: first.id, schemaVersion: 1, integration: null };
  assert.equal(gameProfileEqual(first, reordered), true);
  assert.notEqual(gameDeviceKey({ gatewayId: 'ab', moduleId: 'c' }), gameDeviceKey({ gatewayId: 'a', moduleId: 'bc' }));
});

test('profiles warn about controller changes and captured mappings before activation', () => {
  const value = profile(); assert.equal(profileChangesBindings(value), false);
  value.devices = [device()]; assert.equal(profileChangesBindings(value), false);
  value.devices[0].controller = 'disabled'; assert.equal(profileChangesBindings(value), true);
  value.devices[0].controller = 'enabled'; assert.equal(profileChangesBindings(value), true);
  value.devices[0].controller = 'keep'; value.devices[0].config = configuration(); assert.equal(profileChangesBindings(value), true);
});

test('valid profiles support different configurations and keep a copied integration independent', () => {
  const value = profile(); assert.equal(gameProfileError(value), '');
  value.devices = [{ ...device(), config: configuration() }]; assert.equal(gameProfileError(value), '');
  value.gameId = 'assetto-corsa-rally'; value.integration = integration(); assert.equal(gameProfileError(value), '');
  delete value.integration.connection.processId; assert.equal(gameProfileError(value), '');
  assert.equal(value.integration.connection.processId, undefined);
  const copy = cloneGameProfile(value); copy.integration.document.name = 'Changed'; assert.equal(value.integration.document.name, 'Rally rev lights');
  value.integration = null; assert.equal(gameProfileError(value), '');
  delete value.integration; assert.equal(gameProfileError(value), '');
});

test('profile validation explains invalid version, IDs and bounded UTF-8 names', () => {
  for (const value of [null, [], false, 1, 'profile', {}, { ...profile(), schemaVersion: 2 }, { ...profile(), gameId: 'unknown' }]) assert.match(gameProfileError(value), /supported game/);
  for (const id of ['', '../profile', 'space here', 'a'.repeat(65), 5, undefined]) assert.match(gameProfileError({ ...profile(), id }), /profile ID/);
  for (const name of ['', '  ', 'a'.repeat(81), 'é'.repeat(41), 'one\ntwo', '\u0080', 5, undefined]) assert.match(gameProfileError({ ...profile(), name }), /profile name/);
  assert.equal(gameProfileError({ ...profile(), name: 'é'.repeat(40), id: 'A-_.1' }), '');
});

test('peripheral validation keeps same identities on separate gateways distinct and rejects duplicate routes', () => {
  for (const devices of [null, {}, Array.from({ length: 33 }, device)]) assert.match(gameProfileError({ ...profile(), devices }), /32 peripherals/);
  for (const item of [null, [], {}, { ...device(), gatewayId: '' }, { ...device(), gatewayId: 'x'.repeat(128) }, { ...device(), moduleId: '\n' }, { ...device(), controller: 'maybe' }]) assert.match(gameProfileError({ ...profile(), devices: [item] }), /controller mode/);
  assert.match(gameProfileError({ ...profile(), devices: [device(), device()] }), /only once/);
  assert.match(gameProfileError({ ...profile(), devices: [device(), { ...device(), moduleId: moduleId.toUpperCase() }] }), /only once/);
  assert.equal(gameProfileError({ ...profile(), devices: [device(), { ...device(), gatewayId: 'stm32:222222222222222222222222' }] }), '');
  assert.equal(gameProfileError({ ...profile(), devices: [device(), { ...device(), moduleId: 'second-module', controller: 'disabled' }] }), '');
});

test('invalid captured configurations are rejected without changing saved mappings', () => {
  for (const config of [null, [], {}, { ...configuration(), name: null }, { ...configuration(), expanderMask: -1 }, { ...configuration(), expanderMask: 256 }, { ...configuration(), buttons: null }, { ...configuration(), axes: [1] }]) assert.match(gameProfileError({ ...profile(), devices: [{ ...device(), config }] }), /complete, valid/);
  const wrongCalibration = configuration(); wrongCalibration.axes[0][2] = 4094; wrongCalibration.axes[0][3] = 4094;
  assert.match(gameProfileError({ ...profile(), devices: [{ ...device(), config: wrongCalibration }] }), /minimum must be below/);
  const wrongName = configuration(); wrongName.name = '';
  assert.match(gameProfileError({ ...profile(), devices: [{ ...device(), config: wrongName }] }), /device name/);
  const noInputs = configuration(); noInputs.buttons = []; noInputs.axes = [];
  assert.equal(gameProfileError({ ...profile(), devices: [{ ...device(), config: noInputs }] }), '');
});

test('integration validation requires a valid copied configuration matching the selected game', () => {
  assert.match(gameProfileError({ ...profile(), integration: {} }), /JSON profile/i);
  assert.match(gameProfileError({ ...profile(), integration: integration() }), /provider must match/);
  const value = integration(); value.document.roles = [];
  assert.match(gameProfileError({ ...profile(), gameId: 'assetto-corsa-rally', integration: value }), /roles/);
});

test('every session phase has a user-facing description, including launch and recovery', () => {
  const phases = ['idle', 'validating', 'applying', 'launching', 'waiting', 'running', 'restoring', 'recovery-needed', 'failed'];
  for (const phase of phases) assert.ok(gameSessionLabel({ phase }).length > 5);
  assert.match(gameSessionLabel({ phase: 'recovery-needed' }), /Recovery/);
  assert.match(gameSessionLabel({ phase: 'restoring' }), /previous setup/);
});
