import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import {
  DEFAULT_PROFILE_ID, MAX_PROFILE_BYTES, ProfilesClient, captureProfileSession,
  exportProfileDocument, parseProfileDocument, profileDocumentError, profileDraft, profileDraftDirty,
} from './profiles.ts';

const clone = value => JSON.parse(JSON.stringify(value));
const blank = () => ({ schemaVersion: 1, id: DEFAULT_PROFILE_ID, name: 'Default', revision: 1,
  eligibility: { gameIds: [] }, resources: [], bindings: [], processingNodes: [], virtualOutputs: [],
  visibility: [], overrides: [], companionAssignments: [], editor: { positions: [], viewport: { x: 0, y: 0, zoom: 1 } } });
const nativeSession = () => ({ selectedProfileId: DEFAULT_PROFILE_ID, selectionMode: 'manual', state: 'stopped', appliedSnapshot: null, fault: null });
const port = (id = 'button', valueType = 'boolean') => ({ id, name: id, direction: 'output', delivery: 'state', valueType,
  unit: null, range: null, enumValues: [], freshnessMs: 1500, fallback: false });
const resource = () => ({ id: 'wheel', name: 'Wheel', kind: 'device', required: true, ports: [port()] });
const withResource = () => ({ ...blank(), resources: [resource()] });
const invalid = document => assert.ok(profileDocumentError(document));

// The same versioned documents are loaded by the Rust contract tests.
for (const name of ['default', 'blank', 'mixed']) {
  test(`shared native ${name} profile fixture roundtrips without legacy adapters`, async () => {
    const source = await readFile(new URL(`../../tests/fixtures/profiles/${name}.json`, import.meta.url), 'utf8');
    const document = parseProfileDocument(source);
    assert.equal(profileDocumentError(document), '');
    assert.deepEqual(parseProfileDocument(exportProfileDocument(document)), document);
  });
}

test('imports are bounded UTF-8 JSON and reject legacy/local assignments at every level', () => {
  for (const source of ['{', 'null', '[]', ' '.repeat(MAX_PROFILE_BYTES + 1), 'é'.repeat(MAX_PROFILE_BYTES / 2 + 1)]) assert.throws(() => parseProfileDocument(source));
  for (const extra of [{ assignments: [] }, { connection: {} }, { profile: blank() }, { document: blank() }, { machineAssignments: [] }]) invalid({ ...blank(), ...extra });
  for (const document of [{ ...blank(), schemaVersion: 2 }, { ...blank(), revision: 0 }, { ...blank(), revision: Number.MAX_SAFE_INTEGER + 1 }, { ...blank(), id: 'legacy' }, { ...blank(), id: 'ABCDEF00-0000-4000-8000-000000000001' }, { ...blank(), name: '\n' }, { ...blank(), name: 'x'.repeat(81) }, { ...blank(), eligibility: { gameIds: ['dcs', 'dcs'] } }, { ...blank(), eligibility: { gameIds: ['bad game'] } }]) invalid(document);
  invalid({ ...blank(), eligibility: { gameIds: [], executable: 'C:/private/game.exe' } });
  invalid({ ...withResource(), resources: [{ ...resource(), sourceId: 'private-device' }] });
  invalid({ ...withResource(), resources: [{ ...resource(), ports: [{ ...port(), path: 'private' }] }] });
  assert.ok(profileDocumentError(undefined));
  assert.ok(profileDocumentError({ value: 1n }));
  assert.ok(profileDocumentError({ ...blank(), name: 'é'.repeat(MAX_PROFILE_BYTES) }));
  assert.throws(() => exportProfileDocument({ ...blank(), sourceId: 'private' }));
});

