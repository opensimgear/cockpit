// Exercises the real native USB worker's unsolicited status probes during live heartbeats.
// Requires a paired Sunton with an assigned panel; changes no assignments or settings.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const { ws: RawSocket } = createRequire(require.resolve(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright'))('playwright-core/lib/utilsBundle');
const artifacts = resolve('.research/companion-serial-heartbeat-native');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function connectNative() {
  const targets = await (await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222'}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
  const endpoint = process.env.OSG_NATIVE_PAGE_WS ?? targets.find(target => target.type === 'page' && /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url) && !target.url.includes('profile-progress'))?.webSocketDebuggerUrl;
  assert.ok(endpoint, 'Start the actual native dev Cockpit with CDP enabled.');
  const socket = new RawSocket(endpoint, { handshakeTimeout: 8000 });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', reject); });
  let sequence = 0; const pending = new Map();
  socket.on('error', () => {});
  socket.on('message', bytes => {
    const message = JSON.parse(bytes.toString()), operation = pending.get(message.id);
    if (!operation) return;
    clearTimeout(operation.timer); pending.delete(message.id);
    if (message.error) operation.reject(new Error(message.error.message)); else operation.resolve(message.result);
  });
  socket.on('close', () => { for (const operation of pending.values()) { clearTimeout(operation.timer); operation.reject(new Error('Dev Cockpit disconnected.')); } pending.clear(); });
  return {
    async evaluate(expression) {
      const result = await new Promise((resolve, reject) => {
        const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(new Error('Native status request timed out.')); }, 10000);
        pending.set(id, { resolve, reject, timer });
        socket.send(JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression, returnByValue: true, awaitPromise: true } }));
      });
      if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
      return result.result.value;
    },
    close() { socket.close(); },
  };
}

