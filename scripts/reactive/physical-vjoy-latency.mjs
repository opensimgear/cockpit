// Operator-only Windows fixture. Creates and removes one private one-button profile.
import assert from 'node:assert/strict';
import {execFileSync, spawn} from 'node:child_process';
import {createHash, randomUUID} from 'node:crypto';
import {mkdir, readFile, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createInterface} from 'node:readline';
import {nativePage, until} from './cdp.mjs';
import {physicalVjoyTiming} from './physical-vjoy-timing.mjs';

const env = process.env;
const preflight = process.argv[2] === '--preflight';
assert.ok(process.platform === 'win32' && env.OSG_PHYSICAL_VJOY_PRIVATE_DATABASE === '1',
  'Use a private Windows dev Cockpit and explicitly opt in');
const sourceId = env.OSG_PHYSICAL_VJOY_SOURCE_ID;
const controlId = env.OSG_PHYSICAL_VJOY_CONTROL_ID;
const slot = Number(env.OSG_PHYSICAL_VJOY_SLOT);
assert.match(sourceId, /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i);
assert.match(controlId, /^hid:0009:[0-9a-f]{4}:\d+:\d+:\d+$/i);
assert.ok(Number.isInteger(slot) && slot >= 1 && slot <= 16, 'Choose an exact free vJoy slot');
const executable = resolve(env.OSG_PHYSICAL_VJOY_APP);
const probePath = resolve(env.OSG_VJOY_RAW_PROBE);
const endpoint = new URL(env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222');
assert.ok(endpoint.protocol === 'http:' && endpoint.hostname === '127.0.0.1' && /^\d+$/.test(endpoint.port)
  && !endpoint.username && !endpoint.password, 'Use the local dev Cockpit DevTools endpoint');

// Identify the exact Cockpit owning the WebView2 listener. Avoid exposing the
// dedicated vJoy slot to a running simulator during this receipt-only fixture.
const identity = JSON.parse(execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `
  $ErrorActionPreference = 'Stop'
  $simulators = @('DCS','acs','acs_x86','AC2-Win64-Shipping','AssettoCorsaEVO','acr','X-Plane','iRacingSim64DX11','Le Mans Ultimate','rFactor2')
  if (Get-Process -Name $simulators -ErrorAction SilentlyContinue) { throw 'Close the simulator for this fixture' }
  $listeners = @(Get-NetTCPConnection -LocalAddress 127.0.0.1 -LocalPort ([int]$env:OSG_PHYSICAL_CDP_PORT) -State Listen)
  if ($listeners.Count -ne 1) { throw 'Expected one local DevTools listener' }
  $process = Get-CimInstance Win32_Process -Filter "ProcessId = $($listeners[0].OwningProcess)"
  for ($depth = 0; $process -and $depth -lt 8 -and $process.ExecutablePath -ne $env:OSG_PHYSICAL_EXPECTED_APP; $depth++) {
    $process = Get-CimInstance Win32_Process -Filter "ProcessId = $($process.ParentProcessId)"
  }
  if (!$process -or $process.ExecutablePath -ne $env:OSG_PHYSICAL_EXPECTED_APP) { throw 'Unexpected Cockpit process' }
  [pscustomobject]@{pid=$process.ProcessId; executable=$process.ExecutablePath; created=$process.CreationDate.ToUniversalTime().ToString('o')} | ConvertTo-Json -Compress
`], {windowsHide: true, timeout: 10_000, encoding: 'utf8',
  env: {...env, OSG_PHYSICAL_CDP_PORT: endpoint.port, OSG_PHYSICAL_EXPECTED_APP: executable},
}));
const sha256 = async path => createHash('sha256').update(await readFile(path)).digest('hex');
const directory = resolve('.research/reactive/physical-vjoy-latency', randomUUID());
await mkdir(directory, {recursive: true});
const evidence = {
  at: new Date().toISOString(), preflight, sourceId, controlId, slot, identity,
  appSha256: await sha256(executable), probeSha256: await sha256(probePath), cleanupErrors: [],
};

