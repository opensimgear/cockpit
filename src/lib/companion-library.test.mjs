import assert from 'node:assert/strict';
import test from 'node:test';
import { newDocument, publishDocument } from '../../packages/companion-designer/src/index.ts';
import { CompanionLibraryStore, COMPANION_LIBRARY_LIMITS, indexedDbStorage } from './companion-library.ts';

const copy = value => value === undefined ? undefined : structuredClone(value);
function memory(initial) {
  return {
    value: copy(initial), writes: 0, fail: false,
    async read() { return copy(this.value); },
    async write(value) {
      if (this.fail) throw new Error('Atomic write failed');
      this.value = copy(value); this.writes++;
    },
  };
}
function snapshot(...names) {
  return {
    schemaVersion: 1,
    library: { schemaVersion: 1, drafts: names.map((name, index) => ({ id: `draft-${index}`, name, document: newDocument(name), assets: [] })) },
    publications: {},
  };
}

test('an empty store loads an empty local library without writing or activating anything', async () => {
  const storage = memory();
  assert.deepEqual(await new CompanionLibraryStore(storage).load(), snapshot());
  assert.equal(storage.writes, 0);
});

test('two distinct drafts save and reopen together; returned edits cannot mutate storage', async () => {
  const storage = memory(), store = new CompanionLibraryStore(storage), input = snapshot('Flight', 'Racing');
  const saved = await store.save(input);
  assert.deepEqual(saved, input);
  saved.library.drafts[0].name = 'Unsaved change';
  input.library.drafts[1].document.ui.title = 'Also unsaved';
  assert.equal((await store.load()).library.drafts[0].name, 'Flight');
  assert.equal((await store.load()).library.drafts[1].document.ui.title, 'Racing');
  assert.equal(storage.writes, 1);
});

test('published snapshots are verified, frozen and retained independently of editable drafts', async () => {
  const storage = memory(), store = new CompanionLibraryStore(storage), input = snapshot('Flight');
  const revision = await publishDocument(input.library.drafts[0].document, []);
  input.publications['draft-0'] = [revision, revision];
  const saved = await store.save(input);
  assert.equal(saved.publications['draft-0'].length, 1);
  assert.equal(Object.isFrozen(saved.publications['draft-0'][0].document), true);
  await store.save(snapshot());
  const reopened = await store.load();
  assert.equal(reopened.library.drafts.length, 0);
  assert.equal(reopened.publications['draft-0'][0].publicationHash, revision.publicationHash);
  assert.deepEqual(reopened.publications['draft-0'][0].document, revision.document);
});

test('new immutable publications append without replacing or duplicating prior revisions', async () => {
  const storage = memory(), store = new CompanionLibraryStore(storage), input = snapshot('Panel');
  const first = await publishDocument(input.library.drafts[0].document, []);
  input.publications['draft-0'] = [first];
  await store.save(input);
  input.library.drafts[0].document.ui.title = 'Restyled panel';
  const second = await publishDocument(input.library.drafts[0].document, []);
  input.publications['draft-0'] = [second];
  const saved = await store.save(input);
  assert.deepEqual(saved.publications['draft-0'].map(revision => revision.publicationHash), [first.publicationHash, second.publicationHash]);
  assert.equal(saved.publications['draft-0'][0].document.ui.title, 'Panel');
});

test('failed input validation and failed atomic writes preserve the last committed snapshot', async () => {
  const storage = memory(), store = new CompanionLibraryStore(storage), input = snapshot('Saved');
  await store.save(input);
  const before = copy(storage.value);
  await assert.rejects(store.save({ ...input, credentials: 'never persist' }), /field/);
  const invalid = copy(input); invalid.library.drafts[0].document.ui.controls[0].id = 'unknown';
  await assert.rejects(store.save(invalid), /Invalid saved companion drafts/);
  storage.fail = true;
  await assert.rejects(store.save(snapshot('Pending')), /Atomic write failed/);
  assert.deepEqual(storage.value, before);
  assert.equal(storage.writes, 1);
  storage.fail = false;
  await store.save(snapshot('Recovered'));
  assert.equal((await store.load()).library.drafts[0].name, 'Recovered');
});

test('saved corruption is reported instead of silently replacing the library with defaults', async () => {
  for (const value of [null, [], {}, { ...snapshot(), schemaVersion: 2 }, { ...snapshot(), pairings: [] }]) {
    const storage = memory(value), store = new CompanionLibraryStore(storage);
    await assert.rejects(store.load());
    await assert.rejects(store.save(snapshot('New')));
    assert.deepEqual(storage.value, value);
    assert.equal(storage.writes, 0);
  }
});

test('modified publication content and forged hashes are rejected on load and save', async () => {
  const input = snapshot('Published');
  const revision = await publishDocument(input.library.drafts[0].document, []);
  for (const change of [
    candidate => { candidate.document.ui.title = 'Tampered'; },
    candidate => { candidate.capabilityFingerprint = 'sha256:' + '0'.repeat(64); },
    candidate => { candidate.publicationHash = 'sha256:' + '0'.repeat(64); },
    candidate => { candidate.credentials = 'secret'; },
  ]) {
    const altered = copy(revision); change(altered);
    const corrupt = { ...input, publications: { 'draft-0': [altered] } };
    const corruptStorage = memory(corrupt);
    await assert.rejects(new CompanionLibraryStore(corruptStorage).load());
    assert.equal(corruptStorage.writes, 0);
    const goodStorage = memory(input);
    await assert.rejects(new CompanionLibraryStore(goodStorage).save(corrupt));
    assert.deepEqual(goodStorage.value, input);
  }
});

