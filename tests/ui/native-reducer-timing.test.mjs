import assert from 'node:assert/strict';
import test from 'node:test';
import {summarizeReducerTiming} from './native-reducer-timing.mjs';

const endpoint = {source:'telemetry', port:'physics.packetId', minimumSamples:2};
const rows = [
  {producer:'reader', generation:'1', clockId:'host', sequence:'12', value:5, capturedUs:'1000', appliedUs:'1210'},
  {producer:'reader', generation:'1', clockId:'host', sequence:'36', value:7, capturedUs:'1100', appliedUs:'1430'},
];

test('the Assetto publisher counter and host page sequence count different events', () => {
  const assetto = summarizeReducerTiming(rows, {...endpoint, packetCounter:true});
  assert.equal(assetto.publisherPacketGaps, '1');
  assert.equal(assetto.captureToReducerBatchMs.p95, 0.33);
  assert.equal(assetto.captureIntervalsMs.p50, 0.1);
  assert.equal(summarizeReducerTiming(rows, endpoint).captureSequenceGaps, '23');
});

test('reducer timing rejects clock changes, backward time and repeated publisher packets', () => {
  for (const edit of [{clockId:'other'}, {appliedUs:'1099'}, {value:5}, {value:null}]) {
    assert.throws(() => summarizeReducerTiming([rows[0], {...rows[1], ...edit}], {...endpoint, packetCounter:true}));
  }
});
