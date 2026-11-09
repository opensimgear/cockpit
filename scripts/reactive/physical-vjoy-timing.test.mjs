import assert from 'node:assert/strict';
import test from 'node:test';
import {physicalVjoyTiming} from './physical-vjoy-timing.mjs';

function fixture(preflight = false) {
  const origin = 10_000_000;
  const clock = nowUs => ({
    beforeNs: String(origin + (nowUs - 1) * 1000 + 100),
    afterNs: String(origin + (nowUs - 1) * 1000 + 200),
    nowUs: String(nowUs),
  });
  const edges = Array.from({length: preflight ? 0 : 20}, (_, index) => {
    const read = 1200 + index * 200;
    return {sequence: String(index + 6), nativeReadUs: String(read), capturedUs: String(read + 15), pressed: index % 2 === 0};
  });
  const receiverStart = 10_500_000;
  const receipt = (ns, pressed) => ({report: 2, device: 'exact-device', pressed, peerPressed: false, status: 0x00110000,
    receivedTicks: ns - receiverStart});
  const trace = {clockBefore: clock(999), clockAfter: clock(8001), startUs: '1000', endUs: '8000',
    firstSequence: '5', lastSequence: '40', finalPressed: false, edges};
  const raw = {captureOverflow: false, receiverTickHz: 1_000_000_000, receiverStartedTicks: String(receiverStart),
    receiverEndedTicks: '20000000', devices: [{id: 'exact-device', virtualVjoy: true}],
    buttonEvents: [receipt(10_700_000, false),
      ...edges.map(edge => receipt(origin + Number(edge.nativeReadUs) * 1000 + 900_000, edge.pressed))]};
  return {trace, raw, options: {slot: 2, beforeProbeNs: 10_400_000n, probeReadyNs: 10_600_000n,
    stopNs: 19_000_000n, preflight}};
}

test('joins each ordered edge and reports bounded read-to-receipt timing', () => {
  const {trace, raw, options} = fixture();
  const result = physicalVjoyTiming(trace, raw, options);
  assert.equal(result.device, 'exact-device');
  assert.equal(result.edges.length, 20);
  assert.equal(result.rawReports, '35');
  assert.equal(result.nativeReadToJournalMs.p95, 0.015);
  assert.ok(result.nativeReadToReceiverLowerMs.p95 <= 0.901 && result.nativeReadToReceiverUpperMs.p95 >= 0.900);
  assert.ok(result.clockUncertaintyMs < 0.003);
  assert.equal(result.physicalLatencyMeasured, true);
  assert.equal(result.destinationBudgetPassed, true);
});

test('stationary preflight only verifies a neutral route and shared clocks', () => {
  const {trace, raw, options} = fixture(true);
  const result = physicalVjoyTiming(trace, raw, options);
  assert.equal(result.edges.length, 0);
  assert.equal(result.nativeReadToReceiverUpperMs, null);
  assert.equal(result.physicalLatencyMeasured, false);
  assert.equal(result.destinationBudgetPassed, null);
});

test('sub-microsecond uncertainty permits a zero lower bound and slow receipts miss the budget', () => {
  const {trace, raw, options} = fixture();
  raw.buttonEvents[1].receivedTicks = 700_000;
  const bounded = physicalVjoyTiming(trace, raw, options);
  assert.equal(bounded.edges[0].nativeReadToReceiverLowerMs, 0);
  assert.ok(bounded.edges[0].nativeReadToReceiverUpperMs > 0);

  const late = fixture();
  for (const receipt of late.raw.buttonEvents.slice(1)) receipt.receivedTicks += 60_000_000;
  late.raw.receiverEndedTicks = '100000000';
  late.options.stopNs = 90_000_000n;
  assert.equal(physicalVjoyTiming(late.trace, late.raw, late.options).destinationBudgetPassed, false);
});

test('rejects missing or extra receipts, overflow, and device replacement', () => {
  for (const corrupt of [
    raw => raw.buttonEvents.pop(),
    raw => raw.buttonEvents.push({...raw.buttonEvents.at(-1)}),
    raw => { raw.captureOverflow = true; },
    raw => { raw.buttonEvents[3].device = 'another-device'; },
    raw => { raw.buttonEvents[3].status = 0; },
    raw => { raw.buttonEvents[3].peerPressed = true; },
  ]) {
    const {trace, raw, options} = fixture();
    corrupt(raw);
    assert.throws(() => physicalVjoyTiming(trace, raw, options));
  }
});

test('rejects unrelated clocks, incomplete time windows, and a receipt preceding the read', () => {
  for (const corrupt of [
    trace => { trace.clockAfter.beforeNs = '100000000'; trace.clockAfter.afterNs = '100000100'; },
    trace => { trace.endUs = '20000'; },
    trace => { trace.edges[3].sequence = trace.edges[2].sequence; },
  ]) {
    const {trace, raw, options} = fixture();
    corrupt(trace);
    assert.throws(() => physicalVjoyTiming(trace, raw, options));
  }
  const {trace, raw, options} = fixture();
  raw.buttonEvents[1].receivedTicks = 400_000;
  assert.throws(() => physicalVjoyTiming(trace, raw, options), /predates its physical read/);
});
