import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {closeSync, fstatSync, openSync, readSync} from 'node:fs';
import {resolve} from 'node:path';
import {createInterface} from 'node:readline';
import {setTimeout as delay} from 'node:timers/promises';
import {dualRateLanes} from './vjoy-rate-transport.mjs';

const nsPerMs = 1_000_000n;
const hidSuccess = 0x00110000;
const sourceFiles = ['qualification-fast.bin', 'qualification-peer.bin'];

function records(path, width, headerSize, count) {
  const file = openSync(path, 'r');
  try {
    assert.equal(fstatSync(file).size, headerSize + count * width, `${path} trace size`);
    const header = Buffer.alloc(headerSize);
    assert.equal(readSync(file, header), headerSize);
    const chunk = Buffer.allocUnsafe(width * 4096);
    let remaining = count, cursor = 0, available = 0;
    return {
      header,
      next() {
        if (!remaining) return null;
        if (cursor === available) {
          available = Math.min(remaining, 4096) * width;
          let offset = 0;
          while (offset < available) {
            const bytes = readSync(file, chunk, offset, available - offset, null);
            assert.ok(bytes, `Unexpected EOF in ${path}`);
            offset += bytes;
          }
          cursor = 0;
        }
        const row = chunk.subarray(cursor, cursor + width);
        cursor += width;
        remaining--;
        return row;
      },
      close: () => closeSync(file),
    };
  } catch (error) {
    closeSync(file);
    throw error;
  }
}

function timingDistribution() {
  // One tenth of a millisecond bins. The overflow bucket reports its upper
  // bound as the exact maximum, so it cannot conceal a budget miss.
  const buckets = new Uint32Array(5001);
  let count = 0, max = 0;
  return {
    add(ns) {
      assert.ok(ns >= 0n, 'Timing must be monotonic');
      const ms = Number(ns) / 1e6;
      buckets[Math.min(Math.ceil(ms * 10), buckets.length - 1)]++;
      count++;
      max = Math.max(max, ms);
    },
    summarize() {
      const at = fraction => {
        const rank = Math.ceil(count * fraction);
        let cumulative = 0;
        for (const [index, value] of buckets.entries()) {
          cumulative += value;
          if (cumulative >= rank) return index === buckets.length - 1 ? max : Math.min(max, index / 10);
        }
        throw new Error('Trace percentile is missing');
      };
      assert.ok(count, 'Trace distribution is empty');
      return {count, p50:at(.50), p95:at(.95), p99:at(.99), max};
    },
  };
}

