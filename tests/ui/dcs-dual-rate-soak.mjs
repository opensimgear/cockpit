import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {closeSync, copyFileSync, fstatSync, openSync, readdirSync, readSync, statSync} from 'node:fs';
import {resolve} from 'node:path';
import {createInterface} from 'node:readline';
import {setTimeout as delay} from 'node:timers/promises';
import {dualRateLanes} from './vjoy-rate-transport.mjs';

const nsPerMs = 1_000_000n;
const hidSuccess = 0x00110000;
const sourceFiles = ['qualification-fast.bin', 'qualification-peer.bin'];
const sourceHeaderSize = 8;
const sourceRowSize = 56;
const acceptanceHeaderSize = 8;
const acceptanceRowSize = 18;

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

function timingDistribution(roundDown = false, binNs = 100_000n) {
  // The overflow bucket reports its upper bound as the exact maximum.
  const buckets = new Uint32Array(5001);
  let count = 0, max = 0;
  return {
    add(ns) {
      assert.ok(ns >= 0n, 'Timing must be monotonic');
      const ms = Number(ns) / 1e6;
      const bin = Number((ns + (roundDown ? 0n : binNs - 1n)) / binNs);
      buckets[Math.min(bin, buckets.length - 1)]++;
      count++;
      max = Math.max(max, ms);
    },
    summarize() {
      const at = fraction => {
        const rank = Math.ceil(count * fraction);
        let cumulative = 0;
        for (const [index, value] of buckets.entries()) {
          cumulative += value;
          if (cumulative >= rank) return index === buckets.length - 1 && !roundDown ? max : Math.min(max, Number(BigInt(index) * binNs) / 1e6);
        }
        throw new Error('Trace percentile is missing');
      };
      assert.ok(count, 'Trace distribution is empty');
      return {count, p50:at(.50), p95:at(.95), p99:at(.99), max};
    },
  };
}

function* acceptanceEdges(reader, slot, bit) {
  assert.equal(reader.header.toString('ascii'), 'OSGVJAC1');
  let mask = 0, previousReturn = 0n;
  for (let row = reader.next(); row; row = reader.next()) {
    const nextMask = row[1];
    const entry = row.readBigUInt64LE(2), returned = row.readBigUInt64LE(10);
    assert.equal(row[0], slot, 'Acceptance trace slot does not match');
    assert.ok(nextMask <= 3, 'Acceptance trace has unsupported button bits');
    assert.ok(nextMask !== mask, 'Acceptance trace contains an unchanged accepted mask');
    assert.ok(entry <= returned, 'Accepted UpdateVJD call timestamps must be ordered');
    assert.ok(entry >= previousReturn, 'Accepted UpdateVJD calls must be sequential and monotonic');
    if ((mask ^ nextMask) & bit) yield {entry, returned, down:Boolean(nextMask & bit)};
    mask = nextMask;
    previousReturn = returned;
  }
}

function acceptanceMetrics(edges) {
  const captureToCallEntryUpper = timingDistribution();
  const captureToDllReturnUpper = timingDistribution();
  const dllCallDuration = timingDistribution(false, 1_000n);
  const apiToRawInputLower = timingDistribution(true);
  const apiToRawInputUpper = timingDistribution();
  let receivedBeforeReturn = 0, stopToDllReturnMs;
  return {
    match(down, received, captureBefore, captureAfter) {
      const {value:call} = edges.next();
      assert.ok(call, 'Missing accepted driver edges');
      assert.equal(call.down, down, 'Accepted driver edge order does not match lane requests');
      assert.ok(call.entry >= captureAfter, 'UpdateVJD call entry must follow capture or Stop');
      assert.ok(received >= call.entry, 'Raw Input must follow UpdateVJD call entry');
      if (captureBefore === undefined) {
        stopToDllReturnMs = Number(call.returned - captureAfter) / 1e6;
        return;
      }
      if (received < call.returned) receivedBeforeReturn++;
      captureToCallEntryUpper.add(call.entry - captureBefore);
      captureToDllReturnUpper.add(call.returned - captureBefore);
      dllCallDuration.add(call.returned - call.entry);
      apiToRawInputLower.add(received > call.returned ? received - call.returned : 0n);
      apiToRawInputUpper.add(received - call.entry);
    },
    summarize() {
      assert.ok(edges.next().done, 'Unexpected extra accepted driver edge');
      return {captureToCallEntryUpperMs:captureToCallEntryUpper.summarize(),
        captureToDllReturnUpperMs:captureToDllReturnUpper.summarize(),
        dllCallDurationMs:dllCallDuration.summarize(),
        apiToRawInputLowerMs:apiToRawInputLower.summarize(),
        apiToRawInputUpperMs:apiToRawInputUpper.summarize(),
        receivedBeforeReturn, stopToDllReturnMs};
    },
  };
}