test('typed ports reject unsupported shapes, bounds, and fallback values', () => {
  for (const changed of [
    { id: 'bad id' }, { name: '' }, { direction: 'both' }, { delivery: 'queue' }, { valueType: 'object' },
    { unit: '' }, { unit: 'm'.repeat(33) }, { freshnessMs: 0 }, { freshnessMs: 600001 },
    { range: { min: 2, max: 1 } }, { range: { min: 0, max: Infinity } },
    { enumValues: ['a', 'a'] }, { enumValues: [''] }, { enumValues: Array(65).fill('x') },
    { fallback: 1 }, { valueType: 'integer', fallback: 0.5 }, { valueType: 'number', fallback: Infinity },
    { valueType: 'number', range: { min: 1, max: 2 }, fallback: 0 },
    { valueType: 'text', fallback: '\n' }, { valueType: 'text', fallback: 'a'.repeat(257) },
    { valueType: 'enum', enumValues: ['one'], fallback: 'two' },
  ]) invalid({ ...withResource(), resources: [{ ...resource(), ports: [{ ...port(), ...changed }] }] });
  for (const changed of [
    { fallback: true }, { valueType: 'integer', range: { min: 0, max: 5 }, fallback: 3 },
    { valueType: 'number', range: { min: 0, max: 2 }, fallback: 1.5, unit: 'ratio', freshnessMs: 100 },
    { valueType: 'text', fallback: '' }, { valueType: 'enum', enumValues: ['one'], fallback: 'one' },
    { delivery: 'event', freshnessMs: null, fallback: null },
  ]) assert.equal(profileDocumentError({ ...withResource(), resources: [{ ...resource(), ports: [{ ...port(), ...changed }] }] }), '');
  invalid({ ...withResource(), resources: [{ ...resource(), ports: [port(), port()] }] });
});

test('graph structure, logical assignments, and editor metadata remain portable and bounded', () => {
  const document = withResource();
  const node = { id: 'gain', name: 'Gain', operation: { kind: 'identity' }, inputs: [port('in')], outputs: [port('out')] };
  for (const operation of [{ kind: 'identity' }, { kind: 'not' }, { kind: 'toggle' }, { kind: 'trigger' }, { kind: 'scale', factor: 2, offset: 1 }, { kind: 'threshold', threshold: 0.5 }]) {
    assert.equal(profileDocumentError({ ...document, processingNodes: [{ ...node, operation }] }), '');
  }
  for (const operation of [null, { kind: 'script' }, { kind: 'identity', executable: 'private' }, { kind: 'scale', factor: Infinity, offset: 0 }, { kind: 'threshold', threshold: NaN }]) invalid({ ...document, processingNodes: [{ ...node, operation }] });
  invalid({ ...document, processingNodes: [{ ...node, id: 'wheel' }] });
  invalid({ ...document, resources: Array(65).fill(resource()) });
  invalid({ ...document, processingNodes: Array(257).fill(node) });
  invalid({ ...document, virtualOutputs: Array(17).fill({ id: 'output', name: 'Output', required: false, ports: [] }) });
  invalid({ ...document, virtualOutputs: [{ id: 'out', name: 'Out', required: true, ports: [], slot: 1 }] });
  const manyPorts = Array.from({ length: 128 }, (_, index) => port(`p${index}`));
  invalid({ ...blank(), resources: Array.from({ length: 9 }, (_, index) => ({ ...resource(), id: `r${index}`, ports: manyPorts })) });
  const binding = { id: 'route', from: { nodeId: 'wheel', portId: 'button' }, to: { nodeId: 'gain', portId: 'in' } };
  assert.equal(profileDocumentError({ ...document, processingNodes: [node], bindings: [binding],
    visibility: [{ resourceId: 'wheel', mode: 'hidden-while-active' }],
    overrides: [{ resourceId: 'wheel', enabled: false, debounceMs: 20, invertPorts: ['button'] }],
    companionAssignments: [{ resourceId: 'tablet', publicationHash: 'sha256:' + 'a'.repeat(64) }],
    editor: { positions: [{ nodeId: 'wheel', x: -5, y: 20 }], viewport: { x: 0, y: 0, zoom: 2 } },
  }), '');
  for (const change of [
    { bindings: [binding, binding] }, { bindings: [{ ...binding, from: { nodeId: 'wheel', portId: 'button', sourceId: 'private' } }] },
    { visibility: [{ resourceId: 'wheel', mode: 'hide-all' }] },
    { overrides: [{ resourceId: 'wheel', enabled: null, debounceMs: 60001, invertPorts: [] }] },
    { companionAssignments: [{ resourceId: 'tablet', publicationHash: 'unhashed' }] },
    { editor: { positions: [], viewport: { x: 0, y: 0, zoom: 0 } } },
    { editor: { positions: [{ nodeId: 'wheel', x: 1000001, y: 0 }], viewport: { x: 0, y: 0, zoom: 1 } } },
  ]) invalid({ ...document, ...change });
});