export function summarizeDualRateTrace(directory, slot, counts, raw, stopNs) {
  const readers = [];
  const open = (...args) => {
    const reader = records(...args);
    readers.push(reader);
    return reader;
  };
  try {
    assert.equal(raw.captureOverflow, false);
    assert.equal(raw.stopRequested, true);
    assert.equal(raw.traceSlot, slot);
    assert.ok(raw.traceDevice, 'Expected one vJoy device identity');
    assert.ok(Number.isInteger(raw.traceEvents) && raw.traceEvents <= counts[0] + counts[1] + 3);
    const receiver = open(resolve(directory, 'vjoy-raw.bin'), 14, 24, raw.traceEvents);
    assert.equal(receiver.header.toString('ascii', 0, 8), 'OSGVJOY1');
    const hz = receiver.header.readBigUInt64LE(8);
    const origin = receiver.header.readBigInt64LE(16);
    assert.equal(hz, BigInt(raw.receiverTickHz));
    assert.equal(origin, BigInt(raw.receiverStartedTicks));
    const qpcNs = ticks => ticks * 1_000_000_000n / hz;
    const end = BigInt(raw.receiverEndedTicks);
    assert.ok(qpcNs(end) > stopNs, 'Receiver must stay active through Stop');
    const lanes = counts.map((count,index) => ({count, index,
      source:open(resolve(directory,sourceFiles[index]),32,0,count),
      offerToReceipt:timingDistribution(), offerToDispatch:timingDistribution(),
      dispatchToAdmission:timingDistribution(), offerIntervals:timingDistribution(), receiverIntervals:timingDistribution(),
      edges:0, pendingFramesPeak:0, batchSizePeak:0,
    }));
    let buttons = 0, ticks = origin;
    for (let event = receiver.next(); event; event = receiver.next()) {
      const nextTicks = event.readBigInt64LE(6);
      assert.ok(nextTicks >= ticks && nextTicks <= end, 'Raw Input timestamps must be monotonic');
      ticks = nextTicks;
      assert.equal(event[0], slot);
      assert.equal(event.readInt32LE(2), hidSuccess);
      const mask = event[1];
      assert.ok(mask <= 3);
      const changed = mask ^ buttons;
      buttons = mask;
      for (const lane of lanes) {
        const bit = 1 << lane.index;
        if (!(changed & bit)) continue;
        const received = qpcNs(ticks);
        assert.equal(Boolean(mask & bit), lane.edges % 2 === 0);
        const offer = lane.source.next();
        if (!offer) {
          assert.equal(lane.edges, lane.count, 'Unexpected extra vJoy edge');
          assert.ok(received >= stopNs && !(mask & bit), 'Expected a release after Stop');
          lane.stopReleaseMs = Number(received - stopNs) / 1e6;
        } else {
          const at = offer.readBigUInt64LE(0);
          const dispatched = offer.readBigUInt64LE(8);
          const admitted = offer.readBigUInt64LE(16);
          const pending = offer.readUInt32LE(24), batchIndex = offer.readUInt32LE(28);
          assert.ok(at >= qpcNs(origin) && received < stopNs && pending > 0 && pending <= 256 && batchIndex < 8);
          lane.offerToReceipt.add(received - at);
          lane.offerToDispatch.add(dispatched - at);
          lane.dispatchToAdmission.add(admitted - dispatched);
          if (lane.lastOffer !== undefined) lane.offerIntervals.add(at - lane.lastOffer);
          if (lane.lastReceipt !== undefined) lane.receiverIntervals.add(received - lane.lastReceipt);
          lane.firstOffer ??= at;
          lane.firstDispatch ??= dispatched;
          lane.firstAdmission ??= admitted;
          lane.lastOffer = at;
          lane.lastDispatch = dispatched;
          lane.lastAdmission = admitted;
          lane.lastReceipt = received;
          lane.pendingFramesPeak = Math.max(lane.pendingFramesPeak, pending);
          lane.batchSizePeak = Math.max(lane.batchSizePeak, batchIndex + 1);
        }
        lane.edges++;
      }
    }
    assert.equal(buttons, 0);
    const results = lanes.map(lane => {
      assert.equal(lane.edges, lane.count + 1, 'Every offered edge and Stop release must reach Raw Input');
      const {nodeId,intervalMs} = dualRateLanes[lane.index];
      const cadence = (first,last) => (lane.count-1)*1e9/Number(last-first);
      return {nodeId, count:lane.count, received:lane.edges, targetHz:1000/intervalMs,
        actualHz:cadence(lane.firstOffer,lane.lastOffer), dispatchHz:cadence(lane.firstDispatch,lane.lastDispatch),
        admissionHz:cadence(lane.firstAdmission,lane.lastAdmission),
        offerToReceiptMs:lane.offerToReceipt.summarize(), offerToDispatchMs:lane.offerToDispatch.summarize(),
        dispatchToAdmissionMs:lane.dispatchToAdmission.summarize(), offerIntervalsMs:lane.offerIntervals.summarize(),
        receiverIntervalsMs:lane.receiverIntervals.summarize(),
        pendingFramesPeak:lane.pendingFramesPeak,batchSizePeak:lane.batchSizePeak,stopReleaseMs:lane.stopReleaseMs};
    });
    const first = lanes[0].firstOffer > lanes[1].firstOffer ? lanes[0].firstOffer : lanes[1].firstOffer;
    const last = lanes[0].lastOffer < lanes[1].lastOffer ? lanes[0].lastOffer : lanes[1].lastOffer;
    return {lanes:results, overlapMs:Number(last-first)/1e6,
      probeAfterStopMs:Number(qpcNs(end)-stopNs)/1e6, traceEvents:raw.traceEvents, device:raw.traceDevice};
  } finally {
    for (const reader of readers) reader.close();
  }
}