export function summarizeDualRateTrace(directory, slot, counts, raw, stopNs, acceptancePath) {
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
    const acceptedRows = acceptancePath ?
      (statSync(acceptancePath).size - acceptanceHeaderSize) / acceptanceRowSize : 0;
    if (acceptancePath) {
      assert.ok(Number.isInteger(acceptedRows) && acceptedRows > 0 &&
        acceptedRows <= counts[0] + counts[1] + 2, 'Invalid acceptance trace size');
    }
    const lanes = counts.map((count,index) => ({count, index,
      source:open(resolve(directory,sourceFiles[index]),sourceRowSize,sourceHeaderSize,count),
      offerToReceipt:timingDistribution(), offerToDispatch:timingDistribution(),
      dispatchToAdmission:timingDistribution(), captureToReceiptLower:timingDistribution(true),
      captureToReceiptUpper:timingDistribution(), admissionToReceipt:timingDistribution(),
      captureToAdmissionUpper:timingDistribution(),
      captureBracket:timingDistribution(), offerIntervals:timingDistribution(), receiverIntervals:timingDistribution(),
      acceptance:acceptancePath ?
        acceptanceMetrics(acceptanceEdges(
          open(acceptancePath,acceptanceRowSize,acceptanceHeaderSize,acceptedRows), slot, 1 << index)) : null,
      edges:0, sampledSharedPendingFramesPeak:0, sampledSharedPendingOwnedBytesPeak:0, batchSizePeak:0,
    }));
    for (const lane of lanes) assert.equal(lane.source.header.toString('ascii'), 'OSGDBG02');
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
          lane.acceptance?.match(false, received, undefined, stopNs);
        } else {
          const at = offer.readBigUInt64LE(0);
          const dispatched = offer.readBigUInt64LE(8);
          const captureBefore = offer.readBigUInt64LE(16);
          const captureAfter = offer.readBigUInt64LE(24);
          const admitted = offer.readBigUInt64LE(32);
          const sequence = offer.readUInt32LE(40);
          const pendingFrames = offer.readUInt32LE(44);
          const pendingOwnedBytes = offer.readUInt32LE(48);
          const batchIndex = offer.readUInt32LE(52);
          assert.ok(at >= qpcNs(origin) && received < stopNs);
          assert.ok(pendingFrames > 0 && pendingFrames <= 256, 'Shared queue pending frame sample is out of range');
          assert.ok(pendingOwnedBytes > 0, 'Shared queue byte sample is missing');
          assert.ok(batchIndex < 8, 'Batch index is out of range');
          assert.equal(sequence, lane.edges + 1, 'Native debug rows must preserve exact per-lane request sequence');
          assert.ok(at <= dispatched && dispatched <= captureBefore && captureBefore <= captureAfter,
            'Offer, dispatch and capture timestamps must be ordered');
          assert.ok(admitted >= captureAfter, 'Admission timestamp must follow capture');
          assert.ok(received >= captureAfter && received >= admitted,
            'Raw Input receipt must follow capture and native admission');
          lane.acceptance?.match(Boolean(mask & bit), received, captureBefore, captureAfter);
          lane.offerToReceipt.add(received - at);
          lane.offerToDispatch.add(dispatched - at);
          lane.dispatchToAdmission.add(admitted - dispatched);
          lane.captureToReceiptLower.add(received - captureAfter);
          lane.captureToReceiptUpper.add(received - captureBefore);
          lane.captureToAdmissionUpper.add(admitted - captureBefore);
          lane.admissionToReceipt.add(received - admitted);
          lane.captureBracket.add(captureAfter - captureBefore);
          if (lane.lastOffer !== undefined) lane.offerIntervals.add(at - lane.lastOffer);
          if (lane.lastReceipt !== undefined) lane.receiverIntervals.add(received - lane.lastReceipt);
          lane.firstOffer ??= at;
          lane.firstDispatch ??= dispatched;
          lane.firstAdmission ??= admitted;
          lane.lastOffer = at;
          lane.lastDispatch = dispatched;
          lane.lastAdmission = admitted;
          lane.lastReceipt = received;
          lane.sampledSharedPendingFramesPeak = Math.max(lane.sampledSharedPendingFramesPeak, pendingFrames);
          lane.sampledSharedPendingOwnedBytesPeak = Math.max(lane.sampledSharedPendingOwnedBytesPeak, pendingOwnedBytes);
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
        dispatchToAdmissionMs:lane.dispatchToAdmission.summarize(),
        captureToReceiptLowerMs:lane.captureToReceiptLower.summarize(),
        captureToReceiptUpperMs:lane.captureToReceiptUpper.summarize(),
        captureToAdmissionUpperMs:lane.captureToAdmissionUpper.summarize(),
        captureBracketMs:lane.captureBracket.summarize(), admissionToReceiptMs:lane.admissionToReceipt.summarize(),
        offerIntervalsMs:lane.offerIntervals.summarize(),
        receiverIntervalsMs:lane.receiverIntervals.summarize(),
        sampledSharedPendingFramesPeak:lane.sampledSharedPendingFramesPeak,
        sampledSharedPendingOwnedBytesPeak:lane.sampledSharedPendingOwnedBytesPeak,
        batchSizePeak:lane.batchSizePeak,stopReleaseMs:lane.stopReleaseMs,
        ...(lane.acceptance && {driverAcceptance:lane.acceptance.summarize()})};
    });
    const first = lanes[0].firstOffer > lanes[1].firstOffer ? lanes[0].firstOffer : lanes[1].firstOffer;
    const last = lanes[0].lastOffer < lanes[1].lastOffer ? lanes[0].lastOffer : lanes[1].lastOffer;
    return {lanes:results, overlapMs:Number(last-first)/1e6,
      sampledSharedQueuePeaks:{pendingFrames:Math.max(...results.map(lane=>lane.sampledSharedPendingFramesPeak)),
        pendingOwnedBytes:Math.max(...results.map(lane=>lane.sampledSharedPendingOwnedBytesPeak))},
      probeAfterStopMs:Number(qpcNs(end)-stopNs)/1e6, traceEvents:raw.traceEvents, device:raw.traceDevice};
  } finally {
    for (const reader of readers) reader.close();
  }
}

