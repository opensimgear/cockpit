// IPC-only acceptance against the running native Cockpit. The simulator process,
// BIOS frames and ownship envelopes are authored fixtures; no hardware outputs.
// All game-side writes, including Setup/Repair, stay in this private user folder.
import test from 'node:test';
import assert from 'node:assert/strict';
import {access, cp, mkdir, readFile, unlink, writeFile} from 'node:fs/promises';
import {dirname, resolve, sep} from 'node:path';
import {execFile, spawn} from 'node:child_process';
import {promisify} from 'node:util';
import {createSocket} from 'node:dgram';
import {createHash, randomUUID} from 'node:crypto';

const sleep = ms => new Promise(done => setTimeout(done, ms));
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const nodeId = id => `value-${id.replace(/[^a-zA-Z0-9._-]/g, '-')}`;
const near = (actual, expected) => typeof expected === 'number'
  ? typeof actual === 'number' && Math.abs(actual - expected) < 1e-7
  : actual === expected;

async function connect() {
  const targets = await (await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9224'}/json/list`)).json();
  const target = targets.find(t => t.type === 'page' && /127\.0\.0\.1:1420|tauri\.localhost/.test(t.url) && !/activation/i.test(t.url));
  assert.ok(target, 'Start the rebuilt native dev Cockpit.');
  const socket = new WebSocket(target.webSocketDebuggerUrl), pending = new Map();
  let serial = 0;
  await new Promise((done, reject) => {
    socket.addEventListener('open', done, {once: true});
    socket.addEventListener('error', reject, {once: true});
  });
  socket.addEventListener('message', event => {
    const value = JSON.parse(event.data), waiter = pending.get(value.id);
    if (!waiter) return;
    clearTimeout(waiter.timer); pending.delete(value.id);
    value.error ? waiter.reject(Error(value.error.message)) : waiter.done(value.result);
  });
  return {
    async evaluate(expression) {
      const reply = await new Promise((done, reject) => {
        const id = ++serial;
        const timer = setTimeout(() => { pending.delete(id); reject(Error('Native IPC timed out.')); }, 15000);
        pending.set(id, {done, reject, timer});
        socket.send(JSON.stringify({id, method: 'Runtime.evaluate', params: {expression, awaitPromise: true, returnByValue: true}}));
      });
      if (reply.exceptionDetails) throw Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text);
      return reply.result.value;
    },
    close() {
      for (const item of pending.values()) { clearTimeout(item.timer); item.reject(Error('CDP connection closed.')); }
      pending.clear(); socket.close();
    },
  };
}

function block(address, bytes) {
  const header = Buffer.alloc(4); header.writeUInt16LE(address); header.writeUInt16LE(bytes.length, 2);
  return Buffer.concat([header, bytes]);
}
function integerBlock(output, value) {
  const bytes = Buffer.alloc(2); bytes.writeUInt16LE((value << output.shift_by) & output.mask);
  return block(output.address, bytes);
}
function control(document, id) {
  return Object.values(document).flatMap(category => Object.values(category)).find(item => item.identifier === id);
}
function identity(id, contract) {
  return {id, name: id, operation: {kind: 'identity'},
    inputs: [{id: 'in', name: 'Input', direction: 'input', contract}],
    outputs: [{id: 'out', name: 'Output', direction: 'output', contract}]};
}
async function fixtureExecutable(directory) {
  const executable = resolve(directory, 'installation/bin/DCS.exe'); await mkdir(dirname(executable), {recursive: true});
  await promisify(execFile)('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
    "$ErrorActionPreference='Stop'; Add-Type -TypeDefinition 'public static class DcsPhysicsFixture { public static void Main() { System.Threading.Thread.Sleep(System.Threading.Timeout.Infinite); } }' -OutputAssembly $env:OSG_DCS_PHYSICS_FIXTURE -OutputType WindowsApplication"],
  {windowsHide: true, env: {...process.env, OSG_DCS_PHYSICS_FIXTURE: executable}});
  return executable;
}

