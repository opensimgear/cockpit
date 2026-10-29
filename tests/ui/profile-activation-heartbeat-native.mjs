// Soak activation heartbeat work against the user's actual DCS Feature Lab
// profile and running DCS process. This test deliberately leaves the profile active.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');
const artifacts = resolve('.research/activation-heartbeat-native');
const soakMs = Number(process.env.OSG_ACTIVATION_SOAK_MS ?? 120_000);
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const clone = value => value == null ? value : JSON.parse(JSON.stringify(value));

test('DCS Feature Lab keeps its active owner through native activation heartbeat work', {
  timeout: Math.max(180_000, soakMs + 90_000),
}, async () => {
  assert.ok(Number.isFinite(soakMs) && soakMs >= 120_000, 'OSG_ACTIVATION_SOAK_MS must be at least 120000');
  await mkdir(artifacts, { recursive: true });
  const startupDeadline = Date.now() + 15_000;
  let browser;
  while (!browser) {
    try { browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222', {timeout: 2_000}); }
    catch (error) { if (Date.now() >= startupDeadline) throw error; await sleep(200); }
  }
  let page;
  while (!page && Date.now() < startupDeadline) {
    page = browser.contexts().flatMap(context => context.pages()).find(candidate =>
      /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(candidate.url())
      && !candidate.url().includes('profile-activation-window'));
    if (!page) await sleep(100);
  }
  if (!page) await browser.close();
  assert.ok(page, 'Start the actual native dev Cockpit with CDP enabled.');
  await page.waitForFunction(() => !!window.__TAURI_INTERNALS__?.invoke, undefined, {timeout: 10_000});

  const evidence = {
    recordedAt: new Date().toISOString(),
    actualNativeCockpit: true,
    actualDcsProcessRequired: true,
    soakMs,
    profile: null,
    initial: null,
    final: null,
    statusPolls: { count: 0, minMs: null, maxMs: null, states: {} },
    gamePolls: { count: 0, minMs: null, maxMs: null, detected: 0, processVerified: 0 },
    companion: { connected: false, panelAssigned: false, samples: [], knownPorts: [], unavailableAfterWarmup: [] },
    failure: null,
  };

  const native = async request => {
    const reply = await page.evaluate(request => {
      if (!window.__TAURI_INTERNALS__?.invoke) throw new Error('Existing page is not native Cockpit.');
      return window.__TAURI_INTERNALS__.invoke('gateway_request', {
        request: { ...request, clientId: 'profile-activation-heartbeat-native' },
      });
    }, request);
    assert.equal(reply.ok, true, `${request.command}: ${reply.error}`);
    return reply.state;
  };
  const status = async () => (await native({ command: 'profile_status' })).profiles.profileSession;
  const games = async () => (await native({ command: 'game_get_state' })).games;
  const recordLatency = (bucket, started) => {
    const elapsedMs = Date.now() - started;
    bucket.count++;
    bucket.minMs = bucket.minMs == null ? elapsedMs : Math.min(bucket.minMs, elapsedMs);
    bucket.maxMs = bucket.maxMs == null ? elapsedMs : Math.max(bucket.maxMs, elapsedMs);
  };
  const assertDcsRunning = gameState => {
    const dcs = gameState.catalogue?.find(game => game.id === 'dcs');
    const detected = gameState.detectedGames?.some(game => game.gameId === 'dcs') ?? false;
    const processVerified = dcs?.integrationContext?.processVerified === true;
    if (detected) evidence.gamePolls.detected++;
    if (processVerified) evidence.gamePolls.processVerified++;
    assert.ok(detected || processVerified, 'DCS is not present in the actual game process monitor.');
    return { detected, processVerified, contextStatus: dcs?.integrationContext?.status ?? null };
  };

  try {
    let before = await status();
    if (['faulted', 'recovery-needed'].includes(before.state)) {
      await native({ command: 'profile_recover' });
      before = await status();
      assert.ok(!['faulted', 'recovery-needed'].includes(before.state), 'Profile recovery did not complete.');
    }

    const actualGames = await games();
    evidence.initial = {
      state: before.state,
      owner: clone(before.owner),
      dcs: assertDcsRunning(actualGames),
    };
    const summaries = (await native({ command: 'profile_list' })).profiles.profiles;
    const summary = summaries.find(profile => profile.name === 'DCS Feature Lab');
    assert.ok(summary, 'Saved profile "DCS Feature Lab" was not found.');
    const document = (await native({ command: 'profile_get', id: summary.id })).profiles.document;
    assert.equal(document.id, summary.id, 'Profile identity changed during lookup.');
    assert.equal(document.name, 'DCS Feature Lab');
    assert.ok(document.eligibility?.gameIds?.includes('dcs'), 'DCS Feature Lab is not eligible for DCS.');
    assert.equal(document.revision, summary.revision, 'Profile revision changed between list and get.');
    evidence.profile = { id: document.id, name: document.name, revision: document.revision, gameId: 'dcs' };

    // The reported scenario has an online browser companion. A fresh Cockpit
    // process can start before Brave's reconnect backoff has elapsed; do not
    // silently test an activation that intentionally skips an offline optional.
    const companionDeadline = Date.now() + 30_000;
    while (true) {
      const host = (await native({command: 'companion_status'})).companions.host;
      const client = host.clients.find(client => client.kind === 'browser' && client.connected);
      if (client) { evidence.initial.companion = {companionId: client.companionId, connected: true}; break; }
      assert.ok(Date.now() < companionDeadline, 'Connect the actual browser companion before activation.');
      await sleep(250);
    }

    // Supply the observed active owner so the apply is guarded against replacing
    // a profile that changed after the read. Do not stop or reset the session.
    await native({
      command: 'profile_apply',
      id: document.id,
      expectedRevision: document.revision,
      gameId: 'dcs',
      expectedActiveOwner: clone(before.owner),
    });

    const activationDeadline = Date.now() + 90_000;
    let active;
    while (Date.now() < activationDeadline) {
      active = await status();
      if (active.owner?.profileId === document.id && ['running', 'degraded'].includes(active.state)) break;
      assert.ok(!['faulted', 'recovery-needed'].includes(active.state), `Activation entered ${active.state}: ${active.fault ?? ''}`);
      await sleep(200);
    }
    assert.ok(active?.owner?.profileId === document.id, 'DCS Feature Lab did not acquire active ownership.');
    assert.ok(['running', 'degraded'].includes(active.state), `DCS Feature Lab ended activation in ${active.state}.`);
    assert.equal(active.fault, null);
    assert.equal(active.owner.gameId, 'dcs');
    evidence.initial.activatedOwner = clone(active.owner);
    evidence.initial.activatedStatus = { state: active.state, degradation: clone(active.degradation ?? []) };
    const assigned = (await native({command: 'companion_status'})).companions.host;
    evidence.initial.companionAssigned = assigned.instances.some(instance => instance.profileOwned && instance.connected && instance.armed);
    assert.equal(evidence.initial.companionAssigned, true, 'Activation did not assign and arm the online browser companion.');

    const owner = clone(active.owner);
    const companionResource = document.resources.find(resource => resource.kind === 'companion');
    assert.ok(companionResource, 'DCS Feature Lab needs its actual companion block.');
    const feedbackPorts = companionResource.ports.filter(port => port.direction === 'input').map(port => port.id);
    assert.ok(feedbackPorts.length > 0, 'The companion block has no feedback inputs.');
    const knownPorts = new Set();
    const lastKnown = new Map(feedbackPorts.map(port => [port, Date.now()]));
    const feedbackStarted = Date.now();
    const inspectFeedback = async () => {
      const inspection = (await native({command: 'profile_runtime_inspect', owner, nodeId: companionResource.id, offset: 0, limit: 64})).profileInspection;
      const ports = inspection.ports.filter(port => feedbackPorts.includes(port.endpoint.portId));
      for (const port of ports) if (port.sample.status === 'known') {
        knownPorts.add(port.endpoint.portId);
        lastKnown.set(port.endpoint.portId, Date.now());
      }
      for (const port of feedbackPorts) assert.ok(Date.now() - lastKnown.get(port) < 20_000,
        `Companion feedback ${port} has not recovered a known native value for 20 seconds.`);
      evidence.companion.samples.push({capturedUs: inspection.capturedUs, ports: clone(ports)});
      evidence.companion.knownPorts = [...knownPorts];
      if (Date.now() - feedbackStarted >= 5_000 && ports.some(port => port.sample.status !== 'known')) {
        evidence.companion.unavailableAfterWarmup.push({capturedUs: inspection.capturedUs,
          ports: ports.filter(port => port.sample.status !== 'known').map(port => ({id: port.endpoint.portId, status: port.sample.status}))});
      }
      return ports.length === feedbackPorts.length && ports.every(port => port.sample.status === 'known');
    };
    const endAt = Date.now() + soakMs;
    let nextGamePoll = Date.now();
    let nextStatusPoll = Date.now();
    let nextFeedbackPoll = Date.now();
    while (Date.now() < endAt) {
      const now = Date.now();
      if (now >= nextStatusPoll) {
        const started = Date.now();
        const current = await status();
        recordLatency(evidence.statusPolls, started);
        assert.ok(!['faulted', 'recovery-needed', 'stopped'].includes(current.state), `Profile entered ${current.state}.`);
        assert.deepEqual(current.owner, owner, 'Profile owner changed during the activation soak.');
        assert.equal(current.fault, null, 'Profile reported a fault during the soak.');
        assert.ok(['running', 'degraded'].includes(current.state), `Unexpected active state ${current.state}.`);
        evidence.statusPolls.states[current.state] = (evidence.statusPolls.states[current.state] ?? 0) + 1;
        nextStatusPoll = Date.now() + 150;
      }
      if (Date.now() >= nextGamePoll) {
        const started = Date.now();
        const currentGames = await games();
        recordLatency(evidence.gamePolls, started);
        const dcs = assertDcsRunning(currentGames);
        evidence.gamePolls.last = dcs;
        nextGamePoll = Date.now() + 1_500;
      }
      if (Date.now() >= nextFeedbackPoll) {
        await inspectFeedback();
        nextFeedbackPoll = Date.now() + 500;
      }
      await sleep(Math.max(0, Math.min(nextStatusPoll, nextGamePoll, nextFeedbackPoll, endAt) - Date.now()));
    }

    const finalStatusStarted = Date.now();
    const finalStatus = await status();
    recordLatency(evidence.statusPolls, finalStatusStarted);
    const finalGamesStarted = Date.now();
    const finalGames = await games();
    recordLatency(evidence.gamePolls, finalGamesStarted);
    const finalDcs = assertDcsRunning(finalGames);
    const companion = (await native({command: 'companion_status'})).companions.host;
    evidence.companion.connected = companion.clients.some(client => client.kind === 'browser' && client.connected);
    evidence.companion.panelAssigned = companion.instances.length > 0;
    assert.equal(evidence.companion.connected, true, 'The actual browser companion is disconnected.');
    assert.equal(evidence.companion.panelAssigned, true, 'The profile has no active companion assignment.');
    const feedbackDeadline = Date.now() + 10_000;
    while (!await inspectFeedback()) {
      assert.ok(Date.now() < feedbackDeadline, 'Final companion feedback did not recover known native values.');
      await sleep(200);
    }
    assert.deepEqual([...knownPorts].sort(), feedbackPorts.sort(), 'Companion feedback did not receive known native values for every input.');
    assert.deepEqual(evidence.companion.unavailableAfterWarmup, [], 'Stable companion feedback flickered unavailable after warmup.');
    assert.deepEqual(finalStatus.owner, owner, 'Final profile owner differs from the activated owner.');
    assert.ok(['running', 'degraded'].includes(finalStatus.state));
    assert.equal(finalStatus.fault, null);
    assert.equal(finalStatus.owner.gameId, 'dcs');
    evidence.final = {
      state: finalStatus.state,
      owner: clone(finalStatus.owner),
      degradation: clone(finalStatus.degradation ?? []),
      dcs: finalDcs,
      observedAt: new Date().toISOString(),
    };
  } catch (error) {
    evidence.failure = String(error?.stack ?? error);
    evidence.failedStatus = await status().catch(() => null);
    throw error;
  } finally {
    evidence.completedAt = new Date().toISOString();
    await writeFile(resolve(artifacts, 'evidence.json'), `${JSON.stringify(evidence, null, 2)}\n`);
    await browser.close();
  }
});
