import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { ProfileCompilerClient, COMPILATION_SECTIONS } from './profile-compilation.ts';

const clone = value => JSON.parse(JSON.stringify(value));
const document = JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/default.json', import.meta.url), 'utf8'));
const id = '00000000-0000-4000-8000-000000000002';
const hash = `sha256:${'a'.repeat(64)}`, fingerprint = `sha256:${'b'.repeat(64)}`;
const diagnostics = () => ({ code: 'waitingForBaseline', severity: 'warning', message: 'Acquire a fresh baseline.',
  location: { resourceId: 'wheel', portId: 'pressed', nodeId: null, edgeIds: ['edge'] } });
const totals = () => Object.fromEntries(COMPILATION_SECTIONS.map(section => [section, 0]));
const summary = () => ({ schemaVersion: 1, compilationId: id, profileId: document.id, profileRevision: document.revision,
  programHash: hash, documentSemanticHash: `sha256:${'c'.repeat(64)}`, captureFingerprint: fingerprint, readiness: 'blocked', diagnostics: [], sectionTotals: totals(), budgets: null });
const page = (preview, section, items, offset = 0, limit = 32) => ({ compilationId: preview.compilationId, profileId: preview.profileId,
  profileRevision: preview.profileRevision, programHash: preview.programHash, captureFingerprint: preview.captureFingerprint,
  section, offset, total: items.length, items: items.slice(offset, offset + limit) });
const endpoint = () => ({ nodeId: 'wheel', portId: 'pressed' });
const contract = () => ({ delivery: 'state', schema: { type: 'boolean' }, freshnessMs: 1500, fallback: false });
const route = () => ({ id: 'hid:wheel', transport: 'hid', providerGeneration: '3', attributes: {}, baselineReady: true, writable: false, priority: 10 });
const token = () => ({ inventorySession: 'inventory', resourceId: 'native-wheel', generation: '3', capabilityRevision: hash,
  routeId: route().id, providerSession: 'provider', providerGeneration: '3', portId: 'button:0', contractHash: hash, access: 'read' });
const source = () => ({ endpoint: endpoint(), inventoryResourceId: 'native-wheel', observedPortId: 'button:0', contract: contract(), token: token(), route: route(), readiness: 'ready' });

test('the native-produced summary fixture is accepted with its exact hash format', async () => {
  const native = JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/compilation-summary.json', import.meta.url), 'utf8'));
  assert.deepEqual(await new ProfileCompilerClient(async () => native).compile(document), native);
  const unprefixed = clone(native); unprefixed.programHash = unprefixed.programHash.slice(7);
  await assert.rejects(new ProfileCompilerClient(async () => unprefixed).compile(document), /preview/);
});

test('shape-valid drafts with semantic errors reach native diagnostics without saving', async () => {
  const invalid = clone(document);
  invalid.bindings = [{ id: 'missing-binding', from: { nodeId: 'missing-source', portId: 'value' }, to: { nodeId: 'missing-sink', portId: 'value' } }];
  const result = summary(); result.programHash = null; result.documentSemanticHash = null; result.readiness = 'invalid';
  const problem = diagnostics(); problem.code = 'missingPort'; problem.location.edgeIds = ['missing-binding'];
  result.diagnostics = [problem]; result.sectionTotals.diagnostics = 1;
  const calls = [];
  const preview = await new ProfileCompilerClient(async request => { calls.push(request); return result; }).compile(invalid);
  assert.equal(preview.readiness, 'invalid');
  assert.deepEqual(calls, [{ command: 'profile_compile', document: invalid }]);
});

test('compiling captures a validated immutable draft without selecting or saving it', async () => {
  const calls = [], result = summary(), original = clone(document);
  const client = new ProfileCompilerClient(async request => { calls.push(request); original.name = 'Later edit'; return result; });
  const preview = await client.compile(original);
  assert.deepEqual(calls.map(request => request.command), ['profile_compile']);
  assert.equal(calls[0].document.name, document.name);
  assert.equal(preview.profileRevision, document.revision);
  result.sectionTotals.nodes = 10;
  assert.equal(preview.sectionTotals.nodes, 0);
  assert.ok(Object.isFrozen(preview.sectionTotals));
  const invalid = clone(document); invalid.editor.viewport.x = NaN;
  await assert.rejects(client.compile(invalid));
  assert.equal(calls.length, 1);
});

test('malformed summaries never replace the requested draft preview', async () => {
  for (const change of [
    value => { value.schemaVersion = 2; }, value => { value.compilationId = 'wrong'; },
    value => { value.profileId = id; }, value => { value.profileRevision++; },
    value => { value.programHash = 'wrong'; }, value => { value.documentSemanticHash = 'wrong'; },
    value => { value.captureFingerprint = 'wrong'; }, value => { value.readiness = 'active'; },
    value => { value.diagnostics = [diagnostics()]; }, value => { value.sectionTotals.nodes = -1; },
    value => { value.budgets = {}; }, value => { value.profileRevision = Infinity; },
  ]) {
    const invalid = summary(); change(invalid);
    await assert.rejects(new ProfileCompilerClient(async () => invalid).compile(document), /preview/);
  }
  const valid = summary(); valid.programHash = null; valid.documentSemanticHash = null; valid.readiness = 'invalid';
  valid.diagnostics = [diagnostics()]; valid.sectionTotals.diagnostics = 1;
  assert.equal((await new ProfileCompilerClient(async () => valid).compile(document)).programHash, null);
  valid.budgets = { usedSources: 0, virtualTargets: 0, sourcePorts: 0, processingNodes: 0, sinkPorts: 0,
    bindings: 0, stateSlots: 0, eventQueueCapacity: 512, actionQueueCapacity: 256, timerCapacity: 256 };
  assert.equal((await new ProfileCompilerClient(async () => valid).compile(document)).budgets.timerCapacity, 256);
});

