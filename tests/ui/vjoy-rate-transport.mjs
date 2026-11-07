import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {Agent,request as httpRequest} from 'node:http';
import {setImmediate as nextTurn} from 'node:timers/promises';

function summarizeMilliseconds(values) {
  const sorted = [...values].sort((a,b) => a-b);
  assert.ok(sorted.length && sorted.every(value => Number.isFinite(value) && value >= 0));
  const at = percentile => sorted[Math.ceil(sorted.length * percentile) - 1];
  return { count: sorted.length, p50: at(0.5), p95: at(0.95), p99: at(0.99), max: sorted.at(-1) };
}

const dualRateLanes = [
  {nodeId:'debug', field:'pressed', intervalMs:1, count:193},
  {nodeId:'debug-peer', field:'peerPressed', intervalMs:8, count:25},
];
const intervals = values => values.slice(1).map((value,index) => value - values[index]);

async function submitBrowserRate(request, owner, reference, onStarted) {
  const start = process.hrtime.bigint() + 15_000_000n;
  let started = 0;
  const results = await Promise.allSettled(dualRateLanes.map(async lane => {
    const streamId = randomUUID(), samples = [];
    for (let index = 0; index < lane.count; index++) {
      const deadline = start + BigInt(index * lane.intervalMs) * 1_000_000n;
      // Yield for I/O without rounding the short software schedule to a timer tick.
      while (process.hrtime.bigint() < deadline) await nextTurn();
      const value = index % 2 === 0, sequence = String(index + 1);
      const at = process.hrtime.bigint();
      if (index === 0 && ++started === dualRateLanes.length) onStarted?.();
      const {debugController:state} = await request({command:'profile_debug_controller_input',
        owner,nodeId:lane.nodeId,streamId,sequence,changes:[{portId:'button-1',value}]});
      assert.equal(state.sequence,sequence);
      assert.equal(state.replayed,false);
      assert.ok(Number.isInteger(state.pendingFrames) && state.pendingFrames > 0 && state.pendingFrames <= 256);
      samples.push({at:Number(at-reference)/1e6,roundTripMs:Number(process.hrtime.bigint()-at)/1e6,
        pendingFrames:state.pendingFrames,value});
    }
    return samples;
  }));
  return results.map(result => {
    if (result.status === 'rejected') throw result.reason;
    return result.value;
  });
}

function browserGateway(origin) {
  assert.ok(origin.protocol==='http:'&&['127.0.0.1','localhost'].includes(origin.hostname)
    && !origin.username&&!origin.password,'Use an isolated loopback browser-dev Cockpit');
  const agent=new Agent({keepAlive:true,maxSockets:2});
  const endpoint=new URL('/api/gateway/',origin);
  return {
    request(payload) {
      const body=Buffer.from(JSON.stringify(payload));
      return new Promise((resolve,reject)=>{
        const request=httpRequest(endpoint,{method:'POST',agent,signal:AbortSignal.timeout(5000),
          headers:{origin:origin.origin,'x-osg-client':'1','content-type':'application/json','content-length':body.length}},
        response=>{
          const chunks=[];
          response.on('data',chunk=>chunks.push(chunk));
          response.once('error',reject);
          response.once('end',()=>{
            try{resolve(JSON.parse(Buffer.concat(chunks).toString('utf8')));}
            catch(error){reject(error);}
          });
        });
        request.once('error',reject);
        request.setNoDelay(true);
        request.end(body);
      });
    },
    close(){agent.destroy();},
  };
}

function dualRateResults(submissions, events, receiverTickHz, receiptMs, clock, stopMs) {
  return dualRateLanes.map((lane, index) => {
    const samples = submissions[index];
    let last = false;
    const edges = events.filter(event => {
      assert.equal(typeof event[lane.field], 'boolean', 'Rebuild RawInputProbe with two-button mode');
      const changed = event[lane.field] !== last;
      last = event[lane.field];
      return changed;
    });
    const targetHz = 1000 / lane.intervalMs;
    const submissionsMs = samples.map(sample => sample.at);
    const actualHz = (samples.length - 1) * 1000 / (submissionsMs.at(-1) - submissionsMs[0]);
    const receiverMs = edges.slice(0, -1).map(event => event.receivedTicks * 1000 / receiverTickHz);
    const expected = samples.map(sample => sample.value).concat(false);
    const received = edges.map(event => event[lane.field]);
    const intact = expected.length === received.length && expected.every((value,i) => value === received[i]);
    const latency = intact ? edges.slice(0, -1).map((event,i) => receiptMs(event) - samples[i].at) : [];
    return {
      ...lane, targetHz, actualHz,
      submissionIntervalsMs:summarizeMilliseconds(intervals(submissionsMs)),
      gatewayRoundTripMs:summarizeMilliseconds(samples.map(sample => sample.roundTripMs)),
      pendingFramesPeak:Math.max(...samples.map(sample => sample.pendingFrames)),
      receiverIntervalsMs:receiverMs.length > 1 ? summarizeMilliseconds(intervals(receiverMs)) : null,
      submissionToReceiverMs: intact ? {
        lower:summarizeMilliseconds(latency.map(ms => Math.max(0, ms - clock.high))),
        upper:summarizeMilliseconds(latency.map(ms => ms - clock.low)),
      } : null,
      stopReleaseMs: intact ? receiptMs(edges.at(-1)) - stopMs : null,
      expected, received,
    };
  });
}

export {summarizeMilliseconds,dualRateLanes,intervals,submitBrowserRate,browserGateway,dualRateResults};