test('malformed archives and archive limits fail before a storage write', async () => {
  const input = snapshot('Panel'), revision = await publishDocument(input.library.drafts[0].document, []);
  for (const publications of [null, [], { 'invalid.id': [] }, { 'draft-0': 'no array' },
    { 'draft-0': Array(COMPANION_LIBRARY_LIMITS.revisions + 1).fill(revision) },
    Object.fromEntries(Array.from({ length: COMPANION_LIBRARY_LIMITS.revisions + 1 }, (_, index) => [`draft-${index}`, []])),
  ]) {
    const storage = memory(), store = new CompanionLibraryStore(storage);
    await assert.rejects(store.save({ ...input, publications }));
    assert.equal(storage.writes, 0);
  }
});

test('byte budget rejection does not write a partial oversized snapshot', async () => {
  const storage = memory(), store = new CompanionLibraryStore(storage);
  await assert.rejects(store.save({ extra: 'x'.repeat(COMPANION_LIBRARY_LIMITS.bytes) }), /64 MiB/);
  assert.equal(storage.writes, 0);
});

test('concurrent saves capture their inputs immediately and commit in call order', async () => {
  const storage = memory();
  let release, entered;
  const blocked = new Promise(resolve => { release = resolve; });
  const writing = new Promise(resolve => { entered = resolve; });
  const write = storage.write.bind(storage);
  let first = true;
  storage.write = async value => {
    if (first) { first = false; entered(); await blocked; }
    return write(value);
  };
  const store = new CompanionLibraryStore(storage), one = snapshot('First'), two = snapshot('Second');
  const a = store.save(one);
  await writing;
  const b = store.save(two);
  two.library.drafts[0].name = 'Changed after save call';
  release();
  await Promise.all([a, b]);
  assert.equal((await store.load()).library.drafts[0].name, 'Second');
  assert.equal(storage.writes, 2);
});

function fakeIndexedDb() {
  const state = { value: undefined, opens: 0, stores: 0, transactions: 0, closed: 0, failOpen: false, blocked: false, failure: '', versionChange: undefined, blockedRequest: undefined };
  const database = {
    objectStoreNames: { contains: () => state.stores > 0 },
    createObjectStore() { state.stores++; },
    close() { state.closed++; },
    set onversionchange(callback) { state.versionChange = callback; },
    transaction(_name, mode) {
      state.transactions++;
      const request = {};
      const tx = { error: null, objectStore: () => ({
        get() { queueMicrotask(() => finish()); return request; },
        put(value) { queueMicrotask(() => finish(value)); return request; },
      }) };
      function finish(value) {
        if (state.failure === 'error') { tx.error = new Error('Transaction failed'); tx.onerror(); }
        else if (state.failure === 'abort') tx.onabort();
        else {
          if (mode === 'readwrite') state.value = copy(value);
          request.result = copy(state.value); tx.oncomplete();
        }
      }
      return tx;
    },
  };
  return {
    state,
    factory: { open() {
      state.opens++;
      const request = { result: database, error: null };
      queueMicrotask(() => {
        if (state.failOpen) { request.error = new Error('Open failed'); request.onerror(); }
        else if (state.blocked) { state.blockedRequest = request; request.onblocked(); }
        else { request.onupgradeneeded(); request.onsuccess(); }
      });
      return request;
    } },
  };
}

test('IndexedDB adapter commits one atomic record and shares its connection', async () => {
  const { factory, state } = fakeIndexedDb(), adapter = indexedDbStorage(factory);
  assert.equal(await adapter.read(), undefined);
  const input = snapshot('Native browser storage');
  await adapter.write(input);
  assert.deepEqual(await adapter.read(), input);
  assert.equal(state.opens, 1);
  assert.equal(state.stores, 1);
  assert.equal(state.transactions, 3);
  state.versionChange();
  assert.equal(state.closed, 1);
  await adapter.read();
  assert.equal(state.opens, 2);
});

test('IndexedDB open, blocked, transaction and abort errors preserve the previous record', async () => {
  await assert.rejects(indexedDbStorage().read(), /unavailable/);
  const { factory, state } = fakeIndexedDb(), adapter = indexedDbStorage(factory), input = snapshot('Saved');
  state.failOpen = true;
  await assert.rejects(adapter.read(), /Open failed/);
  state.failOpen = false; state.blocked = true;
  await assert.rejects(adapter.read(), /blocked/);
  state.blockedRequest.onsuccess(); // An unblocked request may complete after rejection.
  assert.equal(state.closed, 1);
  state.blocked = false;
  await adapter.write(input);
  for (const failure of ['error', 'abort']) {
    state.failure = failure;
    await assert.rejects(adapter.write(snapshot('Not saved')), /failed|cancelled/);
    assert.deepEqual(state.value, input);
  }
});

test('simple panels compact old snapshots while preserving current panels and active assignment pins', async () => {
  const storage=memory(), store=new CompanionLibraryStore(storage), input=snapshot('Panel');
  const first=await publishDocument(input.library.drafts[0].document,[]);
  input.publications['draft-0']=[first];await store.save(input);
  input.library.drafts[0].document.ui.title='Edited';
  const second=await publishDocument(input.library.drafts[0].document,[]);
  input.publications['draft-0']=[second];await store.save(input);
  assert.equal((await store.compact([first.publicationHash])).publications['draft-0'].length,2);
  assert.deepEqual((await store.compact([])).publications['draft-0'].map(p=>p.publicationHash),[second.publicationHash]);
  await store.save(snapshot());assert.deepEqual((await store.compact([])).publications,{});
});
test('failed compaction preserves previously committed panels and snapshots', async () => {
  const storage=memory(), store=new CompanionLibraryStore(storage);
  await store.save(snapshot('Saved'));const before=copy(storage.value);storage.fail=true;
  await assert.rejects(store.compact([]),/Atomic write failed/);assert.deepEqual(storage.value,before);
});
