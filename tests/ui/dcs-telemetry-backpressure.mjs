import assert from 'node:assert/strict';
import {until} from '../../scripts/reactive/cdp.mjs';
import {summarizeMilliseconds} from './vjoy-rate-transport.mjs';

export function openDcsTelemetryReaders(cdp, browser) {
  return cdp.evaluate(`(async () => {
    const {SharedObservations} = await import('/src/lib/streams/shared.ts');
    const cancel = new AbortController();
    const originalFetch = window.fetch;
    const trace = window.__dcsSlowReader = {
      fast: 0, slow: 0, error: null,
      close() {
        trace.stopTiming?.();
        cancel.abort();
        trace.fastReader.close();
        window.fetch = originalFetch;
      },
    };
    let transport;
    if (${browser}) {
      const {createBrowserObservationTransport} = await import('/src/lib/streams/browser.ts');
      // Hold only the first receipt for the slow reader on the real shared connection.
      window.fetch = (url, init) => {
        const receipt = typeof url === 'string' && url.endsWith('/api/observe/ack') && JSON.parse(init.body);
        if (receipt && receipt.subscriptionId === trace.slowId && receipt.delivery === '1') {
          return new Promise((resolve, reject) => {
            trace.release = () => originalFetch.call(window, url, init).then(resolve, reject);
            init.signal?.addEventListener('abort', () => reject(init.signal.reason), {once: true});
          });
        }
        return originalFetch.call(window, url, init);
      };
      transport = createBrowserObservationTransport();
    } else {
      const {createNativeObservationTransport} = await import('/src/lib/streams/native.ts');
      transport = createNativeObservationTransport();
    }
    trace.fastReader = new SharedObservations(transport);
    transport.open('games.telemetry', {
      frame(envelope, ack) {
        trace.slow++;
        trace.slowId = envelope.subscriptionId;
        if (${browser} || trace.slow > 1) void ack();
        else trace.release = ack;
      },
      error(error) { trace.error = error; },
    }, cancel.signal);
    trace.fastReader.subscribe('games.telemetry', state => {
      if (state.status !== 'ready') return;
      trace.fast++;
      trace.captureTiming?.();
    });
  })()`);
}

const timingChannel = 'flight.wind.world.z';
const sampleCount = 24;

async function clockBracket(cdp) {
  let best;
  for (let attempt = 0; attempt < 12; attempt++) {
    const before = Number(process.hrtime.bigint()) / 1e6;
    const local = await cdp.evaluate('performance.now()');
    const after = Number(process.hrtime.bigint()) / 1e6;
    const bracket = {low:local - after, high:local - before};
    if (!best || after - before < best.high - best.low) best = bracket;
  }
  return best;
}

// The invalidation has no packet payload. Match the unique value from the
// observation-driven read instead of treating any notification as its receipt.
export async function measureDcsTelemetryBackpressure(cdp, nativeValues, submitOwnship) {
  const initial = await clockBracket(cdp);
  const original = nativeValues[timingChannel];
  const samples = [];
  try {
    await cdp.evaluate(`(async () => {
      const {GameTelemetryClient,GameTelemetryController} = await import('/src/lib/game-telemetry.ts');
      const {gatewayRequest} = await import('/src/lib/gateway.ts');
      const trace = window.__dcsSlowReader, channel = ${JSON.stringify(timingChannel)};
      const observationId = trace.fastReader.subscriptionId('games.telemetry');
      if (!observationId || !trace.release) throw Error('Both telemetry readers must be ready');
      const controller = new GameTelemetryController(new GameTelemetryClient(async request => {
        const requestedMs = performance.now();
        const {gameTelemetry} = await gatewayRequest(request);
        trace.lastRead = {requestedMs, replyMs:performance.now()};
        return gameTelemetry;
      }));
      controller.configure({gameId:'dcs',search:channel,offset:0,limit:8,observationId});
      controller.setActive(true);
      trace.captureTiming = () => { if (trace.expectedValue !== undefined) controller.invalidate(); };
      trace.stopTiming = () => {
        delete trace.captureTiming;
        delete trace.expectedValue;
        controller.dispose();
      };
      controller.subscribe(state => {
        if (trace.expectedValue === undefined) return;
        if (state.error) { trace.error = state.error; return; }
        const row = state.report?.games[0]?.channels.find(row => row.id === channel);
        if (!state.stale && row?.status === 'known' && row.value === trace.expectedValue) {
          trace.timedSample = {value:row.value, ...trace.lastRead, receivedMs:performance.now(), fast:trace.fast};
          delete trace.expectedValue;
        }
      });
    })()`);
    for (let index = 0; index < sampleCount; index++) {
      const value = -20 - index;
      const fastBefore = await cdp.evaluate(`(() => {
        const trace = window.__dcsSlowReader;
        trace.expectedValue = ${value};
        trace.timedSample = null;
        return trace.fast;
      })()`);
      const submission = await submitOwnship(() => { nativeValues[timingChannel] = value; });
      const received = await until(async () => {
        const state = await cdp.evaluate(`(({slow,error,timedSample}) => ({slow,error,timedSample}))(window.__dcsSlowReader)`);
        assert.equal(state.slow, 1, 'The slow receipt must stay held throughout the timing sample');
        assert.equal(state.error, null);
        return state.timedSample;
      }, Boolean, 1500);
      assert.equal(received.value, value);
      assert.ok(received.fast > fastBefore, 'A fast notification must advance the timed read');
      if (samples.length) assert.ok(submission.packetSequence > samples.at(-1).packetSequence);
      samples.push({...submission, ...received, fastBefore});
    }
    const final = await clockBracket(cdp);
    const low = Math.max(initial.low, final.low);
    const high = Math.min(initial.high, final.high);
    assert.ok(high >= low, 'Node and Cockpit clock brackets must overlap');
    assert.ok(samples.every(sample => sample.receivedMs >= sample.submittedMs + low));
    const upperMs = summarizeMilliseconds(samples.map(sample => sample.receivedMs - sample.submittedMs - low));
    const lowerMs = summarizeMilliseconds(samples.map(sample => Math.max(0, sample.receivedMs - sample.submittedMs - high)));
    assert.ok(samples.every(sample => sample.requestedMs <= sample.replyMs && sample.replyMs <= sample.receivedMs));
    const inspectionMs = summarizeMilliseconds(samples.map(sample => sample.replyMs - sample.requestedMs));
    const projectionMs = summarizeMilliseconds(samples.map(sample => sample.receivedMs - sample.replyMs));
    const first = samples[0], last = samples.at(-1);
    const offeredPacketHz = (last.packetSequence - first.packetSequence) * 1000 / (last.submittedMs - first.submittedMs);
    const budgetMisses = upperMs.p95 <= 100 ? [] :
      [`Observation-driven telemetry read p95 exceeded 100 ms: ${upperMs.p95.toFixed(2)} ms`];
    return {sampleCount, offeredPacketHz, lowerMs, upperMs, inspectionMs, projectionMs,
      alignmentWidthMs:high - low, samples, budgetMisses};
  } finally {
    nativeValues[timingChannel] = original;
    await cdp.evaluate('window.__dcsSlowReader.stopTiming?.(); delete window.__dcsSlowReader.stopTiming;');
  }
}