test('native managed DCS ownship physics and BIOS remain independent through withdrawal and rearm', {
  skip: process.env.OSG_DCS_PHYSICS_NATIVE !== '1', timeout: 180000,
}, async () => {
  assert.equal(process.platform, 'win32');
  const directory = resolve('.research/dcs-physics-native', String(Date.now()));
  const userFolder = resolve(directory, 'private-user'), script = resolve(userFolder, 'Scripts/OpenSimGear/DCSPhysics.lua');
  await mkdir(resolve(userFolder, 'Scripts'), {recursive: true});
  const cdp = await connect(), checks = [], cleanupErrors = [];
  const evidence = {actualNativeCockpit: true, ipcOnly: true, noPhysicalOutputs: true,
    authoredTelemetry: true, fixtureProcess: true, actualSimulatorQualified: false,
    directory, userFolder, startedAt: new Date().toISOString(), checks, cleanupErrors, phases: {}};
  const request = async value => {
    const reply = await cdp.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value, clientId: 'dcs-physics-native'})}})`);
    assert.equal(reply.ok, true, `${value.command}: ${reply.error}`); return reply.state;
  };
  const games = async () => (await request({command: 'game_get_state'})).games;
  const game = async () => (await games()).catalogue.find(item => item.id === 'dcs');
  const status = async () => (await request({command: 'profile_status'})).profiles;
  const inspect = async search => (await request({command: 'game_telemetry_inspect', gameId: 'dcs', offset: 0, limit: 128, search})).gameTelemetry.games.find(item => item.id === 'dcs');
  async function until(read, label, timeout = 8500) {
    const end = Date.now() + timeout; let last;
    while (Date.now() < end) { try { last = await read(); if (last) return last; } catch (error) { last = String(error); } await sleep(100); }
    throw Error(`${label}: ${JSON.stringify(last)}`);
  }
  async function checkpoint(phase) {
    evidence.lastPhase = phase; console.log(`DCS physics: ${phase}`);
    await writeFile(resolve(directory, 'progress.json'), JSON.stringify(evidence, null, 2) + '\n');
  }
  async function passive(id, expected) {
    return until(async () => {
      const report = await inspect(id), row = report.channels.find(row => row.id === id);
      return row?.status === 'known' && near(row.value, expected) && {row, context: report.context, source: report.source};
    }, `Passive ${id} did not report ${JSON.stringify(expected)}`);
  }
  async function absent(id) {
    return until(async () => {
      const report = await inspect(id), row = report.channels.find(row => row.id === id);
      return row && row.status !== 'known' && row.value === null && {row, context: report.context};
    }, `Passive ${id} retained a known value`);
  }
  let initialGames, initialProfiles, before, installationBefore, profile, child, producer, timer, fixtureSetup = false;
  let counter = 0, sequence = 0, modelTime = 0, planeId = 17, mode = 'baseline', emitPhysics = true;
  const aircraft = 'A-10C_2', biosId = 'common:CommonData:ALT_MSL_FT:0';
  const nativeValues = {'flight.airspeed.true': 210.25, 'flight.engine.left.speedPercent': 74.5,
    'flight.acceleration.x': 3, 'flight.acceleration.y': 4, 'flight.acceleration.z': 0,
    'flight.ownship.inAir': false, 'flight.wind.world.z': -9};
  const selected = {'flight.airspeed.true': 210.25, 'flight.engine.left.speedPercent': 74.5,
    'engine.rpm_percent': 74.5, 'vehicle.g_force.magnitude': 5, 'flight.ownship.inAir': false, [biosId]: 1000};
  const physicsIds = Object.keys(selected).filter(id => id !== biosId);
  const cleanup = async (label, work) => { try { await work(); } catch (error) { cleanupErrors.push(`${label}: ${error}`); } };
  async function samples() {
    const current = (await status()).profileSession;
    if (current.owner?.profileId !== profile?.id) return null;
    return (await request({command: 'profile_runtime_inspect', owner: current.owner, offset: 0, limit: 64})).profileInspection;
  }
  const output = (page, id) => page?.ports.find(row => row.endpoint.nodeId === nodeId(id) && row.endpoint.portId === 'out')?.sample;
  async function live() {
    return until(async () => { const page = await samples(); return page && Object.entries(selected).every(([id, value]) => output(page, id)?.status === 'known' && near(output(page, id).value.value, value)) && page; }, 'Selected physics, canonical and BIOS ports did not reach processing');
  }
  async function withdrawn(label) {
    const page = await until(async () => { const page = await samples(); return page && physicsIds.every(id => output(page, id)?.status !== 'known') && output(page, biosId)?.status === 'known' && near(output(page, biosId).value.value, 1000) && page; }, `${label}: physics retained values or BIOS was withdrawn`);
    const missing = await absent('engine.rpm_percent');
    await passive(biosId, 1000); await passive('AAP_CDUPWR:0', 1);
    return {page, missing};
  }
  async function setup() {
    await request({command: 'game_integration_setup', gameId: 'dcs', userFolder}); fixtureSetup = true;
    const value = await until(async () => { const value = await game(); return value.integrationSetup.state !== 'setting-up' && value; }, 'Managed setup did not finish', 25000);
    assert.equal(value.integrationSetup.state, 'ready', JSON.stringify(value.integrationSetup));
    assert.equal(value.integrationSetup.physicsExportsReady, true);
    assert.equal(value.integrationSetup.physicsPort, 7781);
    const bytes = await readFile(script), catalogueBytes = await readFile(resolve('src-tauri/src/dcs_physics_channels.json'));
    assert.equal(value.integrationSetup.physicsExportRevision, digest(bytes));
    assert.equal(value.integrationSetup.physicsCatalogueRevision, digest(catalogueBytes));
    assert.ok(value.integrationSetup.physicsChannelCount > 40);
    const exportText = await readFile(resolve(userFolder, 'Scripts/Export.lua'), 'utf8');
    assert.equal(exportText.split('-- OpenSimGear ownship physics begin').length - 1, 1);
    assert.ok(exportText.startsWith('-- private native physics acceptance\n'));
    return {setup: value.integrationSetup, fileDigest: digest(bytes)};
  }
  try {
    initialProfiles = await status(); initialGames = await games(); before = initialGames.catalogue.find(item => item.id === 'dcs').integrationSetup;
    installationBefore = initialGames.manualInstallations?.dcs ?? null;
    assert.ok(!initialGames.detectedGames?.some(item => item.gameId === 'dcs'), 'Close actual DCS before starting this private acceptance fixture.');
    assert.ok((before.connection?.cataloguePath || before.cataloguePath) && before.userFolder, 'An existing validated DCS-BIOS installation is required as a read-only copy source.');
    before.connection ??= {cataloguePath: before.cataloguePath, freshnessMs: 1500,
      commandHost: '127.0.0.1', commandPort: 7778, listenAddress: '0.0.0.0', listenPort: 5010,
      multicastGroup: '239.255.50.10', multicastInterface: process.env.OSG_DCS_ORIGINAL_MULTICAST_INTERFACE ?? '0.0.0.0'};
    evidence.before = {setup: before, installation: installationBefore};
    if (['faulted', 'recovery-needed'].includes(initialProfiles.profileSession.state)) await request({command: 'profile_recover'});
    await request({command: 'profile_selection_mode', mode: 'manual'}); await request({command: 'profile_stop'});
    const biosSource = resolve(before.connection.cataloguePath, '../..'), biosTarget = resolve(userFolder, 'Scripts/DCS-BIOS');
    assert.ok(biosTarget.startsWith(directory + sep));
    await cp(biosSource, biosTarget, {recursive: true, errorOnExist: true, force: false});
    await writeFile(resolve(userFolder, 'Scripts/Export.lua'), "-- private native physics acceptance\npackage.path = package.path .. ';' .. lfs.writedir() .. 'Scripts/?.lua'\ndofile(lfs.writedir() .. [[Scripts\\DCS-BIOS\\BIOS.lua]])\n");
    const executable = await fixtureExecutable(directory);
    await request({command: 'game_installation', gameId: 'dcs', executable});
    const connection = {...before.connection, cataloguePath: resolve(biosTarget, 'doc/json'), freshnessMs: 600,
      commandHost: '127.0.0.1', commandPort: 7778, listenAddress: '0.0.0.0', listenPort: 5010,
      multicastGroup: '239.255.50.10', multicastInterface: '127.0.0.1'};
    await request({command: 'game_integration_settings_save', gameId: 'dcs', expectedSetupRevision: (await game()).integrationSetup.providerSetupRevision, connection, userFolder, aircraftModule: 'A-10C'});
    assert.notEqual((await game()).integrationSetup.providerSetupRevision, before.providerSetupRevision);
    evidence.setup = await setup(); await checkpoint('managed setup and file revisions verified');
    assert.ok(script.startsWith(directory + sep)); await unlink(script);
    await request({command: 'game_integration_check', gameId: 'dcs', userFolder});
    const missing = await game(); assert.notEqual(missing.integrationSetup.state, 'ready'); assert.equal(missing.integrationSetup.physicsExportsReady, false);
    evidence.repair = await setup(); assert.equal(evidence.repair.fileDigest, evidence.setup.fileDigest);
    checks.push('Setup and Repair write only the private fixture folder; managed source/catalogue revisions match generated bytes and the loader is unique.');
    await checkpoint('missing managed exporter repaired');
    const json = async file => JSON.parse(await readFile(resolve(biosTarget, 'doc/json', file), 'utf8'));
    const commonOutput = control(await json('CommonData.json'), 'ALT_MSL_FT').outputs[0];
    const aircraftOutput = control(await json('A-10C.json'), 'AAP_CDUPWR').outputs.find(row => row.type === 'integer');
    child = spawn(executable, [], {cwd: dirname(executable), windowsHide: true, stdio: 'ignore'});
    await new Promise((done, reject) => { child.once('spawn', done); child.once('error', reject); });
    producer = createSocket('udp4'); await new Promise(done => producer.bind(0, done));
    producer.on('error', error => { evidence.udpError = String(error); });
    producer.setMulticastInterface('127.0.0.1'); producer.setMulticastLoopback(true);
    timer = setInterval(() => {
      const name = Buffer.alloc(24); name.write(aircraft);
      producer.send(Buffer.concat([Buffer.from([85, 85, 85, 85]), block(0, name), integerBlock(commonOutput, 1000), integerBlock(aircraftOutput, 1), block(65534, Buffer.from([counter++ % 256, 0]))]), 5010, '239.255.50.10');
      if (!emitPhysics) return;
      if (!['baseline', 'frozen', 'respawn-wait'].includes(mode)) modelTime += 0.05;
      const packet = {schemaVersion: 1, source: 'osg-dcs-ownship', session: 'native-fixture-mission', sequence: sequence++ >>> 0,
        modelTime, aircraft, planeId, allowed: mode !== 'denied', paused: mode === 'paused', activePause: mode === 'active-pause', stopped: mode === 'stopped', values: nativeValues};
      producer.send(Buffer.from(JSON.stringify(packet)), 7781, '127.0.0.1');
    }, 40);
    await until(async () => (await game()).currentVehicle?.id === aircraft, 'Verified fixture BIOS identity was not discovered');
    evidence.baseline = await absent('flight.airspeed.true');
    mode = 'live'; evidence.independent = await passive('flight.airspeed.true', 210.25);
    await passive('flight.acceleration.z', 0);
    const unbound = await passive('flight.wind.world.z', -9); assert.equal(unbound.row.unit, 'm/s'); assert.ok(unbound.row.nativeField.includes('LoGetVectorWindVelocity'));
    evidence.canonical = await passive('engine.rpm_percent', 74.5); assert.equal(evidence.canonical.row.unit, '%');
    assert.ok(!(await inspect('engine.rpm')).channels.some(row => row.id === 'engine.rpm'), 'Native engine percentage must not claim absolute RPM.');
    evidence.magnitude = await passive('vehicle.g_force.magnitude', 5);
    await passive('AAP_CDUPWR:0', 1); await checkpoint('independent native, canonical and unbound BIOS inspection live');
    const setupRevision = (await game()).integrationSetup.providerSetupRevision;
    const resource = await until(async () => {
      const resources = []; let total = 1;
      while (resources.length < total) {
        const page = (await request({command: 'profile_resources_list', offset: resources.length, limit: 128})).profileResources;
        total = page.total; if (!page.resources.length) break; resources.push(...page.resources);
      }
      evidence.resourceCandidates = resources.filter(item => item.attributes.gameId === 'dcs');
      const found = evidence.resourceCandidates.find(item => item.attributes.aircraftModule === 'CommonData' && item.connections.some(connection => connection.scope === 'game:dcs' && connection.route.attributes.providerSetupRevision === setupRevision));
      if (!found) await writeFile(resolve(directory, 'resource-candidates.json'), JSON.stringify({setupRevision, resources: evidence.resourceCandidates}, null, 2));
      return found;
    }, 'Current base DCS resource not found');
    const metadataRoute = resource.connections.find(connection => connection.scope === 'game:dcs').route;
    assert.equal(metadataRoute.attributes.providerSetupRevision, setupRevision);
    assert.match(metadataRoute.attributes.catalogueRevision, /^sha256:[0-9a-f]{64}$/);
    const ports = []; let total = 1;
    while (ports.length < total) { const page = (await request({command: 'profile_resources_inspect', resourceId: resource.id, offset: ports.length, limit: 128})).profileResources; total = page.total; assert.ok(page.ports.length || ports.length === total); ports.push(...page.ports); }
    const chosen = Object.keys(selected).map(id => { const port = ports.find(row => row.descriptor.id === `channel:${id}`); assert.ok(port, `Missing captured ${id}`); return port; });
    const document = (await request({command: 'profile_create', name: `DCS ownship acceptance ${randomUUID()}`})).profiles.document;
    profile = {id: document.id, revision: document.revision}; document.eligibility = {gameIds: ['dcs']};
    document.resources.push({id: 'telemetry', name: 'Private DCS telemetry', kind: 'game', required: true, ports: chosen.map(row => ({...row.descriptor, id: row.descriptor.id.slice(8)}))});
    for (const row of chosen) { const id = row.descriptor.id.slice(8), node = nodeId(id); document.processingNodes.push(identity(node, row.descriptor.contract)); document.bindings.push({id: `route-${node}`, from: {nodeId: 'telemetry', portId: id}, to: {nodeId: node, portId: 'in'}}); }
    const assignments = [{resourceId: 'telemetry', sourceId: resource.id, ports: chosen.map(row => ({profilePortId: row.descriptor.id.slice(8), observedPortId: row.descriptor.id, expectedContractHash: row.contractHash}))}];
    const saved = (await request({command: 'profile_editor_save', document, assignments, expectedRevision: document.revision})).profiles.document; profile.revision = saved.revision;
    evidence.captured = {resource, ports: chosen, profile};
    await request({command: 'profile_apply', id: profile.id, expectedRevision: profile.revision, gameId: 'dcs', expectedActiveOwner: null});
    evidence.phases.live = await live(); await checkpoint('selected physics and BIOS reach identity processing');
    for (const phase of ['paused', 'active-pause', 'denied', 'frozen']) {
      mode = phase; evidence.phases[phase] = await withdrawn(phase);
      mode = 'live'; await live(); await checkpoint(`${phase} withdrawal and rearm verified`);
    }
    emitPhysics = false; evidence.phases.stale = await withdrawn('stale ownship');
    emitPhysics = true; await live(); await checkpoint('stale ownship withdraws physics while BIOS gauges remain live');
    planeId++; modelTime = 0; mode = 'respawn-wait'; evidence.phases.respawn = await withdrawn('respawn baseline');
    mode = 'live'; await live(); await checkpoint('respawn requires advancing ownship baseline');
    mode = 'stopped'; evidence.phases.stopped = await withdrawn('mission stopped');
    mode = 'live'; await live();
    child.kill(); await until(() => child.exitCode !== null || child.signalCode !== null, 'Fixture process did not exit');
    evidence.phases.exit = await absent('flight.airspeed.true'); await absent(biosId);
    await until(async () => { const page = await samples(); return !page || Object.keys(selected).every(id => output(page, id)?.status !== 'known'); }, 'Process exit retained processing values');
    checks.push('Independent inspection and selected native/canonical profile values preserve zero/false and units; unbound wind and BIOS aircraft gauge remain inspectable.');
    checks.push('Pause, active pause, export denial, frozen model time, stale ownship, respawn baseline and mission stop withdraw physics without withdrawing unrelated fresh BIOS gauges; process exit withdraws both.');
    await checkpoint('process exit withdrawal verified');
  } catch (error) {
    evidence.failure = String(error); evidence.failureGame = await game().catch(() => null); evidence.failureProfiles = await status().catch(() => null); throw error;
  } finally {
    clearInterval(timer); if (producer) producer.close();
    await cleanup('Stop profile', () => request({command: 'profile_stop'}));
    if (child && child.exitCode === null && child.signalCode === null) { child.kill(); await sleep(200); }
    if (profile) await cleanup('Delete temporary profile', () => request({command: 'profile_delete', id: profile.id, expectedRevision: profile.revision}));
    if (fixtureSetup) await cleanup('Remove private managed setup', async () => {
      await request({command: 'game_integration_remove', gameId: 'dcs', userFolder});
      await until(async () => (await game()).integrationSetup.state !== 'setting-up', 'Private setup removal did not finish', 15000);
      await assert.rejects(access(script));
    });
    if (installationBefore !== undefined) await cleanup('Restore selected installation', () => request({command: 'game_installation', gameId: 'dcs', executable: installationBefore}));
    if (before) await cleanup('Restore provider settings', async () => {
      await request({command: 'game_integration_settings_save', gameId: 'dcs', expectedSetupRevision: (await game()).integrationSetup.providerSetupRevision, connection: before.connection, userFolder: before.userFolder, aircraftModule: before.aircraftModule ?? null});
      await request({command: 'game_integration_check', gameId: 'dcs'});
      assert.deepEqual((await game()).integrationSetup.connection, before.connection);
    });
    if (initialProfiles?.session.selectedProfileId) await cleanup('Restore selected profile', () => request({command: 'profile_select', id: initialProfiles.session.selectedProfileId, selectionSource: 'manual'}));
    if (initialProfiles?.session.selectionMode) await cleanup('Restore selection mode', () => request({command: 'profile_selection_mode', mode: initialProfiles.session.selectionMode}));
    await cleanup('Leave profile stopped', () => request({command: 'profile_stop'}));
    evidence.finishedAt = new Date().toISOString();
    await writeFile(resolve(directory, 'evidence.json'), JSON.stringify(evidence, null, 2) + '\n'); cdp.close();
    console.log(JSON.stringify({directory, checks, cleanupErrors})); assert.deepEqual(cleanupErrors, []);
  }
});