test('physical Sunton retains one healthy USB session through ninety seconds of status probes and heartbeats', { skip: !!process.env.OSG_HEARTBEAT_PROFILE_ID, timeout: 170000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const native = await connectNative();
  const evidence = { actualNativeCockpit: true, physicalCompanion: true, sampleIntervalMs: 150, samples: [], sampleCount: 0, maxStatusAgeMs: 0 };
  let companionId;
  const request = async (command, companion = {}) => {
    const response = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ command, companion, clientId: 'companion-serial-heartbeat-native' })}})`);
    assert.equal(response.ok, true, response.error ?? command);
    return response.state.companions;
  };
  const link = state => (state.usb.devices ?? [state.usb]).find(row => row.device?.companionId === companionId);
  const instance = state => state.host.instances.find(row => row.companionId === companionId);
  const assignments = state => state.host.instances.map(({ instanceId, companionId, publicationHash }) => ({ instanceId, companionId, publicationHash })).sort((a, b) => a.instanceId.localeCompare(b.instanceId));
  try {
    const before = await request('companion_status');
    const selected = (before.usb.devices ?? [before.usb]).find(row => row.connected && (process.env.OSG_HEARTBEAT_COMPANION_ID ? row.device?.companionId === process.env.OSG_HEARTBEAT_COMPANION_ID : row.device?.boardProfile === 'sunton-3248s035r'));
    assert.ok(selected, 'Connect the paired Sunton, or set OSG_HEARTBEAT_COMPANION_ID to the exact USB companion.');
    companionId = selected.device.companionId;
    assert.ok(before.host.clients.some(row => row.companionId === companionId), 'The selected companion must be paired.');
    const original = instance(before), previousSession = selected.device.sessionId;
    assert.ok(original, 'Assign a panel to the physical companion before acceptance.');
    assert.equal(original.profileOwned, false, 'Deactivate the profile before testing its companion connection.');
    evidence.companionId = companionId; evidence.boardProfile = selected.device.boardProfile; evidence.firmwareVersion = selected.device.firmwareVersion;
    evidence.assignment = { instanceId: original.instanceId, publicationHash: original.publicationHash };
    const started = Date.now();
    await request('companion_usb_reconnect', { companionId });
    let ready;
    while (Date.now() - started < 60000) {
      const state = await request('companion_status'), peer = link(state)?.device, current = instance(state);
      if (peer?.sessionId && peer.sessionId !== previousSession) {
        assert.notEqual(peer.operation?.phase, 'failed', peer.statusMessage ?? peer.operation?.errorCode);
        if (peer.status === 'live-armed' && peer.operation?.phase === 'ready' && peer.publicationHash === original.publicationHash && current?.connected && current.armed && current.sessionId === peer.sessionId && current.generation === peer.generation && current.transport === 'usb') { ready = { peer, current }; break; }
      }
      await sleep(150);
    }
    assert.ok(ready, 'One explicit reconnect must establish a fresh actual USB session and neutral baseline.');
    evidence.reconnectMs = Date.now() - started;
    evidence.sessionId = ready.peer.sessionId; evidence.generation = ready.peer.generation; evidence.initialOperation = ready.peer.operation;
    const soakStarted = Date.now(); let lastRecorded = -1000, latest;
    do {
      latest = await request('companion_status');
      const usb = link(latest), peer = usb?.device, current = instance(latest), elapsedMs = Date.now() - soakStarted;
      const sample = { elapsedMs, connected: usb?.connected, status: peer?.status, statusAgeMs: usb?.statusAgeMs, sessionId: peer?.sessionId, phase: peer?.operation?.phase, errorCode: peer?.operation?.errorCode };
      evidence.sampleCount++;
      if (elapsedMs - lastRecorded >= 1000 || !usb?.connected || peer?.status !== 'live-armed' || peer?.operation?.phase === 'failed') { evidence.samples.push(sample); lastRecorded = elapsedMs; }
      assert.equal(usb?.connected, true, 'The actual USB link must remain connected.');
      assert.ok(Number.isFinite(usb.statusAgeMs) && usb.statusAgeMs >= 0 && usb.statusAgeMs <= 5000, 'Nonce-bound full USB status must remain fresh within five seconds.');
      evidence.maxStatusAgeMs = Math.max(evidence.maxStatusAgeMs, usb.statusAgeMs);
      assert.notEqual(peer.operation?.phase, 'failed', peer.statusMessage ?? peer.operation?.errorCode);
      assert.equal(peer.status, 'live-armed', peer.statusMessage ?? 'The physical screen lost its live session.');
      assert.equal(peer.sessionId, ready.peer.sessionId, 'A reconnect must not conceal a failed heartbeat.');
      assert.equal(peer.generation, ready.peer.generation);
      assert.equal(current?.sessionId, ready.peer.sessionId); assert.equal(current?.armed, true); assert.equal(current?.connected, true);
      assert.equal(current?.publicationHash, original.publicationHash); assert.equal(current?.transport, 'usb');
      if (elapsedMs >= 90000) { evidence.observedMs = elapsedMs; break; }
      await sleep(150);
    } while (true);
    assert.deepEqual(assignments(latest), assignments(before), 'No companion assignment, including other screens, may change.');
    evidence.finalOperation = link(latest).device.operation; evidence.passed = true;
  } catch (error) { evidence.error = error.message; throw error; }
  finally {
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify(evidence, null, 2) + '\n');
    native.close();
  }
});

// Unlike the standalone reconnect acceptance above, this mode observes an
// already active game profile and its real feedback without changing ownership.
test('active game feedback and USB status probes keep the same physical Sunton session healthy throughout the active soak', { skip: !process.env.OSG_HEARTBEAT_PROFILE_ID, timeout: Number(process.env.OSG_HEARTBEAT_SOAK_MS ?? 180000) + 60000 }, async () => {
  const profileId = process.env.OSG_HEARTBEAT_PROFILE_ID,
    sunton = '40efcae8-07ab-4422-a69a-80d08029a700',
    waveshare = 'c051ec9e-5086-468b-8e93-0c2eeff0b522',
    durationMs = Number(process.env.OSG_HEARTBEAT_SOAK_MS ?? 180000),
    probeIntervalMs = 150,
    artifacts = resolve('.research/companion-serial-heartbeat-native-active');

  assert.ok(durationMs >= 90000 && durationMs <= 600000, 'Soak must last ninety seconds through ten minutes.');

  await mkdir(artifacts, { recursive: true });

  const native = await connectNative();

  const request = async value => {
    const reply = await native.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ ...value, clientId: 'active-acr-usb-heartbeat-native' })}})`);
    assert.equal(reply.ok, true, reply.error ?? value.command);
    return reply.state;
  };

  const profile = async () => (await request({ command: 'profile_status' })).profiles.profileSession;

  const usbLink = (usb, id) => (usb.devices ?? [usb]).find(row => row.device?.companionId === id);

  const instance = (companions, id) => companions.host.instances.find(row => row.companionId === id);

  const shape = row => row ? Object.fromEntries([
    'instanceId',
    'companionId',
    'publicationHash',
    'sessionId',
    'generation',
    'armed',
    'connected',
    'transport',
    'profileOwned'
  ].map(key => [key, row[key]])) : null;

  const usbShape = row => ({
    port: row?.port,
    connected: row?.connected,
    companionId: row?.device?.companionId,
    sessionId: row?.device?.sessionId,
    generation: row?.device?.generation,
    publicationHash: row?.device?.publicationHash,
    status: row?.device?.status
  });

  const endpoint = (value) => JSON.stringify([value.nodeId, value.portId]);

  const evidence = {
    actualNativeCockpit: true,
    physicalSunton: true,
    readOnly: true,
    syntheticTelemetry: false,
    durationMs,
    probeIntervalMs,
    usbPolls: 0,
    profilePolls: 0,
    freshStatusObservations: 0,
    knownFeedbackPairs: 0,
    maxStatusAgeMs: 0,
    maxHostStatusLatencyMs: 0,
    maxWriteElapsedMs: 0,
    maxWriteBytes: 0,
    samples: [],
    profileSamples: [],
    startedAt: new Date().toISOString()
  };

  let stop = false,
    start,
    baselineOwner,
    baselinePeer,
    baselineInstance,
    baselineWave,
    baselineWaveUsb,
    desiredHash,
    binding,
    initialIo,
    latestIo,
    latestStatus,
    latestCompanions,
    lastAge = Infinity,
    seen = new Set(),
    tasks = [];

  function checkIo(io) {
    assert.ok(io, 'New native serial write metrics must be exposed.');
    assert.equal(io.baudRate, 115200);
    assert.equal(io.readTimeoutMs, 10);
    assert.equal(io.writeFailures, 0, 'No UART write may fail.');
    assert.equal(io.lastWriteError, null, 'The most recent full write must succeed.');
    assert.ok(!io.lastWriteFailure, 'Sticky failure evidence must stay absent.');
    assert.equal(io.lastWriteBytes, io.lastWriteWrittenBytes, 'Successful writes must send the complete NDJSON record.');
    assert.equal(io.lastWriteTimeoutMs, Math.ceil(io.lastWriteBytes * 10000 / io.baudRate) + 250, 'Write budget must include complete wire time plus scheduling margin.');
    evidence.maxWriteElapsedMs = Math.max(evidence.maxWriteElapsedMs, io.maxWriteElapsedMs ?? io.lastWriteElapsedMs ?? 0);
    evidence.maxWriteBytes = Math.max(evidence.maxWriteBytes, io.maxWriteBytes ?? io.lastWriteBytes ?? 0);
    latestIo = io;
  }

  function checkPeer(companions, usb) {
    const row = usbLink(usb, sunton),
      peer = row?.device,
      current = instance(companions, sunton);
    evidence.lastObservedLink = {
      port: row?.port,
      connected: row?.connected,
      ready: row?.ready,
      statusAgeMs: row?.statusAgeMs,
      serialIo: row?.serialIo,
      device: peer
    };
    assert.equal(row?.port, 'COM8');
    assert.equal(row?.connected, true);
    assert.equal(row?.ready, true);
    assert.equal(peer?.status, 'live-armed', peer?.statusMessage ?? row?.stage);
    assert.equal(peer?.operation?.phase, 'ready', peer?.operation?.message ?? peer?.statusMessage);
    assert.ok(!peer.operation.errorCode);
    assert.ok(!row.lastHostError);
    assert.equal(peer.sessionId, baselinePeer.sessionId, 'No implicit reconnect may conceal a failed heartbeat.');
    assert.equal(peer.generation, baselinePeer.generation);
    assert.equal(peer.publicationHash, desiredHash);
    assert.equal(peer.operation.attemptId, baselinePeer.operation.attemptId, 'Panel operation cannot restart during the soak.');
    assert.equal(peer.operation.sessionId, baselinePeer.sessionId);
    assert.equal(peer.operation.generation, baselinePeer.generation);
    assert.equal(peer.operation.publicationHash, desiredHash);
    assert.deepEqual(shape(current), baselineInstance);
    assert.equal(current?.connected, true);
    assert.equal(current?.armed, true);
    assert.equal(current?.profileOwned, true);
    assert.equal(current?.operation?.phase, 'ready');
    assert.ok(Number.isFinite(row.statusAgeMs) && row.statusAgeMs >= 0 && row.statusAgeMs <= 3000, 'Nonce-bound full device status must keep arriving.');
    if (row.statusAgeMs + 100 < lastAge) evidence.freshStatusObservations++;
    lastAge = row.statusAgeMs;
    evidence.maxStatusAgeMs = Math.max(evidence.maxStatusAgeMs, row.statusAgeMs);
    checkIo(row.serialIo);
    assert.deepEqual(shape(instance(companions, waveshare)), baselineWave, 'The other Waveshare instance must remain unchanged.');
    assert.deepEqual(usbShape(usbLink(usb, waveshare)), baselineWaveUsb, 'The other COM5 USB session must remain unchanged.');
    return row;
  }

  try {
    latestStatus = await profile();
    assert.equal(latestStatus.state, 'running', 'Activate the existing ACR profile before starting this read-only soak.');
    assert.equal(latestStatus.fault, null);
    assert.equal(latestStatus.owner?.profileId, profileId);
    baselineOwner = latestStatus.owner;

    const [companionState, usbState, savedState, inventoryState] = await Promise.all([
      request({ command: 'companion_status' }),
      request({ command: 'companion_usb_status' }),
      request({ command: 'profile_assignments_get', id: profileId }),
      request({
        command: 'profile_resources_list',
        offset: 0,
        limit: 128
      })
    ]);
    latestCompanions = companionState.companions;

    const saved = savedState.profiles,
      inventory = inventoryState.profileResources.resources,
      source = inventory.find(row => row.resourceType === 'companion' && row.attributes.companionId === sunton && row.attributes.componentKind === 'paired-companion');
    assert.ok(source);
    const local = saved.assignments.find(row => row.sourceId === source.id);
    assert.ok(local);
    desiredHash = saved.document.companionAssignments.find(row => row.resourceId === local.resourceId)?.publicationHash;
    assert.ok(desiredHash);
    binding = saved.document.bindings.find(row => row.to.nodeId === local.resourceId && saved.document.resources.some(resource => resource.id === row.from.nodeId && resource.kind === 'game'));
    assert.ok(binding, 'The existing ACR graph must contain its direct game-speed to companion-value binding.');

    baselinePeer = usbLink(usbState.companions.usb, sunton)?.device;
    assert.ok(baselinePeer);
    baselineInstance = shape(instance(latestCompanions, sunton));
    baselineWave = shape(instance(latestCompanions, waveshare));
    baselineWaveUsb = usbShape(usbLink(usbState.companions.usb, waveshare));
    if (baselineWaveUsb.port) assert.equal(baselineWaveUsb.port, 'COM5');
    initialIo = usbLink(usbState.companions.usb, sunton)?.serialIo;
    evidence.owner = baselineOwner;
    evidence.peer = {
      sessionId: baselinePeer.sessionId,
      generation: baselinePeer.generation,
      publicationHash: desiredHash,
      firmwareVersion: baselinePeer.firmwareVersion,
      boardProfile: baselinePeer.boardProfile,
      operation: baselinePeer.operation
    };
    evidence.waveshare = { instance: baselineWave, usb: baselineWaveUsb };
    evidence.binding = binding;
    evidence.initialSerialIo = initialIo;
    checkPeer(latestCompanions, usbState.companions.usb);
    start = Date.now();

    const usbLoop = async () => {
      let recorded = -1000,
        progress = -15000;
      while (!stop && Date.now() - start < durationMs) {
        const began = Date.now(),
          [companionState, usbState] = await Promise.all([
            request({ command: 'companion_status' }),
            request({ command: 'companion_usb_status' })
          ]);
        latestCompanions = companionState.companions;
        const row = checkPeer(latestCompanions, usbState.companions.usb),
          elapsedMs = Date.now() - start;
        evidence.usbPolls++;
        evidence.maxHostStatusLatencyMs = Math.max(evidence.maxHostStatusLatencyMs, Date.now() - began);
        if (elapsedMs - recorded >= 1000) {
          evidence.samples.push({
            elapsedMs,
            statusAgeMs: row.statusAgeMs,
            phase: row.device.operation.phase,
            status: row.device.status,
            sessionId: row.device.sessionId,
            generation: row.device.generation,
            serialIo: row.serialIo
          });
          recorded = elapsedMs;
        }
        if (elapsedMs - progress >= 15000) {
          console.log(JSON.stringify({
            elapsedMs,
            usbPolls: evidence.usbPolls,
            knownFeedbackPairs: evidence.knownFeedbackPairs,
            maxStatusAgeMs: evidence.maxStatusAgeMs,
            serialIo: row.serialIo
          }));
          progress = elapsedMs;
        }
        await sleep(Math.max(0, probeIntervalMs - (Date.now() - began)));
      }
    };

    const profileLoop = async () => {
      while (!stop && Date.now() - start < durationMs) {
        const began = Date.now(),
          [statusState, inspectionState, gameState] = await Promise.all([
            request({ command: 'profile_status' }),
            request({
              command: 'profile_runtime_inspect',
              owner: baselineOwner,
              offset: 0,
              limit: 64
            }),
            request({ command: 'game_get_state' })
          ]);
        latestStatus = statusState.profiles.profileSession;
        assert.deepEqual(latestStatus.owner, baselineOwner, 'The exact active profile owner cannot change during soak.');
        assert.equal(latestStatus.state, 'running');
        assert.equal(latestStatus.fault, null);
        assert.deepEqual(latestStatus.degradation, []);
        const inspected = inspectionState.profileInspection;
        assert.deepEqual(inspected.owner, baselineOwner);
        const from = inspected.ports.find(port => endpoint(port.endpoint) === endpoint(binding.from)),
          to = inspected.ports.find(port => endpoint(port.endpoint) === endpoint(binding.to));
        assert.ok(from && to);
        evidence.lastFeedbackCapture = {
          elapsedMs: Date.now() - start,
          source: from,
          destination: to
        };
        if (from.sample.status === 'known' && to.sample.status === 'known') {
          assert.deepEqual(to.sample, from.sample, 'Same-capture real speed source and companion destination must match.');
          assert.equal(to.observedUs, from.observedUs);
          evidence.knownFeedbackPairs++;
          seen.add(JSON.stringify(from.sample.value));
        }
        const game = gameState.games.catalogue.find(row => row.id === 'assetto-corsa-rally');
        evidence.profilePolls++;
        evidence.profileSamples.push({
          elapsedMs: Date.now() - start,
          state: latestStatus.state,
          source: from.sample,
          destination: to.sample,
          observedUs: from.observedUs,
          gameContext: game?.integrationContext
        });
        await sleep(Math.max(0, 1000 - (Date.now() - began)));
      }
    };

    tasks = [
      usbLoop(),
      profileLoop()
    ];
    await Promise.all(tasks);
    evidence.observedMs = Date.now() - start;
    assert.ok(evidence.observedMs >= durationMs);
    assert.ok(evidence.knownFeedbackPairs >= 10, 'Real game feedback must be observed repeatedly.');
    assert.ok(evidence.freshStatusObservations >= durationMs / 3000, 'Repeated nonce-bound full status replies must be observed.');
    assert.ok(latestIo.writesCompleted > initialIo.writesCompleted);
    assert.ok(latestIo.totalWrittenBytes > initialIo.totalWrittenBytes);
    evidence.finalSerialIo = latestIo;
    evidence.distinctRealSourceValues = seen.size;
    evidence.finalStatus = latestStatus;
    evidence.result = 'passed';
  } catch (error) {
    stop = true;
    await Promise.allSettled(tasks);
    evidence.error = error.stack;
    evidence.observedMs = start ? Date.now() - start : 0;
    evidence.finalSerialIo = latestIo;
    evidence.finalStatus = latestStatus;
    throw error;
  } finally {
    stop = true;
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify(evidence, null, 2) + '\n');
    native.close();
  }

  console.log(JSON.stringify({
    result: evidence.result,
    observedMs: evidence.observedMs,
    usbPolls: evidence.usbPolls,
    profilePolls: evidence.profilePolls,
    knownFeedbackPairs: evidence.knownFeedbackPairs,
    freshStatusObservations: evidence.freshStatusObservations,
    maxStatusAgeMs: evidence.maxStatusAgeMs,
    maxWriteElapsedMs: evidence.maxWriteElapsedMs,
    maxWriteBytes: evidence.maxWriteBytes,
    artifacts
  }));
});