export async function startDualRateSoak(cdp, executable, slot, owner, directory, seconds) {
  const durationMs = Math.ceil(seconds * 1000 / 16) * 16;
  const acceptanceDirectory = process.env.OSG_VJOY_ACCEPTANCE_TRACE_DIR;
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
        let acceptancePath;
        if (acceptanceDirectory) {
          // Neutral-only feeder runs do not contribute any accepted edges.
          const traces = readdirSync(acceptanceDirectory)
            .filter(name => /^vjoy-acceptance-\d+\.bin$/.test(name) &&
              statSync(resolve(acceptanceDirectory,name)).size > acceptanceHeaderSize);
          assert.equal(traces.length, 1, 'Expected one vJoy acceptance trace for the active owner');
          acceptancePath = resolve(directory, traces[0]);
          copyFileSync(resolve(acceptanceDirectory,traces[0]), acceptancePath);
        }
        const result = summarizeDualRateTrace(directory,slot,counts,raw,stopNs,acceptancePath);
        result.lanes.forEach((lane, index) => { lane.captureToReducerBatchMs = reducer[index]; });
        assert.ok(result.overlapMs >= durationMs - 20,'The two native schedules must overlap throughout the soak');
        const budgetMisses = result.lanes.flatMap(lane => {
          const misses=[];
          if (Math.abs(lane.actualHz/lane.targetHz-1)>.05) misses.push(`${lane.nodeId} missed the offer rate`);
          if (lane.offerToReceiptMs.p95>20||lane.offerToReceiptMs.p99>50) misses.push(`${lane.nodeId} missed the offer-to-Raw Input budget`);
          if (lane.captureToReceiptUpperMs.p95>20||lane.captureToReceiptUpperMs.p99>50) misses.push(`${lane.nodeId} missed the capture-to-Raw Input budget`);
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