test('draft and captured session snapshots never alias their source or each other', () => {
  const document = withResource(), editor = profileDraft(document);
  editor.document.resources[0].name = 'Draft wheel';
  assert.equal(profileDraftDirty(editor), true);
  assert.equal(editor.baseDocument.resources[0].name, 'Wheel');
  assert.equal(document.resources[0].name, 'Wheel');
  assert.equal(profileDraftDirty(profileDraft(document)), false);
  const session = nativeSession();
  session.appliedSnapshot = { profileId: document.id, revision: document.revision, semanticHash: 'sha256:' + 'a'.repeat(64) };
  const captured = captureProfileSession(session);
  session.appliedSnapshot.revision = 99;
  assert.equal(captured.appliedSnapshot.revision, 1);
  assert.ok(Object.isFrozen(captured)); assert.ok(Object.isFrozen(captured.appliedSnapshot));
  assert.throws(() => { captured.appliedSnapshot.revision = 3; }, TypeError);
});

test('client storage CRUD and editor opening never select or apply a runtime', async () => {
  let saved = blank(); const session = nativeSession(), calls = [];
  const client = new ProfilesClient(async request => {
    calls.push(clone(request));
    if (request.command === 'profile_save') saved = { ...clone(request.document), revision: saved.revision + 1 };
    if (request.command === 'profile_create' || request.command === 'profile_duplicate' || request.command === 'profile_import') saved = { ...clone(request.document ?? saved), id: '00000000-0000-4000-8000-000000000002', name: request.name ?? saved.name, revision: 1 };
    if (request.command === 'profile_rename') saved = { ...saved, name: request.name, revision: saved.revision + 1 };
    return { document: saved, session, profiles: [{ id: saved.id, name: saved.name, revision: saved.revision, isDefault: true, semanticHash: 'a'.repeat(64) }], diagnostics: [], readOnly: false, deletedId: request.id };
  });
  assert.equal(client.editor, null); assert.equal(client.session, null);
  await client.status(); await client.list();
  const before = clone(client.session);
  const editor = await client.open(saved.id);
  editor.document.name = 'Local edit';
  assert.equal(client.editor.document.name, 'Default');
  client.updateDraft(editor.document);
  assert.equal(client.editor.document.name, 'Local edit');
  assert.deepEqual(client.session, before);
  await client.saveDraft();
  assert.equal(profileDraftDirty(client.editor), false);
  assert.equal(client.editor.baseRevision, 2);
  assert.deepEqual(client.session, before);
  assert.equal(calls.filter(call => call.command === 'profile_save').length, 1);
  assert.equal(calls.at(-1).expectedRevision, 1);
  const exported = await client.export(saved.id);
  const imported = await client.import(exported, 'Imported');
  assert.equal(imported.name, 'Imported');
  await client.import(exported);
  await client.create('New'); await client.rename(saved.id, 'Renamed', saved.revision);
  const duplicated = await client.duplicate(saved.id, 'Copy');
  duplicated.editor.viewport.zoom = 3;
  assert.equal(saved.editor.viewport.zoom, 1);
  await client.duplicate(saved.id); await client.delete(saved.id, saved.revision);
  assert.ok(calls.every(call => !['profile_select', 'profile_apply', 'mapping_virtual_start', 'controller_visibility_apply'].includes(call.command)));
  assert.deepEqual(client.session, before);
  client.close(); assert.equal(client.editor, null);
});

