import assert from 'node:assert/strict';
import {mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {summarizeDualRateTrace} from './dcs-dual-rate-soak.mjs';

const ms = 1_000_000n;

function sourceTrace(times) {
  const bytes = Buffer.alloc(times.length * 32);
  times.forEach((time, index) => {
    const offset = index * 32;
    bytes.writeBigUInt64LE(time, offset);
    bytes.writeBigUInt64LE(time, offset+8);
    bytes.writeBigUInt64LE(time+ms, offset+16);
    bytes.writeUInt32LE(1, offset+24);
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

test('bounded dual-rate trace matches both lanes and requires releases after Stop', () => {
  const directory = mkdtempSync(join(tmpdir(), 'dual-rate-trace-'));
  try {
    for (const lane of ['fast', 'peer']) {
      writeFileSync(join(directory, `qualification-${lane}.bin`), sourceTrace([2n*ms, 3n*ms, 4n*ms]));
    }
    const receiver = join(directory, 'vjoy-raw.bin');
    const raw = {traceEvents:5, traceSlot:1, traceDevice:'fixture-vjoy', captureOverflow:false,
      stopRequested:true, receiverStartedTicks:String(ms), receiverEndedTicks:String(10n*ms), receiverTickHz:1e9};
    writeFileSync(receiver, receiverTrace([[1n,0],[3n,3],[4n,0],[5n,3],[8n,0]]));
    const result = summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms);
    assert.equal(result.overlapMs, 2);
    assert.deepEqual(result.lanes.map(lane => [lane.received,lane.offerToReceiptMs.p95,lane.stopReleaseMs]),
      [[4,1,1],[4,1,1]]);
    writeFileSync(receiver, receiverTrace([[1n,0],[3n,3],[4n,0],[5n,3],[6n,0]]));
    assert.throws(() => summarizeDualRateTrace(directory, 1, [3,3], raw, 7n*ms), /release after Stop/);
  } finally {
    rmSync(directory, {recursive:true,force:true});
  }
});
