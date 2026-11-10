import assert from 'node:assert/strict';
import {mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {summarizeDualRateTrace} from './dcs-dual-rate-soak.mjs';

const ms = 1_000_000n;

function sourceTrace(times, lane = 0) {
  const bytes = Buffer.alloc(8 + times.length * 56);
  bytes.write('OSGDBG02');
  times.forEach((time, index) => {
    const offset = 8 + index * 56;
    bytes.writeBigUInt64LE(time, offset);
    bytes.writeBigUInt64LE(time+100_000n, offset+8);
    bytes.writeBigUInt64LE(time+200_000n, offset+16);
    bytes.writeBigUInt64LE(time+310_000n, offset+24);
    bytes.writeBigUInt64LE(time+400_000n, offset+32);
    bytes.writeUInt32LE(index+1, offset+40);
    bytes.writeUInt32LE(2 + lane, offset+44);
    bytes.writeUInt32LE(1280 + lane * 64, offset+48);
    bytes.writeUInt32LE(index % 3, offset+52);
  });
  return bytes;
}

function receiverTrace(events) {
  const bytes = Buffer.alloc(24 + events.length * 14);
  bytes.write('OSGVJOY1');
  bytes.writeBigUInt64LE(1_000_000_000n,8);
  bytes.writeBigInt64LE(ms,16);
  events.forEach(([time, buttons], index) => {
    const offset = 24 + index * 14;
    bytes[offset] = 1;
    bytes[offset+1] = buttons;
    bytes.writeInt32LE(0x00110000, offset+2);
    bytes.writeBigInt64LE(time * ms, offset+6);
  });
  return bytes;
}

function acceptanceTrace(rows) {
  const bytes = Buffer.alloc(8 + rows.length * 18);
  bytes.write('OSGVJAC1');
  rows.forEach(([mask, entry, returned], index) => {
    const offset = 8 + index * 18;
    bytes[offset] = 1;
    bytes[offset+1] = mask;
    bytes.writeBigUInt64LE(entry, offset+2);
    bytes.writeBigUInt64LE(returned, offset+10);
  });
  return bytes;
}

test('bounded dual-rate trace matches both lanes and requires releases after Stop', () => {
  const directory = mkdtempSync(join(tmpdir(), 'dual-rate-trace-'));
  try {
    for (const lane of ['fast', 'peer']) {
      writeFileSync(join(directory, `qualification-${lane}.bin`), sourceTrace([2n*ms, 3n*ms, 4n*ms], lane === 'peer' ? 1 : 0));
    }
    const receiver = join(directory, 'vjoy-raw.bin');
    const raw = {traceEvents:5, traceSlot:1, traceDevice:'fixture-vjoy', captureOverflow:false,
      stopRequested:true, receiverStartedTicks:String(ms), receiverEndedTicks:String(10n*ms), receiverTickHz:1e9};
    writeFileSync(receiver, receiverTrace([[1n,0],[3n,3],[4n,0],[5n,3],[8n,0]]));
    const accepted = join(directory, 'vjoy-acceptance-123.bin');
    const acceptedRows = [[3,2_500_000n,3_200_000n],[0,3_500_000n,3_800_000n],
      [3,4_500_000n,5_100_000n],[0,7_500_000n,7_600_000n]];
    writeFileSync(accepted, acceptanceTrace(acceptedRows));
    const result = summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms, accepted);
    assert.equal(result.overlapMs, 2);
    assert.deepEqual(result.lanes.map(lane => [lane.received,lane.offerToReceiptMs.p95,lane.stopReleaseMs]),
      [[4,1,1],[4,1,1]]);
    assert.deepEqual(result.lanes.map(lane => [lane.captureToReceiptLowerMs.p95, lane.captureToReceiptUpperMs.p95,
      lane.captureBracketMs.max, lane.captureToAdmissionUpperMs.p95, lane.sampledSharedPendingFramesPeak,
      lane.sampledSharedPendingOwnedBytesPeak]),
    [[.6,.8,.11,.2,2,1280],[.6,.8,.11,.2,3,1344]]);
    assert.deepEqual(result.sampledSharedQueuePeaks, {pendingFrames:3,pendingOwnedBytes:1344});
    assert.deepEqual(result.lanes.map(({driverAcceptance:driver}) => [driver.captureToDllReturnUpperMs.p50,
      driver.captureToCallEntryUpperMs.p50, driver.dllCallDurationMs.p50, driver.apiToRawInputUpperMs.p50]),
    [[.9,.3,.6,.5],[.9,.3,.6,.5]]);
    assert.deepEqual(result.lanes.map(({driverAcceptance:driver}) => [driver.apiToRawInputLowerMs.max,
      driver.apiToRawInputUpperMs.max,driver.receivedBeforeReturn,driver.stopToDllReturnMs]),
    [[.2,.5,2,.6],[.2,.5,2,.6]]);
    assert.equal(summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms).lanes[0].driverAcceptance, undefined);
    const missing = join(directory, 'missing.bin');
    writeFileSync(missing, acceptanceTrace(acceptedRows.slice(0,-1)));
    assert.throws(() => summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms, missing), /Missing accepted driver edges/);
    const extra = join(directory, 'extra.bin');
    writeFileSync(extra, acceptanceTrace([...acceptedRows,[1,8_100_000n,8_200_000n]]));
    assert.throws(() => summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms, extra), /Unexpected extra accepted driver edge/);
    const unordered = join(directory, 'unordered.bin');
    writeFileSync(unordered, acceptanceTrace([[3,3_500_000n,2_500_000n]]));
    assert.throws(() => summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms, unordered), /timestamps must be ordered/);
    const earlyReceipt = join(directory, 'early-receipt.bin');
    writeFileSync(earlyReceipt, acceptanceTrace([[3,3_100_000n,3_200_000n],...acceptedRows.slice(1)]));
    assert.throws(() => summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms, earlyReceipt), /Raw Input must follow/);
    writeFileSync(receiver, receiverTrace([[1n,0],[3n,3],[4n,0],[5n,3],[6n,0]]));
    assert.throws(() => summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms), /release after Stop/);
    const invalidSequence = sourceTrace([2n*ms, 3n*ms, 4n*ms]);
    invalidSequence.writeUInt32LE(8, 8+40);
    writeFileSync(join(directory, 'qualification-fast.bin'), invalidSequence);
    writeFileSync(receiver, receiverTrace([[1n,0],[3n,3],[4n,0],[5n,3],[8n,0]]));
    assert.throws(() => summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms), /exact per-lane request sequence/);
    const reversedCapture = sourceTrace([2n*ms, 3n*ms, 4n*ms]);
    reversedCapture.writeBigUInt64LE(9n*ms, 8+16);
    writeFileSync(join(directory, 'qualification-fast.bin'), reversedCapture);
    assert.throws(() => summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms), /timestamps must be ordered/);
    const oversizedQueue = sourceTrace([2n*ms, 3n*ms, 4n*ms]);
    oversizedQueue.writeUInt32LE(257, 8+44);
    writeFileSync(join(directory, 'qualification-fast.bin'), oversizedQueue);
    assert.throws(() => summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms), /pending frame sample is out of range/);
    const missingBytes = sourceTrace([2n*ms, 3n*ms, 4n*ms]);
    missingBytes.writeUInt32LE(0, 8+48);
    writeFileSync(join(directory, 'qualification-fast.bin'), missingBytes);
    assert.throws(() => summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms), /queue byte sample is missing/);
  } finally {
    rmSync(directory, {recursive:true,force:true});
  }
});
