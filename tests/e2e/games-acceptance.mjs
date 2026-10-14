// Games requests use the ordinary Rust manager and independently running
// production C USB/CAN fixtures. The authored executable is never launched.
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomBytes } from 'node:crypto';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { processClient } from './process-client.mjs';

const sleep = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));

test('Games applies and restores a real simulated peripheral without flash writes, guards manual changes, and persists profiles', { timeout: 120_000 }, async () => {
  assert.ok(process.env.OSG_E2E_MANIFEST, 'Run tests/e2e/run.py to provision external components first');
  const launch = JSON.parse(await readFile(process.env.OSG_E2E_MANIFEST, 'utf8'));
  await mkdir(launch.artifacts, { recursive: true });
  const directory = await mkdtemp(resolve(launch.artifacts, 'games-'));
  const env = {
    ...process.env,
    OSG_LOCAL_GATEWAYS_ONLY: '1',
    OSG_SIMULATION_NAMESPACE: `games-e2e-${randomBytes(8).toString('hex')}`,
    OSG_DCS_BIOS_CONFIG: resolve(directory, 'dcs.json'),
    OSG_GAMES_CONFIG: resolve(directory, 'games.json'),
  };
  const uid = randomBytes(12).toString('hex');
  const gatewayId = `stm32:${uid}`;
  const moduleId = randomBytes(16).toString('hex');
  const profileId = `dcs-acceptance-${randomBytes(4).toString('hex')}`;
  const executable = resolve(directory, 'DCS.exe');
  await writeFile(executable, 'Authored discovery fixture; game_activate never executes this file.\n');
  const definition = JSON.parse(await readFile(resolve(launch.deviceRepository, 'simulation/configs/pedals.json'), 'utf8'));
  definition.identity = moduleId;
  const definitionPath = resolve(directory, 'pedals.json');
  await writeFile(definitionPath, JSON.stringify(definition));
  const binary = process.env.OSG_E2E_SERVICE_BINARY || launch.serviceBinary;
  let service = processClient(binary, [], env);
  const peers = [];
  const request = (command, extra = {}) => service.request({
    command, clientId: 'cockpit-games-production-acceptance', gatewayId, ...extra,
  });
  const ok = async (command, extra = {}) => {
    const reply = await request(command, extra);
    assert.equal(reply.ok, true, `${command}: ${reply.error}\n${JSON.stringify(reply.state?.games?.session)}`);
    return reply.state;
  };
  async function wait(operation, accepts, label, limit = 30_000) {
    const deadline = Date.now() + limit;
    let value;
    do {
      value = await operation();
      if (accepts(value)) return value;
      await sleep(100);
    } while (Date.now() < deadline);
    assert.fail(`${label}: ${JSON.stringify(value)}`);
  }
  const module = state => state.modules.find(item => item.id === moduleId);
  const configuration = async () => (await ok('config_read', { moduleId })).moduleConfigs[moduleId];
  const games = async () => (await ok('game_get_state')).games;
  let baseline, desired, savedGeneration;
  try {
    const gateway = processClient(launch.gatewayBinary, [
      '--identity', uid, '--state-dir', resolve(directory, 'gateway'),
    ], env);
    peers.push(gateway);
    const device = processClient(launch.python, [
      resolve(launch.deviceRepository, 'simulation/run.py'), '--config', definitionPath,
      '--gateway', uid, '--state-dir', resolve(directory, 'device'),
      '--output-dir', launch.deviceBuild, '--no-build',
    ], env);
    peers.push(device);
    const ports = await wait(() => ok('ports'), state => state.ports.some(port => port.serialNumber === uid), 'gateway discovery');
    await ok('connect', { port: ports.ports.find(port => port.serialNumber === uid).path });
    await wait(() => ok('status'), state => !state.layoutPending && module(state)?.online
      && module(state).enabled && module(state).published && module(state).profileMatch,
    'published pedals with a settled route');
    const original = await configuration();
    assert.equal(original.pendingApply, false);
    baseline = structuredClone(original.config);
    desired = structuredClone(baseline);
    desired.name = 'DCS acceptance pedals';
    assert.ok(desired.axes.length > 0, 'The production pedals expose actual axis configuration');
    desired.axes[0][4] = 321;
    savedGeneration = original.savedGeneration;
    const previousIntegration = (await ok('integration_get_state')).integration;
    const previousDcs = (await ok('dcs_status')).dcsBios;
    assert.equal(previousIntegration.running, false);
    assert.equal(previousIntegration.configured, false);
    assert.equal(previousDcs.running, false);
    await ok('game_installation', { gameId: 'dcs', executable });
    const catalogue = (await games()).catalogue;
    const discovered = catalogue.find(game => game.id === 'dcs');
    assert.equal(discovered.installed, true);
    assert.equal(discovered.source, 'manual');
    assert.equal(discovered.executable, executable);
    if (process.platform !== 'win32') assert.equal(discovered.launchSupported, false);
    const gameProfile = {
      schemaVersion: 1, id: profileId, gameId: 'dcs', name: 'Acceptance flight setup',
      devices: [{ gatewayId, moduleId, controller: 'disabled', config: desired }], integration: null,
    };
    await ok('game_profile_save', { gameProfile });
    await ok('game_profile_default', { gameId: 'dcs', profileId });
    await ok('game_activate', { gameId: 'dcs', profileId, acceptBindingChanges: true });
    await wait(games, state => state.session.phase === 'running', 'native game profile activation');
    const active = await configuration();
    assert.deepEqual(active.config, desired, 'The actual device received the game configuration');
    assert.equal(active.savedGeneration, savedGeneration, 'Game activation does not write peripheral flash');
    assert.ok(active.generation > original.generation, 'The production runtime committed the temporary configuration');
    const disabled = await ok('status');
    assert.equal(module(disabled).enabled, false);
    assert.equal(module(disabled).published, false);
    assert.equal(module(disabled).reportId, null);
    const blocked = await request('config_apply', { moduleId, config: baseline });
    assert.equal(blocked.ok, false);
    assert.match(blocked.error, /game session owns/i);
    assert.deepEqual((await configuration()).config, desired, 'Rejected manual edits do not reach the hardware');
    await ok('game_deactivate');
    await wait(games, state => state.session.phase === 'idle', 'previous setup restoration');
    const restored = await configuration();
    assert.deepEqual(restored.config, baseline);
    assert.equal(restored.savedGeneration, savedGeneration, 'Recovery also leaves peripheral flash unchanged');
    const enabled = await wait(() => ok('status'), state => !state.layoutPending && module(state)?.online
      && module(state).enabled && module(state).published && module(state).profileMatch,
    'restored controller and HID descriptor');
    assert.equal(module(enabled).profileMatch, true);
    const integration = (await ok('integration_get_state')).integration;
    assert.equal(integration.configured, previousIntegration.configured);
    assert.equal(integration.running, previousIntegration.running);
    assert.deepEqual(integration.config, previousIntegration.config);
    assert.equal((await ok('dcs_status')).dcsBios.running, previousDcs.running);
    await service.stop();
    service = processClient(binary, [], env);
    const persisted = await games();
    assert.equal(persisted.session.phase, 'idle', 'Restart does not activate saved profiles');
    assert.equal(persisted.defaultProfiles.dcs, profileId);
    assert.deepEqual(persisted.profiles.find(profile => profile.id === profileId), gameProfile);
    assert.equal(persisted.catalogue.find(game => game.id === 'dcs').installed, true);
    assert.equal((await ok('integration_get_state')).integration.running, false);
    await writeFile(resolve(directory, 'result.json'), JSON.stringify({
      scope: 'software simulation', nativeGameCoordinator: true, gateways: 1, peripherals: 1,
      configApplied: true, controllerDisabled: true, manualEditGuard: true, restored: true,
      savedGeneration, flashUnchanged: true, persistedWithoutActivation: true,
      actualGameLaunched: false, hardware: false,
    }, null, 2));
  } finally {
    await service.stop();
    await Promise.all(peers.reverse().map(peer => peer.stop()));
  }
});
