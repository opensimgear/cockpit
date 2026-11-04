import assert from 'node:assert/strict';
import test from 'node:test';
import { followProfileResources } from './profile-resource-observation.ts';

function harness() {
  let listener, visible = true, releases = 0;
  const reads = [], received = [];
  const subscription = followProfileResources(
    receive => { listener = receive; return () => { releases++; }; },
    { listAll: () => new Promise((resolve, reject) => reads.push({ resolve, reject })) },
    (page, error) => received.push({ page, error }),
    () => visible,
  );
  const signal = (revision, metadata = {}, session = 'inventory') => listener({
    status: 'ready', frame: {
      source: 'profile-resources', payload: { session, revision, inventoryDiagnostics: [], readOnly: false, ...metadata },
    },
  });
  const settle = async (index, revision, session = 'inventory') => {
    reads[index].resolve({ session, revision, resources: [], offset: 0, total: 0, inventoryDiagnostics: ['Page diagnostic'] });
    await Promise.resolve();
  };
  return { signal, settle, listener, subscription, received, reads, show: value => { visible = value; }, releases: () => releases };
}

test('resources follow only the latest commit while one page reconstruction is in flight', async () => {
  const h = harness();
  h.signal('1');
  h.signal('2');
  h.signal('3');
  h.signal('3', { inventoryDiagnostics: ['Committed diagnostic'], readOnly: true });
  assert.equal(h.reads.length, 1);
  await h.settle(0, '1');
  assert.deepEqual(h.received, []);
  assert.equal(h.reads.length, 2);
  await h.settle(1, '3');
  assert.equal(h.received[0].page.revision, '3');
  assert.deepEqual(h.received[0].page.inventoryDiagnostics, ['Committed diagnostic']);
  assert.equal(h.received[0].page.readOnly, true);
  h.signal('3', { inventoryDiagnostics: ['Changed metadata'] });
  assert.equal(h.reads.length, 2);
  assert.deepEqual(h.received[1].page.inventoryDiagnostics, ['Changed metadata']);
});

test('unexpected page revisions and errors wait for a new committed cursor without retries', async () => {
  const h = harness();
  h.signal('1');
  await h.settle(0, '2');
  assert.match(h.received.at(-1).error, /latest resource inventory revision/);
  h.signal('1');
  h.subscription.refreshVisible();
  assert.equal(h.reads.length, 1);
  h.signal('2', {}, 'replacement');
  h.reads[1].reject(new Error('Read failed'));
  await Promise.resolve();
  assert.equal(h.received.at(-1).error, 'Read failed');
  h.signal('3', {}, 'replacement');
  await h.settle(2, '3', 'replacement');
  assert.equal(h.received.at(-1).page.session, 'replacement');
});

test('hidden views coalesce catalogue commits and loss and disposal fence obsolete replies', async () => {
  const h = harness();
  h.show(false);
  h.signal('1');
  h.signal('2');
  assert.equal(h.reads.length, 0);
  h.show(true);
  h.subscription.refreshVisible();
  h.listener({ status: 'lost', error: 'Source lost' });
  await h.settle(0, '2');
  assert.deepEqual(h.received, [{ page: null, error: 'Source lost' }]);
  h.signal('2');
  h.subscription.dispose();
  await h.settle(1, '2');
  h.signal('3');
  assert.equal(h.reads.length, 2);
  assert.equal(h.releases(), 1);
  assert.deepEqual(h.received, [{ page: null, error: 'Source lost' }]);
});

test('malformed headers do not issue page reads and a repeated baseline can reuse its cached rows', async () => {
  const h = harness();
  h.listener({ status: 'connecting' });
  h.signal('01');
  assert.equal(h.reads.length, 0);
  assert.match(h.received[0].error, /current resource inventory/);
  h.signal('1');
  await h.settle(0, '1');
  h.listener({ status: 'lost' });
  h.signal('1');
  assert.equal(h.reads.length, 1);
  assert.equal(h.received.at(-1).page.revision, '1');
});