test('all inspection sections carry exact captured identities and immutable typed items', async () => {
  const entries = {
    diagnostics: [diagnostics()], nodes: [{ nodeId: 'convert', inputPorts: ['in'], outputPorts: ['out'], incomingEdgeIds: ['one'], outgoingEdgeIds: ['two'] }],
    order: ['convert'], sources: [source()],
    sinks: [{ ...source(), inventoryResourceId: null, observedPortId: null, contract: contract(), token: null, route: null, adapter: 'virtual-controller' }],
    branches: [{ sink: endpoint(), sourcePorts: [endpoint()], processingNodes: ['convert'], edgeIds: ['one'], required: true, readiness: 'blocked' }],
    controllers: [{ id: 'virtual-a', name: 'Controller A', buttons: 1, axes: 0, hats: 0, connectedPorts: ['button'], readiness: 'blocked' }],
    visibility: [{ resourceId: 'wheel', inventoryResourceId: 'native-wheel', mode: 'hidden-while-active', readiness: 'blocked' }],
  };
  const preview = summary(); for (const section of COMPILATION_SECTIONS) preview.sectionTotals[section] = entries[section].length;
  const client = new ProfileCompilerClient(async request => page(preview, request.section, entries[request.section], request.offset, request.limit));
  for (const section of COMPILATION_SECTIONS) {
    const inspected = await client.inspectAll(preview, section);
    assert.deepEqual(inspected.items, entries[section]);
    assert.ok(Object.isFrozen(inspected.items));
  }
});

test('short native pages advance by their actual count without replay or mixed snapshots', async () => {
  const entries = Array.from({ length: 67 }, (_, index) => `node-${index}`), offsets = [], preview = summary(); preview.sectionTotals.order = entries.length;
  const client = new ProfileCompilerClient(async request => { offsets.push(request.offset); return page(preview, 'order', entries, request.offset, Math.min(request.limit, 7)); });
  assert.deepEqual((await client.inspectAll(preview, 'order')).items, entries);
  assert.deepEqual(offsets, [0, 7, 14, 21, 28, 35, 42, 49, 56, 63]);
  for (const field of ['compilationId', 'profileId', 'profileRevision', 'programHash', 'captureFingerprint', 'section', 'offset', 'total']) {
    const changing = new ProfileCompilerClient(async request => ({ ...page(preview, 'order', entries, request.offset), [field]: typeof preview[field] === 'number' ? 999 : 'changed' }));
    await assert.rejects(changing.inspect(preview, 'order'), /immutable preview/);
  }
  await assert.rejects(new ProfileCompilerClient(async request => ({ ...page(preview, 'order', entries, request.offset), items: [] })).inspectAll(preview, 'order'), /immutable preview/);
  await assert.rejects(new ProfileCompilerClient(async () => null).inspect(preview, 'order'), /immutable preview/);
});

test('invalid bounds and malformed section items cannot enter an inspection result', async () => {
  const preview = summary(); preview.sectionTotals.order = 1;
  const client = new ProfileCompilerClient(async () => { throw new Error('must not send'); });
  for (const [offset, limit] of [[-1, 1], [0, 0], [0, 65], [Infinity, 1]]) await assert.rejects(client.inspect(preview, 'order', offset, limit), /bounded/);
  await assert.rejects(client.inspect(preview, 'unexpected'), /bounded/);
  const malformed = {
    diagnostics: { ...diagnostics(), severity: 'urgent' }, nodes: { nodeId: 'node', inputPorts: 'invalid' }, order: '',
    sources: { ...source(), contract: { delivery: 'state', schema: { type: 'number', min: NaN, max: 1, unit: null } } },
    sinks: { ...source(), adapter: 'unknown' }, branches: { sink: endpoint(), sourcePorts: [], required: 'yes' },
    controllers: { id: 'controller', name: 'Controller', buttons: 129, axes: 0, hats: 0, connectedPorts: [], readiness: 'blocked' },
    visibility: { resourceId: 'wheel', inventoryResourceId: null, mode: 'hidden', readiness: 'blocked' },
  };
  for (const section of COMPILATION_SECTIONS) {
    const current = summary(); current.sectionTotals[section] = 1;
    await assert.rejects(new ProfileCompilerClient(async () => page(current, section, [malformed[section]])).inspect(current, section), /immutable preview/);
  }
  for (const bad of [{ ...source(), token: { ...token(), routeId: 'wrong' } }, { ...source(), token: { ...token(), access: 'invoke' } }, { ...source(), route: { ...route(), priority: NaN } }, { ...source(), route: { ...route(), priority: 2147483648 } }]) {
    const current = summary(); current.sectionTotals.sources = 1;
    await assert.rejects(new ProfileCompilerClient(async () => page(current, 'sources', [bad])).inspect(current, 'sources'), /immutable preview/);
  }
});

test('expired previews remain explicit failures without recapture or automatic compilation', async () => {
  const calls = [], preview = summary();
  const client = new ProfileCompilerClient(async request => { calls.push(request); throw new Error('The compilation preview expired; compile the current draft again.'); });
  await assert.rejects(client.inspect(preview, 'sources'), /expired/);
  assert.deepEqual(calls.map(request => request.command), ['profile_compilation_inspect']);
});
