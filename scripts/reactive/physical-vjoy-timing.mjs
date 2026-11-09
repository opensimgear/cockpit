import assert from 'node:assert/strict';
import {summarizeMilliseconds} from '../../tests/ui/vjoy-rate-transport.mjs';

const usInNs = 1_000n;
const nsInSecond = 1_000_000_000n;
const decoded = 0x00110000;
const milliseconds = ns => Number(ns) / 1e6;

function counter(value) {
  assert.match(value, /^(?:0|[1-9][0-9]*)$/);
  return BigInt(value);
}

// The journal emits floor(elapsed in µs) + 1. The enclosing QPC samples
// bound its origin even when the journal value is rounded to a microsecond.
function origin(sample) {
  const before = counter(sample.beforeNs);
  const after = counter(sample.afterNs);
  const us = counter(sample.nowUs);
  assert.ok(us > 0n && before <= after, 'Invalid journal clock bracket');
  return {low: before - us * usInNs, high: after - (us - 1n) * usInNs};
}

export function physicalVjoyTiming(trace, raw, {slot, beforeProbeNs, probeReadyNs, stopNs, preflight = false}) {
  assert.equal(raw.captureOverflow, false, 'The Raw Input probe overflowed');
  assert.ok(Number.isSafeInteger(raw.receiverTickHz) && raw.receiverTickHz > 0);
  const ticksToNs = ticks => ticks * nsInSecond / BigInt(raw.receiverTickHz);
  const startTicks = counter(raw.receiverStartedTicks);
  const startNs = ticksToNs(startTicks);
  const endNs = ticksToNs(counter(raw.receiverEndedTicks));
  assert.ok(startNs >= beforeProbeNs && startNs <= probeReadyNs && endNs > stopNs,
    'The receiver must share Windows QPC and remain active through Stop');

  const first = origin(trace.clockBefore), last = origin(trace.clockAfter);
  const low = first.low > last.low ? first.low : last.low;
  const high = first.high < last.high ? first.high : last.high;
  assert.ok(low <= high && high - low <= 1_000_000n, 'The journal and QPC clocks do not agree closely enough');
  const captureStart = counter(trace.startUs);
  const captureEnd = counter(trace.endUs);
  assert.ok(captureStart < captureEnd && low + (captureStart - 1n) * usInNs >= startNs
    && high + captureEnd * usInNs <= stopNs, 'Physical capture must be contained in the receiver and profile lifetime');
  assert.equal(trace.finalPressed, false, 'Release the selected button before the capture ends');
  let sequence = counter(trace.firstSequence);
  const lastSequence = counter(trace.lastSequence);
  assert.ok(lastSequence > sequence, 'The physical source sent no native reports');

  const records = raw.buttonEvents.filter(event => event.report === slot);
  const device = records[0]?.device;
  assert.ok(device && raw.devices.some(row => row.id === device && row.virtualVjoy === true),
    'The exact vJoy slot must have an observed native baseline');
  let received = startNs;
  const receiptTimes = records.map(event => {
    assert.equal(event.device, device, 'The slot must resolve to a single Raw Input device');
    assert.equal(event.status, decoded, 'HID button decoding failed');
    assert.equal(event.peerPressed, false, 'The test profile owns only vJoy button 1');
    assert.ok(Number.isSafeInteger(event.receivedTicks) && event.receivedTicks >= 0);
    const at = ticksToNs(startTicks + BigInt(event.receivedTicks));
    assert.ok(at >= received && at < stopNs, 'The slot trace is unordered or contains a Stop edge');
    received = at;
    return at;
  });
  assert.equal(records[0].pressed, false, 'A neutral receiver baseline is required');
  assert.deepEqual(records.slice(1).map(event => event.pressed), trace.edges.map(edge => edge.pressed),
    'Every physical edge must match exactly one ordered vJoy receipt');
  assert.ok(preflight ? trace.edges.length === 0 : trace.edges.length >= 20,
    preflight ? 'Keep the button neutral during preflight' : 'At least twenty physical transitions are required');

  let level = false;
  let lastRead = captureStart;
  const edges = trace.edges.map((edge, index) => {
    const seq = counter(edge.sequence);
    const read = counter(edge.nativeReadUs);
    const captured = counter(edge.capturedUs);
    assert.ok(seq > sequence && seq <= lastSequence
      && read >= lastRead && read <= captured && captured <= captureEnd && edge.pressed === !level,
      'Physical evidence is out of order or outside the trace');
    sequence = seq;
    level = edge.pressed;
    lastRead = read;
    const receipt = receiptTimes[index + 1];
    const elapsedLow = receipt - (high + read * usInNs);
    const elapsedHigh = receipt - (low + (read - 1n) * usInNs);
    assert.ok(elapsedHigh >= 0n, 'A vJoy receipt predates its physical read');
    return {
      sequence: edge.sequence, pressed: edge.pressed, nativeReadUs: edge.nativeReadUs,
      capturedUs: edge.capturedUs, receiverQpcNs: receipt.toString(),
      nativeReadToJournalMs: milliseconds((captured - read) * usInNs),
      nativeReadToReceiverLowerMs: milliseconds(elapsedLow < 0n ? 0n : elapsedLow),
      nativeReadToReceiverUpperMs: milliseconds(elapsedHigh),
    };
  });
  assert.equal(level, false, 'The final observed level must be neutral');
  const summary = key => edges.length ? summarizeMilliseconds(edges.map(edge => edge[key])) : null;
  const upper = summary('nativeReadToReceiverUpperMs');
  return {
    device, slot, edges, clockUncertaintyMs: milliseconds(high - low + usInNs),
    rawReports: (lastSequence - counter(trace.firstSequence)).toString(),
    nativeReadToJournalMs: summary('nativeReadToJournalMs'),
    nativeReadToReceiverLowerMs: summary('nativeReadToReceiverLowerMs'),
    nativeReadToReceiverUpperMs: upper,
    physicalLatencyMeasured: !preflight,
    destinationBudgetPassed: upper === null ? null : upper.p95 <= 20 && upper.p99 <= 50,
  };
}