const page = await nativePage(endpoint.origin);
const request = async value => {
  const reply = await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',
    {request:${JSON.stringify({...value,clientId:'physical-vjoy-latency'})}})`);
  assert.equal(reply.ok, true, `${value.command}: ${reply.error}`);
  return reply.state;
};
const status = async () => (await request({command: 'profile_status'})).profiles.profileSession;
const inventory = async () => (await request({command: 'system_inventory'})).systemInventory.virtualDevices;
let saved, probe, exited;
try {
  await until(() => page.evaluate(`!!window.__TAURI_INTERNALS__?.invoke &&
    !!document.querySelector('nav[aria-label="Main navigation"]')`));
  assert.equal((await status()).owner, null, 'Stop any active profile first');
  const target = await until(inventory, rows => rows.some(row => row.slot === slot && row.status === 'free'));
  const device = target.find(row => row.slot === slot);
  assert.ok(device.buttons >= 1 && Array.isArray(device.axes) && !device.discreteHats && !device.forceFeedback
    && device.buttons + device.axes.length + device.continuousHats <= 128, 'The chosen slot must have a compatible layout');
  const layout = {
    buttons: Array.from({length: device.buttons}, (_, index) => `button-${index + 1}`),
    axes: device.axes.map((_, index) => ({portId: `axis-${index + 1}`, range: 'bipolar', fallback: 0})),
    hats: Array.from({length: device.continuousHats}, (_, index) => `hat-${index + 1}`),
  };
  const prepared = await page.evaluate(`(async () => {
    const {gatewayRequest} = await import('/src/lib/gateway.ts');
    const {blankProfile, addProfileResources} = await import('/src/lib/profile-editor.ts');
    const {controllerSourceId} = await import('/src/lib/controller-inputs.ts');
    const {ProfileInventoryClient} = await import('/src/lib/profile-inventory.ts');
    const {graphSetVirtual, graphConnect} = await import('/src/lib/profile-graph.ts');
    const resources = new ProfileInventoryClient(async request => (await gatewayRequest(request)).profileResources);
    const inventory = await resources.listAll();
    const matches = inventory.resources.filter(row => controllerSourceId(row) === ${JSON.stringify(sourceId)});
    if (matches.length !== 1) throw Error('The selected physical controller must resolve to one resource');
    const {resource, ports} = await resources.inspectAll(matches[0].id);
    const port = ports.find(port => port.descriptor.id === ${JSON.stringify(controlId)} && port.current);
    if (resource.resourceType !== 'host-controller' || !port || port.descriptor.contract.schema.type !== 'boolean')
      throw Error('The exact observed HID button must be ready');
    const draft = addProfileResources(blankProfile(crypto.randomUUID(), 'Physical vJoy timing ' + Date.now()), [], [resource]);
    const source = draft.document.resources[0];
    source.ports = [structuredClone(port.descriptor)];
    draft.assignments.find(row => row.resourceId === source.id).ports = [{
      profilePortId: port.descriptor.id, observedPortId: port.descriptor.id, expectedContractHash: port.contractHash,
    }];
    const wired = graphConnect(graphSetVirtual(draft, {
      id: 'physical-vjoy', name: 'Physical vJoy', required: true, layout: ${JSON.stringify(layout)},
    }), {nodeId: source.id, portId: port.descriptor.id}, {nodeId: 'physical-vjoy', portId: 'button-1'});
    return {wired, inputResource: {id: resource.id, name: resource.name, generation: resource.generation,
      capabilityRevision: resource.capabilityRevision}};
  })()`);
  evidence.inputResource = prepared.inputResource;
  saved = (await request({command: 'profile_editor_save', ...prepared.wired, expectedRevision: null})).profiles.document;
  evidence.profile = {id: saved.id, revision: saved.revision};
  await request({command: 'profile_virtual_slot_set', id: saved.id, expectedRevision: saved.revision, nodeId: 'physical-vjoy', slot});

  const beforeProbeNs = process.hrtime.bigint();
  probe = spawn(probePath, ['--vjoy-buttons', preflight ? '5000' : '15000'], {
    windowsHide: true, timeout: preflight ? 9_000 : 19_000,
  });
  let ready;
  const started = new Promise(resolve => { ready = resolve; });
  const output = [];
  createInterface({input: probe.stdout}).on('line', line => {
    if (line === 'ready') ready();
    else output.push(line);
  });
  exited = new Promise((resolve, reject) => { probe.once('error', reject); probe.once('close', resolve); });
  await Promise.race([started, exited.then(() => { throw Error('Raw Input probe did not become ready'); })]);
  const probeReadyNs = process.hrtime.bigint();

  const active = (await request({command: 'profile_apply', id: saved.id, expectedRevision: saved.revision,
    expectedActiveOwner: null})).profiles.profileSession;
  assert.equal(active.owner?.profileId, saved.id);
  assert.equal(active.state, 'running');
  evidence.owner = active.owner;
  await until(inventory, rows => rows.some(row => row.slot === slot && row.status === 'busy'));
  const cue = preflight ? null : setTimeout(() => {
    console.log('Tap the selected button at least 10 times. Finish released within 7 seconds.');
  }, 350);
  try {
    evidence.source = await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',
      {request:${JSON.stringify({command: 'qualification_physical_button', sourceId, controlId, durationMs: preflight ? 750 : 8_000})}})`);
  } finally {
    clearTimeout(cue);
  }
  assert.equal(evidence.source.sourceId, sourceId);
  assert.equal(evidence.source.control.id, controlId);
  await new Promise(resolve => setTimeout(resolve, 250));
  assert.deepEqual((await status()).owner, active.owner);
  const stopNs = process.hrtime.bigint();
  await request({command: 'profile_stop'});
  const stopped = await status();
  assert.equal(stopped.owner, null);
  const released = await until(inventory, rows => rows.some(row => row.slot === slot && row.status === 'free'));
  evidence.stop = {state: stopped.state, owner: stopped.owner, slotStatus: released.find(row => row.slot === slot).status};
  assert.equal(await exited, 0);
  evidence.receiver = JSON.parse(output.at(-1));
  evidence.clock = {beforeProbeNs: beforeProbeNs.toString(), probeReadyNs: probeReadyNs.toString(), stopNs: stopNs.toString()};
  evidence.timing = physicalVjoyTiming(evidence.source, evidence.receiver, {slot, beforeProbeNs, probeReadyNs, stopNs, preflight});
} catch (error) {
  evidence.failure = String(error);
  throw error;
} finally {
  try {
    if (saved) {
      const current = (await status()).owner;
      if (current?.profileId === saved.id) await request({command: 'profile_stop'});
      await request({command: 'profile_delete', id: saved.id, expectedRevision: saved.revision});
      evidence.profileDeleted = true;
    }
  } catch (error) {
    evidence.cleanupErrors.push(String(error));
  } finally {
    if (probe && probe.exitCode === null && probe.signalCode === null) probe.kill();
    await exited?.catch(() => {});
    page.close();
    await writeFile(resolve(directory, 'evidence.json'), JSON.stringify(evidence, null, 2) + '\n');
    console.log(`Evidence saved to ${directory}`);
  }
}
assert.deepEqual(evidence.cleanupErrors, []);
const {edges, ...summary} = evidence.timing;
console.log(JSON.stringify({matchedEdges: edges.length, ...summary}, null, 2));
