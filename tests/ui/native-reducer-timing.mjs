import assert from 'node:assert/strict';
import {summarizeMilliseconds} from './vjoy-rate-transport.mjs';

export async function openReducerTiming(cdp, {source, port, minimumSamples, packetCounter = false}) {
  const invoke = request => cdp.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{
    request:${JSON.stringify(request)}
  })`);
  const status = await invoke({command:'profile_status', clientId:'native-reducer-timing'});
  assert.equal(status.ok, true, status.error);
  const {owner} = status.state.profiles.profileSession;
  const command = 'qualification_native_timing';
  await invoke({command, action:'start', owner, source, port});
  return async () => {
    const {rows} = await invoke({command, action:'finish', owner});
    return summarizeReducerTiming(rows, {source, port, minimumSamples, packetCounter});
  };
}

export function summarizeReducerTiming(rows, {source, port, minimumSamples, packetCounter}) {
  assert.ok(rows.length >= minimumSamples, `Expected ${minimumSamples} captures on ${source}/${port}, received ${rows.length}.`);
  const durations = [], cadence = [];
  let gaps = 0n;
  for (const [index, row] of rows.entries()) {
    const elapsed = BigInt(row.appliedUs) - BigInt(row.capturedUs);
    assert.ok(elapsed >= 0n);
    durations.push(Number(elapsed) / 1000);
    if (packetCounter) assert.ok(Number.isSafeInteger(row.value));
    if (index === 0) continue;
    const previous = rows[index - 1];
    assert.equal(row.clockId, previous.clockId);
    assert.equal(row.producer, previous.producer);
    assert.equal(row.generation, previous.generation);
    const sequenceStep = BigInt(row.sequence) - BigInt(previous.sequence);
    const interval = BigInt(row.capturedUs) - BigInt(previous.capturedUs);
    assert.ok(sequenceStep > 0n && interval > 0n);
    // An Assetto host sequence advances across every page copy, including
    // both copies of a stability check. Only the publisher counter tracks packets.
    const step = packetCounter ? BigInt(row.value) - BigInt(previous.value) : sequenceStep;
    assert.ok(step > 0n);
    gaps += step - 1n;
    cadence.push(Number(interval) / 1000);
  }
  const gapName = packetCounter ? 'publisherPacketGaps' : 'captureSequenceGaps';
  return {source, port, rows, [gapName]:String(gaps), captureIntervalsMs:summarizeMilliseconds(cadence),
    captureToReducerBatchMs:summarizeMilliseconds(durations)};
}