test('selection, mode, pause and explicit recovery consume native state without activating', async () => {
  const calls = [], session = nativeSession();
  const client = new ProfilesClient(async request => {
    calls.push(request);
    if (request.command === 'profile_select') session.selectedProfileId = request.id;
    if (request.command === 'profile_selection_mode') session.selectionMode = request.mode;
    if (request.command === 'profile_pause') session.state = request.paused ? 'paused' : 'stopped';
    return { session };
  });
  await client.select('other');
  assert.equal(client.session.selectedProfileId, 'other');
  assert.equal(calls.at(-1).selectionSource, 'manual');
  await client.selectionMode('automatic'); await client.select(DEFAULT_PROFILE_ID, 'automatic');
  await client.pause(); assert.equal(client.session.state, 'paused');
  await client.pause(false); assert.equal(client.session.state, 'stopped');
  session.state = 'faulted'; session.fault = 'Test fault';
  await client.status(); assert.equal(client.session.fault, 'Test fault');
  await client.reset(); assert.deepEqual(calls.at(-1), { command: 'profile_reset', confirm: true });
  assert.ok(calls.every(call => call.command !== 'profile_apply'));
});

test('client errors and invalid imports leave drafts, session and transport untouched', async () => {
  let fail = false, count = 0;
  const client = new ProfilesClient(async () => { count++; if (fail) throw Error('revision conflict'); return { document: blank(), session: nativeSession() }; });
  await assert.rejects(client.saveDraft(), /Open a profile/);
  assert.throws(() => client.updateDraft(blank()), /Open this profile/);
  await client.open(DEFAULT_PROFILE_ID);
  assert.throws(() => client.updateDraft({ ...blank(), id: 'different' }));
  assert.throws(() => client.updateDraft({ ...blank(), revision: 2 }));
  const before = clone(client.editor), session = clone(client.session), previous = count;
  await assert.rejects(client.import(JSON.stringify({ ...blank(), assignments: [{ sourceId: 'private' }] })));
  await assert.rejects(client.save({ ...blank(), name: '' }));
  assert.equal(count, previous);
  fail = true;
  await assert.rejects(client.saveDraft(), /revision conflict/);
  await assert.rejects(client.import(JSON.stringify(blank())), /revision conflict/);
  assert.deepEqual(client.editor, before); assert.deepEqual(client.session, session);
  const missing = new ProfilesClient(async () => ({}));
  await assert.rejects(missing.get(DEFAULT_PROFILE_ID), /missing its document/);
  assert.deepEqual(await missing.status(), {}); assert.equal(missing.session, null);
});

test('a save completing after another draft opens cannot replace the newly opened editor', async () => {
  let finish;
  const first = blank(), second = { ...blank(), id: '00000000-0000-4000-8000-000000000003', name: 'Other' };
  const client = new ProfilesClient(async request => request.command === 'profile_save'
    ? new Promise(resolve => { finish = () => resolve({ document: { ...first, revision: 2 } }); })
    : { document: request.id === second.id ? second : first });
  await client.open(first.id);
  const saving = client.saveDraft();
  await client.open(second.id);
  finish(); await saving;
  assert.equal(client.editor.document.id, second.id);
});

