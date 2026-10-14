import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./runtime-config', './integration-profile', './games']);
const { visibleGames, defaultGameProfile, newGameProfile, cloneGameProfile, gameProfileEqual, gameDeviceKey, gameIntegrationLabel, gameIntegrationLiveLabel, gameIntegrationNativeVerified, gameIntegrationReady, gameIntegrationVerificationLabel, gameIntegrationSetupRequest, gameIntegrationTargetError, gameIntegrationUserFolder, gameIntegrationFolderError, gameIntegrationSetupBlockedReason, profileChangesBindings, gameProfileError, gameSessionLabel } = modules.get('./games');
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

test('integration readiness stays independent from live telemetry and never assumes installation is ready', () => {
  const game = { id: 'dcs', installed: true };
  assert.equal(gameIntegrationLabel(game), 'Integration not checked');
  const states = ['not-installed', 'setup-required', 'setting-up', 'ready', 'needs-attention', 'unsupported-platform'];
  for (const state of states) assert.ok(gameIntegrationLabel({ ...game, integrationSetup: { state } }).length > 5);
  game.integrationSetup = { state: 'ready', live: 'waiting' };
  assert.equal(gameIntegrationLabel(game), 'Ready for integration');
  assert.equal(gameIntegrationLiveLabel(game), 'Waiting for game data');
  game.integrationSetup.live = 'receiving';
  assert.equal(gameIntegrationLiveLabel(game), 'Receiving game data');
  game.integrationSetup.live = 'stale';
  assert.equal(gameIntegrationLiveLabel(game), 'Game data is stale');
  assert.equal(gameIntegrationLabel(game), 'Ready for integration');
  game.installed = false;
  assert.equal(gameIntegrationLabel(game), 'Game not installed');
  assert.equal(gameIntegrationLiveLabel({}), 'Waiting for game data');
});

test('DCS setup chooses a single detected folder but requires a choice when several exist', () => {
  const first = String.raw`C:\Users\Pilot\Saved Games\DCS`, second = String.raw`D:\Profiles\DCS.openbeta`;
  assert.equal(gameIntegrationUserFolder({}), '');
  assert.equal(gameIntegrationUserFolder({ integrationSetup: { candidates: [] } }), '');
  assert.equal(gameIntegrationUserFolder({ integrationSetup: { candidates: [first] } }), first);
  assert.equal(gameIntegrationUserFolder({ integrationSetup: { candidates: [first, second] } }), '');
  assert.equal(gameIntegrationUserFolder({ integrationSetup: { candidates: [first, second], userFolder: second } }), second);
});

test('DCS setup requires an absolute target and rejects control characters before requesting a write', () => {
  for (const path of ['', ' ', 'DCS', '..\\DCS', 'C:DCS', String.raw`\Saved Games\DCS`]) assert.ok(gameIntegrationFolderError(path));
  for (const path of [String.raw`C:\Users\Pilot\Saved Games\DCS`, 'D:/Profiles/DCS', String.raw`\\server\Profiles\DCS`, '/home/pilot/Saved Games/DCS', '  /home/pilot/DCS  ']) assert.equal(gameIntegrationFolderError(path), '');
  assert.match(gameIntegrationFolderError('C:/Users/pilot/\nDCS'), /control/);
  assert.match(gameIntegrationFolderError('C:/Users/pilot/\u0000DCS'), /control/);
});

test('setup writes are blocked while a game profile, integration, setup or recovery owns the files', () => {
  const game = { id: 'dcs', installed: true, integrationSetup: { state: 'setup-required', message: 'Setup required.' } };
  assert.equal(gameIntegrationSetupBlockedReason(game, { phase: 'idle' }, false), '');
  assert.equal(gameIntegrationSetupBlockedReason(game, { phase: 'failed' }, false), '');
  for (const phase of ['validating', 'applying', 'launching', 'waiting', 'running', 'restoring', 'recovery-needed']) assert.match(gameIntegrationSetupBlockedReason(game, { phase }, false), /game profile/);
  assert.match(gameIntegrationSetupBlockedReason(game, { phase: 'idle' }, true), /active integration/);
  assert.match(gameIntegrationSetupBlockedReason({ ...game, installed: false }, { phase: 'idle' }, false), /Install the game/);
  game.integrationSetup.state = 'unsupported-platform';
  game.integrationSetup.message = 'Setup requires Windows.';
  assert.equal(gameIntegrationSetupBlockedReason(game, { phase: 'idle' }, false), 'Setup requires Windows.');
  game.integrationSetup.state = 'setting-up';
  assert.match(gameIntegrationSetupBlockedReason(game, { phase: 'idle' }, false), /in progress/);
  game.integrationSetup.state = 'needs-attention';
  game.integrationSetup.recoveryNeeded = true;
  assert.match(gameIntegrationSetupBlockedReason(game, { phase: 'idle' }, false), /Recover/);
});

