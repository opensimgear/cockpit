import assert from 'node:assert/strict';
import test from 'node:test';
import {scanQrFrames} from './qr-frame-scan.ts';

function camera() {
  let next = 0;
  const pending = new Map();
  const video = {
    requestVideoFrameCallback(callback) {
      const id = next++;
      pending.set(id, callback);
      return id;
    },
    cancelVideoFrameCallback(id) { pending.delete(id); },
  };
  return {
    video, pending,
    async frame(now) {
      for (const [id, callback] of [...pending]) {
        pending.delete(id);
        callback(now);
      }
      await Promise.resolve();
    },
  };
}

test('QR scan waits for new frames and bounds decode work', async () => {
  const {video, pending, frame} = camera();
  const codes = [];
  let reads = 0, finish;
  scanQrFrames(video, () => {
    reads++;
    return new Promise(resolve => { finish = resolve; });
  }, code => codes.push(code), assert.fail);
  assert.equal(reads, 0);
  await frame(10);
  await frame(120);
  assert.equal(reads, 1);
  assert.equal(pending.size, 0);
  finish(undefined);
  await Promise.resolve();
  await frame(180);
  assert.equal(reads, 1);
  await frame(210);
  assert.equal(reads, 2);
  finish('pair-token');
  await Promise.resolve();
  assert.deepEqual(codes, ['pair-token']);
  assert.equal(pending.size, 0);
});

test('stopping cancels a scheduled frame and ignores an in-flight result', async () => {
  const {video, pending, frame} = camera();
  const stopped = scanQrFrames(video, assert.fail, assert.fail, assert.fail);
  stopped();
  assert.equal(pending.size, 0);
  let finish;
  const stop = scanQrFrames(video, () => new Promise(resolve => { finish = resolve; }), assert.fail, assert.fail);
  await frame(10);
  stop();
  finish('retired-token');
  await Promise.resolve();
  assert.equal(pending.size, 0);
});

test('a decode error ends the scan and reports the original error', async () => {
  const {video, pending, frame} = camera();
  const failure = new Error('camera decoding failed');
  const errors = [];
  scanQrFrames(video, async () => { throw failure; }, assert.fail, reason => errors.push(reason));
  await frame(20);
  assert.deepEqual(errors, [failure]);
  assert.equal(pending.size, 0);
});
