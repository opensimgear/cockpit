// Desktop and browser-dev acceptance with authored simulator process and telemetry.
// OSG_DCS_COMBINED_VJOY_PROBE adds a browser panel and a temporary vJoy slot.
import test from 'node:test';
import assert from 'node:assert/strict';
import {access, cp, mkdir, readFile, rm, rmdir, unlink, writeFile} from 'node:fs/promises';
import {dirname, resolve, sep} from 'node:path';
import {execFile, spawn} from 'node:child_process';
import {promisify} from 'node:util';
import {createSocket} from 'node:dgram';
import {createHash, randomUUID} from 'node:crypto';
import {browserPage, nativePage} from '../../scripts/reactive/cdp.mjs';
import {combinedDcsFixture, combinedTimingMisses} from './dcs-companion-vjoy-fixture.mjs';
import {soakSeconds} from './dcs-feature-soak.mjs';
import {qualifyDcsTelemetryReopen} from './dcs-telemetry-reopen.mjs';
import {openDcsTelemetryReaders} from './dcs-telemetry-backpressure.mjs';
import {browserGateway} from './vjoy-rate-transport.mjs';
import {dcsDesktopWakes} from './dcs-desktop-wakes.mjs';
const browserUrl = process.env.OSG_DCS_PHYSICS_BROWSER_URL;
const observeMcp = process.env.OSG_DCS_COMBINED_MCP === '1';
const nativeRates = process.env.OSG_DCS_COMBINED_NATIVE_RATES === '1';
const seconds = soakSeconds(process.env.OSG_DCS_COMBINED_SOAK_SECONDS);
const sleep = ms => new Promise(done => setTimeout(done, ms));
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const normalizedPath = value => value.replace(/^\\\\\?\\/, '').replaceAll('/', '\\').toLowerCase().replace(/\\+$/, '');
const nodeId = id => `value-${id.replace(/[^a-zA-Z0-9._-]/g, '-')}`;
const near = (actual, expected) => typeof expected === 'number'
  ? typeof actual === 'number' && Math.abs(actual - expected) < 1e-7
  : actual === expected;
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
test(`${browserUrl ? 'browser-dev' : 'native'} managed DCS ownship physics and BIOS remain independent through withdrawal and rearm`, {
  skip: !browserUrl && process.env.OSG_DCS_PHYSICS_NATIVE !== '1', timeout: 300000 + seconds * 1000,
}, async () => {
  assert.equal(process.platform, 'win32');
  const combinedProbe = process.env.OSG_DCS_COMBINED_VJOY_PROBE;
  if (observeMcp) assert.ok(!browserUrl && combinedProbe, 'Combined MCP requires desktop Cockpit and a vJoy probe');
  if (nativeRates) assert.ok(!browserUrl && observeMcp && combinedProbe, 'Native rates require desktop Cockpit, combined MCP and a vJoy probe');
  const nativeCommands = process.env.OSG_DCS_COMMANDS_NATIVE === '1';
  if (nativeCommands) assert.ok(nativeRates, 'The DCS command fixture requires native rates');
  if (seconds) assert.ok(observeMcp && !nativeRates, 'Soak requires combined desktop MCP with the debug and panel input profile');
  const directory = resolve('.research/dcs-physics-native', String(Date.now()));
  const userFolder = resolve(directory, 'private-user'), script = resolve(userFolder, 'Scripts/OpenSimGear/DCSPhysics.lua');
  const biosTarget = resolve(userFolder, 'Scripts/DCS-BIOS');
  await mkdir(resolve(userFolder, 'Scripts'), {recursive: true});
  const browserGatewayClient = browserUrl ? browserGateway(new URL(browserUrl)) : null;
  const cdp = browserUrl ? await browserPage(browserUrl) : await nativePage(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9224');
  const checks = [], cleanupErrors = [];
  const evidence = {transport: browserUrl ? 'browser-dev' : 'native', actualNativeCockpit: !browserUrl,
    actualBrowserCockpit: !!browserUrl, ipcOnly: !browserUrl, noPhysicalOutputs: true,
    authoredTelemetry: true, fixtureProcess: true, actualSimulatorQualified: false,
    directory, userFolder, startedAt: new Date().toISOString(), checks, cleanupErrors, phases: {}};
  const request = async value => {
    const reply = browserUrl
      ? await browserGatewayClient.request({...value, clientId: 'dcs-physics-native'})
      : await cdp.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value, clientId: 'dcs-physics-native'})}})`);
    assert.equal(reply.ok, true, `${value.command}: ${reply.error}`); return reply.state;
  };
  const combined = combinedProbe ? combinedDcsFixture(cdp, request, combinedProbe, {browserUrl, observeMcp, nativeRates, seconds}) : null;
  const desktopMode = {'1': 'desktop-wide', 'selected-game': 'selected-game'}[process.env.OSG_DCS_DESKTOP_WAKES];
  const desktop = !browserUrl && desktopMode ? dcsDesktopWakes(request, until, desktopMode) : null;
  if (combined) evidence.ipcOnly = false;
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
  let initialGames, initialProfiles, before, installationBefore, fixtureInstallation, profile, child, producer, timer, fixtureSetup = false;
  let counter = 0, sequence = 0, modelTime = 0, planeId = 17, mode = 'baseline', emitPhysics = true, biosMode = 'full';
  let nextOwnshipSubmission;
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
    const originalDcs = initialGames.catalogue.find(item => item.id === 'dcs');
    installationBefore = originalDcs.selectedInstallationId;
    assert.ok(installationBefore, 'Select an existing DCS installation before starting this private acceptance fixture.');
    assert.ok(!initialGames.detectedGames?.some(item => item.gameId === 'dcs'), 'Close actual DCS before starting this private acceptance fixture.');
    assert.ok((before.connection?.cataloguePath || before.cataloguePath) && before.userFolder, 'An existing validated DCS-BIOS installation is required as a read-only copy source.');
    before.connection ??= {cataloguePath: before.cataloguePath, freshnessMs: 1500,
      commandHost: '127.0.0.1', commandPort: 7778, listenAddress: '0.0.0.0', listenPort: 5010,
      multicastGroup: '239.255.50.10', multicastInterface: process.env.OSG_DCS_ORIGINAL_MULTICAST_INTERFACE ?? '0.0.0.0'};
    evidence.before = {setup: before, installation: installationBefore};
    if (['faulted', 'recovery-needed'].includes(initialProfiles.profileSession.state)) await request({command: 'profile_recover'});
    await request({command: 'profile_selection_mode', mode: 'manual'}); await request({command: 'profile_stop'});
    const biosSource = resolve(before.connection.cataloguePath, '../..');
    assert.ok(biosTarget.startsWith(directory + sep));
    const documentationAssets = resolve(biosSource, 'doc/doc_assets');
    await cp(biosSource, biosTarget, {
      recursive: true, errorOnExist: true, force: false,
      filter: source => source !== documentationAssets,
    });
    await writeFile(resolve(userFolder, 'Scripts/Export.lua'), "-- private native physics acceptance\npackage.path = package.path .. ';' .. lfs.writedir() .. 'Scripts/?.lua'\ndofile(lfs.writedir() .. [[Scripts\\DCS-BIOS\\BIOS.lua]])\n");
    const executable = await fixtureExecutable(directory);
    const registered = (await request({command: 'game_add', name: originalDcs.name, installPath: resolve(directory, 'installation'), executable})).games;
    assert.equal(registered.addedGameId, 'dcs');
    fixtureInstallation = registered.catalogue.find(item => item.id === 'dcs').installations.find(item => normalizedPath(item.installPath) === normalizedPath(resolve(directory, 'installation')));
    assert.ok(fixtureInstallation, 'The private DCS installation was not registered.');
    assert.ok(!originalDcs.installations.some(item => item.id === fixtureInstallation.id));
    evidence.fixtureInstallation = fixtureInstallation;
    await request({command: 'game_installation_select', gameId: 'dcs', installationId: fixtureInstallation.id});
    assert.equal((await game()).selectedInstallationId, fixtureInstallation.id);
    const connection = {...before.connection, cataloguePath: resolve(biosTarget, 'doc/json'), freshnessMs: 600,
      commandHost: '127.0.0.1', commandPort: await combined?.openCommands() ?? 7778, listenAddress: '0.0.0.0', listenPort: 5010,
      multicastGroup: '239.255.50.10', multicastInterface: '127.0.0.1'};
    await request({command: 'game_integration_settings_save', gameId: 'dcs', expectedSetupRevision: (await game()).integrationSetup.providerSetupRevision, connection, userFolder, aircraftModule: 'A-10C'});
    assert.notEqual((await game()).integrationSetup.providerSetupRevision, before.providerSetupRevision);
    evidence.setup = await setup(); await checkpoint('managed setup and file revisions verified');
    if (process.env.OSG_DCS_SAVED_GAMES_WATCH_ROOT) {
      const root = resolve(process.env.OSG_DCS_SAVED_GAMES_WATCH_ROOT);
      const profileFolder = resolve(root, 'DCS.osg-watch-' + randomUUID());
      const hasFolder = async () => (await game()).integrationSetup.candidates
        ?.some(path => normalizedPath(path) === normalizedPath(profileFolder));
      await mkdir(profileFolder);
      try {
        await until(hasFolder, 'New DCS Saved Games profile was not observed');
      } finally {
        await rmdir(profileFolder);
      }
      await until(async () => !(await hasFolder()), 'Removed DCS Saved Games profile stayed visible');
      evidence.savedGamesWatch = {creation: true, removal: true};
      await checkpoint('private Saved Games profile creation and removal verified');
    }
    assert.ok(script.startsWith(directory + sep)); await unlink(script);
    const missing = await until(async () => {
      const value = await game();
      return value.integrationSetup.state !== 'ready' && value.integrationSetup.physicsExportsReady === false && value;
    }, 'Removing the private DCS exporter did not withdraw setup readiness');
    evidence.fileWithdrawal = missing.integrationSetup;
    evidence.repair = await setup(); assert.equal(evidence.repair.fileDigest, evidence.setup.fileDigest);
    checks.push('Setup and Repair write only the private fixture folder; managed source/catalogue revisions match generated bytes and the loader is unique.');
    await checkpoint('missing managed exporter repaired');
    await request({command: 'game_integration_recover', gameId: 'dcs'});
    const recoveredSetup = (await game()).integrationSetup;
    assert.equal(recoveredSetup.state, 'ready');
    assert.equal(recoveredSetup.recoveryNeeded, false);
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
      if (biosMode !== 'off') {
        const fields = biosMode === 'full' ? [block(0, name), integerBlock(commonOutput, 1000), integerBlock(aircraftOutput, 1)] : [];
        const tick = Buffer.alloc(2); tick.writeUInt16LE(counter++ % 65536);
        producer.send(Buffer.concat([Buffer.from([85, 85, 85, 85]), ...fields, block(65534, tick)]), 5010, '239.255.50.10');
      }
      if (!emitPhysics) return;
      if (!['baseline', 'frozen', 'respawn-wait'].includes(mode)) modelTime += 0.05;
      const packet = {schemaVersion: 1, source: 'osg-dcs-ownship', session: 'native-fixture-mission', sequence: sequence++ >>> 0,
        modelTime, aircraft, planeId, allowed: mode !== 'denied', paused: mode === 'paused', activePause: mode === 'active-pause', stopped: mode === 'stopped', values: nativeValues};
      const bytes = Buffer.from(JSON.stringify(packet));
      const submittedMs = nextOwnshipSubmission ? Number(process.hrtime.bigint()) / 1e6 : undefined;
      producer.send(bytes, 7781, '127.0.0.1');
      nextOwnshipSubmission?.({packetSequence: packet.sequence, submittedMs});
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
    evidence.phases.telemetryReopen = await qualifyDcsTelemetryReopen(cdp, browserUrl, nativeValues, passive);
    await passive(biosId, 1000);
    const competitor = spawn(executable, [], {cwd: dirname(executable), windowsHide: true, stdio: 'ignore'});
    try {
      await new Promise((done, reject) => { competitor.once('spawn', done); competitor.once('error', reject); });
      await until(async () => /Multiple game processes/.test((await game()).integrationContext?.message ?? ''),
        'A competing fixture process did not make the context ambiguous');
      await absent('flight.airspeed.true');
      await absent(biosId);
      assert.equal(child.exitCode, null);
    } finally {
      if (competitor.exitCode === null && competitor.signalCode === null) {
        competitor.kill();
        await until(() => competitor.exitCode !== null || competitor.signalCode !== null, 'Competing fixture process did not exit');
      }
    }
    evidence.phases.competingProcess = {
      withdrawal: true,
      resumed: await passive('flight.airspeed.true', 210.25),
    };
    await passive('AAP_CDUPWR:0', 1);
    await checkpoint('competing process withdraws passive telemetry and sole process recovers');
    await cdp.evaluate(`import('/src/lib/streams/shared.ts').then(({SharedObservations})=>{
      const observations=new SharedObservations(), activity={frames:0,close:()=>observations.close()};
      window.__dcsTelemetryWake=activity;
      observations.subscribe('games.telemetry',state=>{if(state.status==='ready')activity.frames++;});
    })`);
    try {
      const frames = () => cdp.evaluate('window.__dcsTelemetryWake.frames');
      await until(async () => (await frames()) > 0, 'Telemetry subscription did not get a baseline');
      const before = await frames();
      await sleep(1250);
      evidence.phases.inspectorNotifications = await frames() - before;
      assert.ok(evidence.phases.inspectorNotifications >= 16,
        `Expected DCS source wakes beyond the maintenance cadence, got ${evidence.phases.inspectorNotifications}`);
    } finally {
      await cdp.evaluate('window.__dcsTelemetryWake.close(); delete window.__dcsTelemetryWake');
    }
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
    await combined?.prepare(document, assignments, aircraft);
    await desktop?.prepare(document);
    const saved = (await request({command: 'profile_editor_save', document, assignments, expectedRevision: document.revision})).profiles.document; profile.revision = saved.revision;
    await combined?.bind(profile);
    evidence.captured = {resource, ports: chosen, profile};
    await request({command: 'profile_apply', id: profile.id, expectedRevision: profile.revision, gameId: 'dcs', expectedActiveOwner: null});
    evidence.phases.live = await live(); await checkpoint('selected physics and BIOS reach identity processing');
    if (desktop) { evidence.phases.desktopWakes = await desktop.verify(child.pid); await live(); }
    for (const phase of ['paused', 'active-pause', 'denied', 'frozen']) {
      mode = phase; evidence.phases[phase] = await withdrawn(phase);
      mode = 'live'; await live(); await checkpoint(`${phase} withdrawal and rearm verified`);
    }
    emitPhysics = false; evidence.phases.stale = await withdrawn('stale ownship');
    emitPhysics = true; await live(); await checkpoint('stale ownship withdraws physics while BIOS gauges remain live');
    biosMode = 'counter-only';
    const owner = (await status()).profileSession.owner;
    const setAirborne = airborne => {
      nativeValues['flight.ownship.inAir'] = airborne;
      selected['flight.ownship.inAir'] = airborne;
    };
    const submitOwnship = airborne => new Promise((resolve,reject) => {
      assert.equal(nextOwnshipSubmission, undefined);
      setAirborne(airborne);
      const timeout=setTimeout(() => {
        nextOwnshipSubmission=undefined;
        reject(Error('The DCS fixture did not send the changed ownship value'));
      },1000);
      nextOwnshipSubmission = submission => {
        clearTimeout(timeout);
        nextOwnshipSubmission=undefined;
        resolve(submission);
      };
    });
    if (seconds) {
      await checkpoint('Starting the combined desktop soak');
      evidence.phases.soak = await combined.soak({owner,directory,status,live,setAirborne});
      await checkpoint('Combined desktop soak complete');
    }
    await combined?.openInspector();
    await openDcsTelemetryReaders(cdp, !!browserUrl);
    try {
      const readers = () => cdp.evaluate(`(({fast,slow,error})=>({fast,slow,error}))(window.__dcsSlowReader)`);
      await until(async () => { const state = await readers(); return state.slow === 1 && state.fast > 0; }, 'Telemetry readers did not receive a baseline');
      if(combined) evidence.phases.panelPresentation = await combined.measurePresentation(owner, submitOwnship);
      const before = (await readers()).fast;
      const [, burst] = await Promise.all([sleep(1250), combined?.burst(owner, {
        setAirborne,
        setPaused(paused) { mode = paused ? 'paused' : 'live'; },
      })]);
      if (burst) evidence.phases.combined = burst;
      const stalled = await readers();
      assert.equal(stalled.error, null);
      assert.equal(stalled.slow, 1, 'Only one native delivery may be in flight');
      assert.ok(stalled.fast - before >= 16, 'The fast reader must continue during the withheld acknowledgement');
      evidence.phases.leaseOnly = await live();
      assert.deepEqual((await status()).profileSession.owner, owner);
      const ownshipStamp = page => BigInt(page.ports.find(row => row.endpoint.nodeId === 'telemetry'
        && row.endpoint.portId === 'flight.airspeed.true').observedUs);
      await passive(biosId, 1000);
      biosMode = 'off';
      const missing = await absent(biosId);
      evidence.phases.biosStale = await until(async () => {
        const page = await samples();
        return output(page, biosId)?.status === 'stale'
          && physicsIds.every(id => output(page, id)?.status === 'known' && near(output(page, id).value.value, selected[id])) && page;
      }, 'BIOS expiry must withdraw its derived output while fresh ownship processing remains live');
      const ownship = await passive('flight.airspeed.true', 210.25);
      assert.equal(ownship.context.aircraft, aircraft);
      assert.equal(ownship.context.physicsStatus, 'live');
      assert.ok(ownshipStamp(evidence.phases.biosStale) > ownshipStamp(evidence.phases.leaseOnly),
        'The ownship capture must advance through BIOS expiry');
      assert.deepEqual((await status()).profileSession.owner, owner);
      const afterExpiry = await readers();
      assert.equal(afterExpiry.slow, 1, 'BIOS expired while the slow receipt was still held');
      assert.equal(afterExpiry.error, null);
      assert.ok(afterExpiry.fast > stalled.fast, 'Fast notifications must continue through BIOS expiry');
      evidence.phases.biosExpiryUnderBackpressure = {missing, ownship, fastNotifications: afterExpiry.fast - stalled.fast};
      await cdp.evaluate('window.__dcsSlowReader.release()');
      await until(async () => (await readers()).slow > 1, 'Slow reader did not resume after acknowledgement');
      evidence.phases.slowReader = {fastNotifications: stalled.fast - before, withheldDeliveries: stalled.slow, ownerPreserved: true};
    } finally {
      await cdp.evaluate('window.__dcsSlowReader.close(); delete window.__dcsSlowReader');
    }
    await checkpoint('BIOS counter-only renewal and expiry verified with one stalled telemetry reader');
    if (combined) {
      evidence.phases.closedPreviews = await combined.withPreviewsClosed(owner, setAirborne);
      assert.deepEqual((await status()).profileSession.owner, owner);
      setAirborne(false);
    }
    const stopNs = process.hrtime.bigint();
    if (combined) Object.assign(evidence.phases.combined, await combined.stop(stopNs));
    else await request({command: 'profile_stop'});
    const budgetMisses = combined ? combinedTimingMisses(
      evidence.phases.panelPresentation, browserUrl || nativeRates ? evidence.phases.combined : null,
    ) : [];
    evidence.budgetMisses = budgetMisses;
    if (nativeCommands) biosMode = 'full';
    await request({command: 'profile_apply', id: profile.id, expectedRevision: profile.revision, gameId: 'dcs', expectedActiveOwner: null});
    if (nativeCommands) evidence.phases.commandReapply = await live();
    else evidence.phases.ownshipOnlyStart = await until(async () => {
      const page = await samples();
      return page && output(page, biosId)?.status !== 'known'
        && physicsIds.every(id => output(page, id)?.status === 'known' && near(output(page, id).value.value, selected[id])) && page;
    }, 'Fresh ownship must establish its read baseline while BIOS is unavailable');
    biosMode = 'full'; await live();
    await checkpoint('BIOS expiry and recovery preserve the independent ownship family');
    planeId++; modelTime = 0; mode = 'respawn-wait'; evidence.phases.respawn = await withdrawn('respawn baseline');
    mode = 'live'; await live(); await checkpoint('respawn requires advancing ownship baseline');
    mode = 'stopped'; evidence.phases.stopped = await withdrawn('mission stopped');
    mode = 'live'; await live();
    evidence.phases.beforeProcessExit = (await status()).profileSession;
    child.kill(); await until(() => child.exitCode !== null || child.signalCode !== null, 'Fixture process did not exit');
    evidence.phases.exit = await absent('flight.airspeed.true'); await absent(biosId);
    await until(async () => { const page = await samples(); return !page || Object.keys(selected).every(id => output(page, id)?.status !== 'known'); }, 'Process exit retained processing values');
    evidence.phases.afterProcessExit = (await status()).profileSession;
    if (desktop) evidence.phases.desktopExit = await desktop.verifyExit();
    checks.push('Independent inspection and selected native/canonical profile values preserve zero/false and units; unbound wind and BIOS aircraft gauge remain inspectable.');
    checks.push('Counter-only BIOS renewals preserve retained values. BIOS expiry withdraws its identity output without withdrawing fresh ownship.');
    checks.push(nativeCommands ? 'A fresh BIOS baseline allows the command profile to reapply without replay.' : 'A new read-only run can start from ownship alone and BIOS recovers independently.');
    checks.push('Pause, active pause, export denial, frozen model time, stale ownship, respawn baseline and mission stop withdraw physics without withdrawing unrelated fresh BIOS gauges; process exit withdraws both.');
    await checkpoint('process exit withdrawal verified');
    assert.deepEqual(budgetMisses, [], 'Combined software timing budgets');
  } catch (error) {
    evidence.failure = String(error); evidence.failureInspection = await samples().catch(() => null);
    evidence.failureBios = await inspect(biosId).catch(() => null); evidence.failurePhysics = await inspect('flight.airspeed.true').catch(() => null);
    evidence.failureGame = await game().catch(() => null); evidence.failureProfiles = await status().catch(() => null); throw error;
  } finally {
    clearInterval(timer); if (producer) producer.close();
    await cleanup('Stop profile', () => request({command: 'profile_stop'}));
    if (desktop) await cleanup('Restore desktop preferences and verify release', () => desktop.close());
    if (cleanupErrors.length) await cleanup('Recover failed Stop', async () => {
      const current = (await status()).profileSession;
      evidence.cleanupProfileSession = current;
      if (['faulted', 'recovery-needed'].includes(current.state)) await request({command: 'profile_recover'});
    });
    if (combined) await cleanup('Close combined panel and probe', () => combined.close());
    if (child && child.exitCode === null && child.signalCode === null) { child.kill(); await sleep(200); }
    if (profile) await cleanup('Delete temporary profile', () => request({command: 'profile_delete', id: profile.id, expectedRevision: profile.revision}));
    if (fixtureSetup) await cleanup('Remove private managed setup', async () => {
      await request({command: 'game_integration_remove', gameId: 'dcs', userFolder});
      await until(async () => (await game()).integrationSetup.state !== 'setting-up', 'Private setup removal did not finish', 15000);
      await assert.rejects(access(script));
    });
    if (fixtureInstallation) await cleanup('Restore selected installation', () => request({command: 'game_installation_select', gameId: 'dcs', installationId: installationBefore}));
    if (fixtureInstallation) await cleanup('Remove only the private installation', async () => {
      const installationFolder = resolve(directory, 'installation');
      assert.ok(installationFolder.startsWith(directory + sep));
      assert.equal(normalizedPath(fixtureInstallation.installPath), normalizedPath(installationFolder));
      await rm(installationFolder, {recursive: true, force: true});
      await until(async () => (await games()).discovery?.status !== 'scanning', 'Existing discovery did not finish', 90000);
      const scan = async paths => {
        await request({command: 'game_discovery_start', paths});
        return until(async () => { const scan = (await games()).discovery; return scan.status === 'ready' && scan; }, 'Fixture removal discovery did not finish', 90000);
      };
      let discovery = await scan([]);
      if (!discovery.libraries.some(library => library.id === 'disk')) {
        // The API enables disk removal checks only with a configured scan root.
        // Apply appends this private root; it preserves every existing scan root.
        discovery = await scan([directory]);
        evidence.cleanupAddedScanRoot = directory;
      }
      const removal = discovery.entries.find(item => item.action === 'remove' && item.gameId === 'dcs' && item.installationId === fixtureInstallation.id);
      assert.ok(removal, 'Discovery must offer removal of exactly the missing private installation.');
      assert.equal(normalizedPath(removal.installPath), normalizedPath(installationFolder));
      await request({command: 'game_discovery_apply', scanId: discovery.scanId, actions: discovery.entries.map(item => ({id: item.id, selected: item.id === removal.id}))});
      const current = await game(), original = initialGames.catalogue.find(item => item.id === 'dcs');
      assert.equal(current.selectedInstallationId, installationBefore);
      assert.deepEqual(current.installations, original.installations, 'Cleanup must preserve all original DCS installations exactly.');
      assert.equal(current.name, original.name);
      evidence.installationRestored = true;
    });
    if (before) await cleanup('Restore provider settings', async () => {
      await request({command: 'game_integration_settings_save', gameId: 'dcs', expectedSetupRevision: (await game()).integrationSetup.providerSetupRevision, connection: before.connection, userFolder: before.userFolder, aircraftModule: before.aircraftModule ?? null});
      await request({command: 'game_integration_check', gameId: 'dcs'});
      assert.deepEqual((await game()).integrationSetup.connection, before.connection);
    });
    if (initialProfiles?.session.selectedProfileId) await cleanup('Restore selected profile', () => request({command: 'profile_select', id: initialProfiles.session.selectedProfileId, selectionSource: 'manual'}));
    if (initialProfiles?.session.selectionMode) await cleanup('Restore selection mode', () => request({command: 'profile_selection_mode', mode: initialProfiles.session.selectionMode}));
    await cleanup('Leave profile stopped', () => request({command: 'profile_stop'}));
    if (!cleanupErrors.length) await cleanup('Remove copied DCS-BIOS catalogue', () => {
      assert.ok(biosTarget.startsWith(directory + sep));
      return rm(biosTarget, {recursive: true, force: true});
    });
    if (browserGatewayClient) await cleanup('Close direct browser gateway transport', () => browserGatewayClient.close());
    await cleanup('Close Cockpit page', () => cdp.close());
    evidence.finishedAt = new Date().toISOString();
    await writeFile(resolve(directory, 'evidence.json'), JSON.stringify(evidence, null, 2) + '\n');
    console.log(JSON.stringify({directory, checks, cleanupErrors})); assert.deepEqual(cleanupErrors, []);
  }
});