test('ACR native verification requires shared memory and survives a closed game independently of live data', () => {
  const game = { id: 'assetto-corsa-rally', installed: true, integrationSetup: { state: 'needs-attention', connection: { source: 'shared-memory' }, verified: false, live: 'waiting', optionalMaxRpm: false } };
  assert.equal(gameIntegrationLabel(game), 'Telemetry configured · verification needed');
  assert.equal(gameIntegrationNativeVerified(game), false);
  assert.equal(gameIntegrationReady(game), false);
  assert.equal(gameIntegrationVerificationLabel(game), 'Native telemetry not verified');
  game.integrationSetup.verified = true;
  game.integrationSetup.state = 'ready';
  assert.equal(gameIntegrationNativeVerified(game), true);
  assert.equal(gameIntegrationReady(game), true);
  assert.equal(gameIntegrationLabel(game), 'Ready for integration');
  assert.equal(gameIntegrationVerificationLabel(game), 'Native telemetry verified');
  assert.equal(gameIntegrationLiveLabel(game), 'Waiting for game data');
  game.integrationSetup.live = 'stale';
  assert.equal(gameIntegrationReady(game), true);
  assert.equal(gameIntegrationLiveLabel(game), 'Game data is stale');
  game.installed = false;
  assert.equal(gameIntegrationReady(game), false);
});

test('synthetic ACR telemetry can never present as native verification or native readiness', () => {
  const game = { id: 'assetto-corsa-rally', installed: true, integrationSetup: { state: 'ready', connection: { source: 'synthetic' }, verified: true, live: 'receiving' } };
  assert.equal(gameIntegrationNativeVerified(game), false);
  assert.equal(gameIntegrationReady(game), false);
  assert.equal(gameIntegrationLabel(game), 'Synthetic test telemetry configured');
  assert.equal(gameIntegrationVerificationLabel(game), 'Native telemetry not verified');
  assert.equal(gameIntegrationLiveLabel(game), 'Receiving synthetic test data');
  game.integrationSetup.live = 'stale';
  assert.equal(gameIntegrationLiveLabel(game), 'Synthetic test data is stale');
  game.integrationSetup.live = 'waiting';
  assert.equal(gameIntegrationLiveLabel(game), 'Waiting for synthetic test data');
  delete game.integrationSetup.connection;
  assert.equal(gameIntegrationNativeVerified(game), false);
  assert.equal(gameIntegrationLabel(game), 'Native telemetry not verified');
  const dcs = { id: 'dcs', installed: true, integrationSetup: { state: 'ready' } };
  assert.equal(gameIntegrationReady(dcs), true);
  assert.equal(gameIntegrationNativeVerified(dcs), false);
});

test('ACR setup needs neither a Saved Games folder nor a durable process ID', () => {
  const game = { id: 'assetto-corsa-rally', installed: true, integrationSetup: { state: 'setup-required' } };
  assert.equal(gameIntegrationTargetError(game, ''), '');
  assert.equal(gameIntegrationTargetError(game, 'relative-dcs-path'), '');
  assert.equal(gameIntegrationSetupBlockedReason(game, { phase: 'idle' }, false), '');
  assert.match(gameIntegrationSetupBlockedReason(game, { phase: 'running' }, false), /game profile/);
  assert.match(gameIntegrationSetupBlockedReason(game, { phase: 'idle' }, true), /active integration/);
  for (const command of ['game_integration_check', 'game_integration_setup', 'game_integration_cancel', 'game_integration_recover']) assert.deepEqual(plain(gameIntegrationSetupRequest(game, command, 'C:/Users/Pilot/Saved Games/DCS')), { command, gameId: 'assetto-corsa-rally' });
  const dcs = { id: 'dcs' };
  assert.ok(gameIntegrationTargetError(dcs, ''));
  assert.equal(gameIntegrationTargetError(dcs, 'C:/Users/Pilot/Saved Games/DCS'), '');
  assert.deepEqual(plain(gameIntegrationSetupRequest(dcs, 'game_integration_setup', '  C:/Users/Pilot/Saved Games/DCS  ')), { command: 'game_integration_setup', gameId: 'dcs', userFolder: 'C:/Users/Pilot/Saved Games/DCS' });
  assert.deepEqual(plain(gameIntegrationSetupRequest(dcs, 'game_integration_check', '')), { command: 'game_integration_check', gameId: 'dcs' });
});

test('ACR game profiles capture deferred native telemetry without inventing or requiring a process', () => {
  const value = plain(newGameProfile('assetto-corsa-rally'));
  value.integration = integration();
  delete value.integration.connection.processId;
  assert.equal(gameProfileError(value), '');
  assert.equal('processId' in value.integration.connection, false);
  const captured = cloneGameProfile(value);
  assert.equal(gameProfileError(captured), '');
  assert.equal('processId' in captured.integration.connection, false);
  captured.integration.connection.processId = null;
  assert.equal(gameProfileError(captured), '');
  captured.integration.connection.processId = 0;
  assert.match(gameProfileError(captured), /process ID/);
  assert.equal('processId' in value.integration.connection, false, 'The original settings remain deferred');
});