test('edits made during a save rebase onto its committed revision and can save again', async () => {
  let document = blank(), finish, delayed = true;
  const requests = [];
  const client = new ProfilesClient(async request => {
    requests.push(clone(request));
    if (request.command === 'profile_save') {
      if (request.expectedRevision !== document.revision) throw Error('revision conflict');
      if (delayed) await new Promise(resolve => { finish = resolve; });
      document = { ...clone(request.document), revision: document.revision + 1 };
    }
    return { document };
  });
  await client.open(document.id);
  client.updateDraft({ ...client.editor.document, name: 'First edit' });
  const saving = client.saveDraft();
  const newer = client.editor.document;
  newer.name = 'Later edit'; newer.editor.viewport.zoom = 2;
  client.updateDraft(newer);
  finish(); await saving;
  assert.equal(document.name, 'First edit');
  assert.equal(client.editor.document.name, 'Later edit');
  assert.equal(client.editor.document.editor.viewport.zoom, 2);
  assert.equal(client.editor.baseDocument.name, 'First edit');
  assert.equal(client.editor.baseDocument.editor.viewport.zoom, 1);
  assert.equal(client.editor.document.revision, 2);
  assert.equal(client.editor.baseRevision, 2);
  assert.equal(profileDraftDirty(client.editor), true);
  delayed = false;
  await client.saveDraft();
  assert.equal(requests.at(-1).expectedRevision, 2);
  assert.equal(document.name, 'Later edit');
  assert.equal(document.revision, 3);
  assert.equal(profileDraftDirty(client.editor), false);
});

test('a pending save does not rebase a freshly reopened editor for the same profile', async () => {
  const document = blank(); let finish;
  const client = new ProfilesClient(async request => request.command === 'profile_save'
    ? new Promise(resolve => { finish = () => resolve({ document: { ...document, revision: 2 } }); })
    : { document });
  await client.open(document.id);
  const saving = client.saveDraft();
  await client.open(document.id);
  finish(); await saving;
  assert.equal(client.editor.baseRevision, 1);
  assert.equal(client.editor.document.revision, 1);
});

test('late editor loads and closed editors cannot replace newer user choices', async () => {
  const pending = new Map();
  const client = new ProfilesClient(request => new Promise(resolve => { pending.set(request.id, resolve); }));
  const first = client.open('first'), second = client.open('second');
  pending.get('second')({ document: { ...blank(), name: 'Second' } }); await second;
  pending.get('first')({ document: { ...blank(), name: 'First' } }); await first;
  assert.equal(client.editor.document.name, 'Second');
  const closing = client.open('third'); client.close();
  pending.get('third')({ document: blank() }); await closing;
  assert.equal(client.editor, null);
});

test('successful removal clears only its own editor and failed removal preserves the draft', async () => {
  let fail = false;
  const client = new ProfilesClient(async request => {
    if (fail) throw Error('storage unavailable');
    return request.command === 'profile_get' ? { document: blank() } : { deletedId: request.id };
  });
  await client.open(DEFAULT_PROFILE_ID);
  await client.delete('other', 1); assert.ok(client.editor);
  fail = true;
  await assert.rejects(client.delete(DEFAULT_PROFILE_ID, 1), /storage unavailable/); assert.ok(client.editor);
  fail = false;
  await client.delete(DEFAULT_PROFILE_ID, 1); assert.equal(client.editor, null);
});

test('non-finite draft values cannot normalize into null and reach storage', async () => {
  let calls = 0;
  const client = new ProfilesClient(async () => { calls++; return { document: blank() }; });
  await assert.rejects(client.save({ ...withResource(), resources: [{ ...resource(), ports: [{ ...port(), freshnessMs: NaN }] }] }));
  assert.equal(calls, 0);
});

test('editor updates reject invalid nullable values before cloning and preserve the prior draft', async () => {
  const document = { ...withResource(), overrides: [{ resourceId: 'wheel', enabled: null, debounceMs: 10, invertPorts: [] }] };
  const requests = [];
  const client = new ProfilesClient(async request => {
    requests.push(request);
    return { document };
  });
  await client.open(document.id);
  const before = client.editor;
  for (const value of [NaN, Infinity, -Infinity]) {
    const invalid = client.editor.document;
    invalid.overrides[0].debounceMs = value;
    assert.throws(() => client.updateDraft(invalid), /runtime overrides/);
    assert.throws(() => profileDraft(invalid), /runtime overrides/);
    assert.deepEqual(client.editor, before);
  }
  assert.deepEqual(requests.map(request => request.command), ['profile_get']);
  await client.saveDraft();
  assert.equal(requests.at(-1).document.overrides[0].debounceMs, 10);
});