export async function startDualRateSoak(cdp, executable, slot, owner, directory, seconds) {
  const durationMs = Math.ceil(seconds * 1000 / 16) * 16;
  const beforeReady = process.hrtime.bigint();
  const probe = spawn(executable, ['--vjoy-trace',resolve(directory,'vjoy-raw.bin'),String(durationMs+110000),String(slot)],
    {windowsHide:true});
  const lines = createInterface({input:probe.stdout});
  const ready = once(lines,'line');
  let summary, probeError = '';
  lines.on('line',line => { if (line !== 'ready') summary=line; });
  probe.stderr.setEncoding('utf8').on('data',chunk => { probeError=(probeError+chunk).slice(-4096); });
  probe.stdin.on('error',() => {});
  const report = once(probe,'close').then(([code]) => {
    assert.equal(code,0,probeError);
    return JSON.parse(summary);
  });
  void report.catch(() => {});
  try {
    const [line] = await Promise.race([ready,report.then(() => { throw Error('Raw Input receiver exited before ready'); }),
      delay(10000,null,{ref:false}).then(() => { throw Error('Raw Input receiver did not signal readiness'); })]);
    const afterReady = process.hrtime.bigint();
    assert.equal(line,'ready');
    const startNs = afterReady + 300_000_000n;
    const endNs = startNs + BigInt(durationMs)*nsPerMs;
    const request = {command:'qualification_debug_burst',owner,nodes:dualRateLanes.map(lane=>lane.nodeId),
      startNs:String(startNs),durationMs,traceDirectory:directory};
    let failure;
    const offered = cdp.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(request)}})`,
      durationMs + 15000).then(reply => {
      assert.equal(reply.ok,true,`qualification_debug_burst: ${reply.error}`);
      const counts = reply.state.qualificationTrace;
      assert.deepEqual(counts,dualRateLanes.map(lane=>durationMs/lane.intervalMs+1));
      const reducer = reply.state.captureToReducerBatchMs;
      assert.deepEqual(reducer.map(row => row.count), counts);
      for (const row of reducer) {
        const timings = [row.p50, row.p95, row.p99, row.max];
        assert.ok(timings.every((ms, index) =>
          Number.isFinite(ms) && ms >= (timings[index - 1] ?? 0)));
      }
      return {counts, reducer};
    });
    void offered.catch(error => { failure=error; });
    return {startNs,endNs,offered,
      check() { if (failure) throw failure; },
      async stopped(stopNs) {
        const {counts, reducer} = await offered;
        probe.stdin.end('stop\n');
        const raw = await report;
        const originNs = BigInt(raw.receiverStartedTicks)*1_000_000_000n/BigInt(raw.receiverTickHz);
        assert.ok(originNs >= beforeReady && originNs <= afterReady,'Raw Input must share the Node monotonic clock');
        const result = summarizeDualRateTrace(directory,slot,counts,raw,stopNs);
        result.lanes.forEach((lane, index) => { lane.captureToReducerBatchMs = reducer[index]; });
        assert.ok(result.overlapMs >= durationMs - 20,'The two native schedules must overlap throughout the soak');
        const budgetMisses = result.lanes.flatMap(lane => {
          const misses=[];
          if (Math.abs(lane.actualHz/lane.targetHz-1)>.05) misses.push(`${lane.nodeId} missed the offer rate`);
          if (lane.offerToReceiptMs.p95>20||lane.offerToReceiptMs.p99>50) misses.push(`${lane.nodeId} missed the Raw Input budget`);
          return misses;
        });
        return {...result,budgetMisses};
      },
      close() { probe.kill(); },
    };
  } catch (error) {
    probe.kill();
    throw error;
  }
}
