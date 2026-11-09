import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {until} from '../../scripts/reactive/cdp.mjs';

const channel = 'flight.ownship.inAir';

async function sharedDcsEndpoints() {
  const ports = [5010, 7781];
  const {stdout} = await promisify(execFile)('powershell.exe', [
    '-NoProfile', '-NonInteractive', '-Command',
    `$ErrorActionPreference='Stop'; $endpoints=@(Get-NetUDPEndpoint -LocalPort ${ports.join(',')} |
      Select-Object LocalPort,LocalAddress,OwningProcess); ConvertTo-Json -InputObject $endpoints -Compress`,
  ], {timeout:5000, windowsHide:true});
  const endpoints = JSON.parse(stdout);
  assert.deepEqual(endpoints.map(row => row.LocalPort).sort((a,b) => a-b), ports,
    'Profile, inspector and panels must share one receive endpoint per DCS source');
  assert.equal(new Set(endpoints.map(row => row.OwningProcess)).size, 1,
    'Both endpoints must belong to the same Cockpit host');
  return endpoints;
}

// Run the same source, Cockpit observation and browser panel lifecycle through
// native IPC or browser-dev HTTP. Only the first panel is replaced.
export async function verifyDcsTransportLifecycle(cdp, panels, submitOwnship) {
  assert.equal(panels.length, 2, 'Use a paired setup with browser-dev or native combined MCP');
  const [reopening, peer] = panels.map(panel => panel.page);
  const lamp = page => page.evaluate(() => window.__combinedLamp);
  const matching = (page, after, value) => until(async () => (await lamp(page))
    .slice(after).find(row => row.status === 'fresh' && row.value === value));
  const inspect = id => cdp.evaluate(`window.__dcsLifecycle.inspect(${JSON.stringify(id)})`);
  const open = () => cdp.evaluate('window.__dcsLifecycle.open()');
  const state = () => cdp.evaluate('window.__dcsLifecycle.state()');
  const identity = ({source, session, generation}) => ({source, session, generation});
  const waitValue = (id, expected, after) => {
    let revision = after;
    return until(async () => {
      const {last, error} = await state();
      assert.equal(error, null);
      if (!last || BigInt(last.revision) <= BigInt(revision)) return null;
      revision = last.revision;
      const row = await inspect(id);
      return row.status === 'known' && row.value === expected ? {frame:last, value:row.value} : null;
    });
  };
  await cdp.evaluate(`(async () => {
    const {SharedObservations} = await import('/src/lib/streams/shared.ts');
    const {gatewayRequest} = await import('/src/lib/gateway.ts');
    const topic = 'games.telemetry', reader = new SharedObservations();
    let first, last, error, retire;
    window.__dcsLifecycle = {
      open() {
        first = last = undefined;
        error = null;
        retire = reader.subscribe(topic, state => {
          if (state.status === 'ready') { first ??= state.frame; last = state.frame; }
          if (state.status === 'error' || state.status === 'lost') error = state.error;
        });
      },
      retire() { retire(); },
      close() { reader.close(); },
      state() { return {first, last, error, id:reader.subscriptionId(topic)}; },
      async inspect(observationId) {
        try {
          const {gameTelemetry} = await gatewayRequest({command:'game_telemetry_inspect',
            gameId:'dcs', search:${JSON.stringify(channel)}, offset:0, limit:8, observationId});
          return gameTelemetry.games[0].channels.find(row => row.id === ${JSON.stringify(channel)});
        } catch (error) { return {error:error.message}; }
      },
    };
  })()`);
  try {
    const initial = await Promise.all([reopening, peer].map(page => until(async () => {
      const rows = await lamp(page);
      const last = rows.at(-1);
      return last?.status === 'fresh' && last.value === true ? rows.length : null;
    })));
    await submitOwnship(false);
    await open();
    const first = await until(state, value => value.id);
    assert.equal(first.first.kind, 'snapshot');
    const baseline = await waitValue(first.id, false, '0');
    const [originalPanel, originalPeer] = await Promise.all([
      matching(reopening, initial[0], false), matching(peer, initial[1], false),
    ]);
    const before = await Promise.all([lamp(reopening), lamp(peer)]);
    const firstChange = await submitOwnship(true);
    const [changed, panelChange, peerChange] = await Promise.all([
      waitValue(first.id, true, baseline.frame.revision),
      matching(reopening, before[0].length, true), matching(peer, before[1].length, true),
    ]);
    // Telemetry publishes invalidation snapshots. The scoped read supplies
    // the value, while companion feedback carries value deltas directly.
    assert.equal(changed.frame.kind, 'snapshot');
    assert.deepEqual(identity(changed.frame), identity(baseline.frame));
    for (const [sample, original] of [[panelChange, originalPanel], [peerChange, originalPeer]]) {
      assert.equal(sample.mode, 'delta');
      assert.equal(sample.sessionId, original.sessionId);
      assert.ok(BigInt(sample.sequence) > BigInt(original.sequence));
    }

    const address = reopening.url();
    await cdp.evaluate('window.__dcsLifecycle.retire()');
    await until(() => inspect(first.id), row => row.error === 'The telemetry observation has closed.');
    await reopening.goto('about:blank');
    const peerOffset = (await lamp(peer)).length;
    const offlineChange = await submitOwnship(false);
    const uninterrupted = await matching(peer, peerOffset, false);
    assert.equal(uninterrupted.sessionId, originalPeer.sessionId);
    assert.equal(uninterrupted.mode, 'delta');
    assert.equal((await state()).id, undefined);

    await open();
    await reopening.goto(address, {waitUntil:'domcontentloaded'});
    const replacement = await until(state, value => value.id);
    assert.notEqual(replacement.id, first.id);
    assert.equal(replacement.first.kind, 'snapshot');
    assert.ok(BigInt(replacement.first.revision) >= BigInt(changed.frame.revision));
    const fresh = await waitValue(replacement.id, false, '0');
    const newPanel = await matching(reopening, 0, false);
    const acquisition = await sharedDcsEndpoints();
    assert.notEqual(newPanel.sessionId, originalPanel.sessionId);
    assert.equal(newPanel.publicationHash, originalPanel.publicationHash);
    const reopenedRows = await lamp(reopening);
    const panelBaseline = reopenedRows[0];
    assert.equal(panelBaseline.mode, 'snapshot');
    assert.equal(panelBaseline.sessionId, newPanel.sessionId);
    assert.ok(BigInt(panelBaseline.sequence) <= BigInt(newPanel.sequence));
    const replayed = reopenedRows.slice(0, reopenedRows.findIndex(row =>
      row.sessionId === newPanel.sessionId && row.sequence === newPanel.sequence) + 1);
    assert.ok(replayed.length > 0 && replayed.every(row => row.sessionId === newPanel.sessionId && row.value !== true),
      'Reopening must not replay the previous session or its lit value');

    const after = await Promise.all([lamp(reopening), lamp(peer)]);
    const nextChange = await submitOwnship(true);
    const [next, newDelta, peerDelta] = await Promise.all([
      waitValue(replacement.id, true, fresh.frame.revision),
      matching(reopening, after[0].length, true), matching(peer, after[1].length, true),
    ]);
    assert.equal(next.frame.kind, 'snapshot');
    assert.deepEqual(identity(next.frame), identity(fresh.frame));
    assert.equal(newDelta.mode, 'delta');
    assert.equal(newDelta.sessionId, newPanel.sessionId);
    assert.ok(BigInt(newDelta.sequence) > BigInt(newPanel.sequence));
    assert.equal(peerDelta.sessionId, originalPeer.sessionId);
    assert.ok(BigInt(peerDelta.sequence) > BigInt(uninterrupted.sequence));
    const restored = await Promise.all([lamp(reopening), lamp(peer)]);
    await submitOwnship(false);
    await Promise.all([matching(reopening, restored[0].length, false), matching(peer, restored[1].length, false)]);
    return {first, baseline, changed, replacement, fresh, next, originalPanel, panelBaseline, newPanel, peerDelta,
      firstChange, offlineChange, nextChange, acquisition, oldScopeRejected:true};
  } finally {
    await cdp.evaluate('window.__dcsLifecycle.close(); delete window.__dcsLifecycle');
  }
}
