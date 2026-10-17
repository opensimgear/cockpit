import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { ProfileInventoryClient, resourcePortError, resourceTokensEqual } from './profile-inventory.ts';
import { ProfilesClient, parseProfileDocument } from './profiles.ts';

const clone = value => JSON.parse(JSON.stringify(value));
const route = () => ({ id: 'gateway:a/module:1', transport: 'can', providerGeneration: 'epoch:2/boot:3', attributes: { gatewayId: 'gateway:a', bus: 'can' }, baselineReady: true, writable: true, priority: 10 });
const summary = (id = 'native/resource:1') => ({ id, name: 'Wheel', resourceType: 'osg-module', availability: 'online', generation: '9007199254740999', capabilityRevision: 'sha256:capabilities', portCount: 1, cachedPortCount: 1,
  connections: [{ scope: 'can:gateway:a', key: 'module:1', route: route() }], attributes: { physicalIdentity: 'evidence:not-a-portable-assignment' }, diagnostics: [] });
const descriptor = (id = 'axis:0') => ({ id, name: 'Axis', direction: 'output', contract: { delivery: 'state', schema: { type: 'number', min: 0, max: 1, unit: 'ratio' }, freshnessMs: 1500, fallback: null } });
const observed = (id = 'axis:0', hash = 'sha256:port', current = true) => ({ descriptor: descriptor(id), contractHash: hash, routeIds: current ? [route().id] : [], current });
const listing = (resources, offset = 0, limit = 32) => ({ session: 'native-inventory-session', revision: '9007199254741000', offset, total: resources.length, resources: resources.slice(offset, offset + limit), inventoryDiagnostics: [], readOnly: false });
const inspected = (ports, offset = 0, limit = 128) => ({ session: 'native-inventory-session', revision: '9007199254741000', resource: { ...summary(), portCount: ports.length, cachedPortCount: ports.length }, evidence: [{ kind: 'verified-physical', value: 'verified-hardware-id' }], ports: ports.slice(offset, offset + limit), offset, total: ports.length });
const token = () => ({ inventorySession: 'native-inventory-session', resourceId: 'native/resource:1', generation: '9007199254740999', capabilityRevision: 'sha256:capabilities', routeId: route().id, providerSession: 'provider-session', providerGeneration: route().providerGeneration, portId: 'axis:0', contractHash: 'sha256:port', access: 'read' });
const action = () => ({ id: 'led.rgb.set', name: 'Set RGB', direction: 'input', contract: { delivery: 'action', parameters: [{ id: 'rgb', name: 'Color', schema: { type: 'rgb' } }], semantics: 'setter', endpoint: 'led:rgb', readable: true, display: null, ownership: { scope: 'endpoint', maxLeaseMs: 5000 } } });

test('typed inventory retains opaque identities, transport attributes and offline blocks', async () => {
  const resources = [summary('can:gateway:a/module:same'), summary('can:gateway:b/module:same'), { ...summary('controller:no-serial-a'), resourceType: 'host-controller' }, { ...summary('controller:no-serial-b'), resourceType: 'host-controller' }, { ...summary('companion:tablet'), resourceType: 'companion' }, { ...summary('game:dcs'), resourceType: 'game-integration', availability: 'unavailable', connections: [], diagnostics: ['Catalogue not available'] }, { ...summary('removed:wheel'), availability: 'offline', connections: [] }];
  const calls = [];
  const client = new ProfileInventoryClient(async request => { calls.push(request); return listing(resources, request.offset, request.limit); });
  const result = await client.listAll();
  assert.deepEqual(result.resources.map(resource => resource.id), resources.map(resource => resource.id));
  assert.equal(result.resources[0].generation, '9007199254740999');
  assert.equal(result.resources[0].connections[0].route.attributes.gatewayId, 'gateway:a');
  assert.equal(result.resources.at(-1).availability, 'offline');
  assert.equal(result.resources[5].availability, 'unavailable');
  resources[0].name = 'Changed provider object';
  assert.equal(result.resources[0].name, 'Wheel');
  assert.ok(Object.isFrozen(result.resources[0].connections[0].route));
  assert.ok(calls.every(request => request.command === 'profile_resources_list'));
});

test('resource paging is complete and never assembles different native snapshots', async () => {
  const resources = Array.from({ length: 65 }, (_, index) => summary(`opaque:${index}`)), offsets = [];
  const client = new ProfileInventoryClient(async request => { offsets.push(request.offset); return listing(resources, request.offset, request.limit); });
  assert.equal((await client.listAll()).resources.length, 65);
  assert.deepEqual(offsets, [0, 32, 64]);
  for (const field of ['session', 'revision', 'total']) {
    const changing = new ProfileInventoryClient(async request => ({ ...listing(resources, request.offset, request.limit), ...(request.offset ? { [field]: field === 'total' ? 66 : 'changed' } : {}) }));
    await assert.rejects(changing.listAll(), /changed|Malformed/);
  }
  const duplicate = new ProfileInventoryClient(async request => listing([summary(), summary()], request.offset, request.limit));
  await assert.rejects(duplicate.listAll(), /duplicate identities/);
  assert.equal((await new ProfileInventoryClient(async () => listing([])).listAll()).total, 0);
});

test('malformed or non-progressing resource pages fail without retry loops', async () => {
  for (const change of [{ resources: [] }, { offset: 1 }, { total: 2049 }, { revision: 1 }, { session: '' }, { resources: [{ ...summary(), generation: 1 }] }, { resources: [{ ...summary(), availability: 'maybe' }] }, { resources: [{ ...summary(), connections: [{ scope: 'can', key: 'a', route: { ...route(), writable: 'yes' } }] }] }]) {
    await assert.rejects(new ProfileInventoryClient(async () => ({ ...listing([summary()]), ...change })).list(), /Malformed/);
  }
  const client = new ProfileInventoryClient(async () => assert.fail('Invalid page reached native'));
  for (const [offset, limit] of [[-1, 32], [2049, 32], [0, 0], [0, 129], [0.5, 32]]) await assert.rejects(client.list(offset, limit), /bounded/);
});

test('inspection preserves changed port variants and their exact advertised constraints', async () => {
  const ports = [observed('axis:0', 'sha256:old', false), ...Array.from({ length: 129 }, (_, index) => observed(`axis:${index}`, `sha256:${index}`))];
  const calls = [];
  const client = new ProfileInventoryClient(async request => { calls.push(request); return inspected(ports, request.offset, request.limit); });
  const result = await client.inspectAll('native/resource:1');
  assert.equal(result.ports.length, 130);
  assert.equal(result.ports.filter(port => port.descriptor.id === 'axis:0').length, 2);
  assert.equal(result.ports[0].current, false); assert.deepEqual(result.ports[0].routeIds, []);
  assert.equal(result.ports[1].descriptor.contract.fallback, null);
  assert.deepEqual(calls.map(request => request.offset), [0, 32, 64, 96, 128]);
  ports[0].descriptor.name = 'New observation';
  assert.equal(result.ports[0].descriptor.name, 'Axis');
  assert.ok(Object.isFrozen(result.ports[0].descriptor.contract.schema));
});

test('reply-budget short pages advance by actual count and retain aggregate resource limits', async () => {
  const resources = Array.from({ length: 8 }, (_, index) => summary(`resource:${index}`));
  const ports = Array.from({ length: 8 }, (_, index) => observed(`axis:${index}`));
  const offsets = [];
  const client = new ProfileInventoryClient(async request => {
    offsets.push([request.command, request.offset]);
    if (request.command === 'profile_resources_list') return listing(resources, request.offset, 3);
    const result = inspected(ports, request.offset, 3);
    result.resource.portCount = 26000;
    result.resource.attributes = Object.fromEntries(Array.from({ length: 300 }, (_, index) => [`key${index}`, `value${index}`]));
    result.resource.diagnostics = Array.from({ length: 300 }, (_, index) => `diagnostic${index}`);
    result.evidence = Array.from({ length: 300 }, (_, index) => ({ kind: 'local-path', value: `path:${index}` }));
    return result;
  });
  assert.equal((await client.listAll()).resources.length, 8);
  const details = await client.inspectAll(summary().id);
  assert.equal(details.ports.length, 8);
  assert.equal(details.resource.portCount, 26000);
  assert.equal(details.evidence.length, 300);
  assert.deepEqual(offsets.map(([, offset]) => offset), [0, 3, 6, 0, 3, 6]);
});

test('inspection refuses stale generations, mixed capabilities, repeated variants and wrong identities', async () => {
  const ports = Array.from({ length: 129 }, (_, index) => observed(`axis:${index}`, `sha256:${index}`));
  for (const change of [{ session: 'old' }, { revision: '5' }, { total: 130 }, { resource: { ...summary(), generation: '3' } }, { resource: { ...summary(), capabilityRevision: 'changed' } }]) {
    const client = new ProfileInventoryClient(async request => ({ ...inspected(ports, request.offset, request.limit), ...(request.offset ? change : {}) }));
    await assert.rejects(client.inspectAll('native/resource:1'), /changed|Malformed/);
  }
  const duplicate = new ProfileInventoryClient(async () => inspected([observed(), observed()]));
  await assert.rejects(duplicate.inspectAll('native/resource:1'), /duplicate port variants/);
  for (const change of [{ ports: [] }, { resource: { ...summary(), id: 'wrong' } }, { ports: [{ ...observed(), descriptor: null }] }, { evidence: [{ kind: 'guess', value: 'name' }] }, { ports: [{ ...observed(), routeIds: [4] }] }]) await assert.rejects(new ProfileInventoryClient(async () => ({ ...inspected([observed()]), ...change })).inspect('native/resource:1'), /Malformed/);
  assert.equal((await new ProfileInventoryClient(async () => inspected([])).inspectAll('native/resource:1')).ports.length, 0);
});

test('state/event/action DTOs retain units, enum ordering, text bytes, display and ownership', () => {
  const schemas = [{ type: 'boolean' }, { type: 'integer', min: -32768, max: 32767, unit: null }, { type: 'number', min: 0, max: 1, unit: 'ratio' }, { type: 'text', maxBytes: 16, encoding: 'ascii' }, { type: 'enum', values: ['on', 'off'] }, { type: 'rgb' }];
  for (const schema of schemas) {
    assert.equal(resourcePortError({ ...descriptor(), contract: { delivery: 'event', schema } }), '');
    assert.equal(resourcePortError({ ...action(), contract: { ...action().contract, parameters: [{ id: 'arg', name: 'Argument', schema }] } }), '');
  }
  for (const [schema, fallback] of [[schemas[0], false], [schemas[1], 7], [schemas[2], 0.5], [schemas[3], ''], [schemas[3], 'text'], [schemas[4], 'on'], [schemas[5], 'A1b2c3']]) assert.equal(resourcePortError({ ...descriptor(), contract: { delivery: 'state', schema, freshnessMs: 500, fallback } }), '');
  for (const [schema, fallback] of [[schemas[0], 'false'], [schemas[1], 0.5], [schemas[2], 5], [schemas[3], 'é'], [schemas[3], '\n'], [schemas[3], 'x'.repeat(17)], [schemas[4], 'unknown'], [schemas[5], 'color']]) assert.ok(resourcePortError({ ...descriptor(), contract: { delivery: 'state', schema, freshnessMs: 500, fallback } }));
  assert.equal(resourcePortError({ ...action(), contract: { ...action().contract, parameters: [{ id: 'text', name: 'Text', schema: { type: 'text', maxBytes: 16, encoding: 'utf8' } }], display: { rows: 1, columns: 16, encoding: 'utf8' } } }), '');
  const unsupported = [null, { ...descriptor(), id: '' }, { ...descriptor(), direction: 'both' }, { ...descriptor(), contract: { delivery: 'event', schema: { type: 'opaque' } } }, { ...descriptor(), contract: { ...descriptor().contract, freshnessMs: 0 } }, { ...descriptor(), contract: { ...descriptor().contract, fallback: Infinity } }, { ...descriptor(), contract: { ...descriptor().contract, schema: { type: 'integer', min: 0.5, max: 1, unit: null } } }, { ...descriptor(), contract: { delivery: 'event', schema: { type: 'enum', values: ['a', 'a'] } } }, { ...action(), direction: 'output' }, { ...action(), contract: { ...action().contract, parameters: Array(5).fill(action().contract.parameters[0]) } }, { ...action(), contract: { ...action().contract, display: { rows: 1, columns: 64, encoding: 'ascii' } } }, { ...action(), contract: { ...action().contract, ownership: { scope: 'device', maxLeaseMs: 0 } } }];
  for (const value of unsupported) assert.ok(resourcePortError(value));
  assert.ok(resourcePortError({ ...action(), contract: { ...action().contract, parameters: [action().contract.parameters[0], action().contract.parameters[0]] } }));
  const displayAction = { ...action(), contract: { ...action().contract, parameters: [{ id: 'text', name: 'Text', schema: { type: 'text', maxBytes: 16, encoding: 'utf8' } }], display: { rows: 1, columns: 16, encoding: 'utf8' } } };
  for (const change of [{ semantics: 'one-shot' }, { readable: false }, { parameters: [] }, { parameters: action().contract.parameters }, { display: { rows: 1, columns: 17, encoding: 'utf8' } }, { display: { rows: 1, columns: 16, encoding: 'ascii' } }]) assert.ok(resourcePortError({ ...displayAction, contract: { ...displayAction.contract, ...change } }));
});

test('tokens bind every ownership/capability epoch exactly without numeric coercion', () => {
  assert.equal(resourceTokensEqual(token(), clone(token())), true);
  for (const field of Object.keys(token())) {
    const changed = { ...token(), [field]: field === 'generation' ? '9007199254741000' : field === 'access' ? 'invoke' : `changed:${field}` };
    assert.equal(resourceTokensEqual(token(), changed), false, field);
  }
  assert.equal(resourceTokensEqual(token(), { ...token(), generation: 9007199254740999 }), false);
});

test('token acquisition and validation never dispatch an action or mutate native session state', async () => {
  const calls = [];
  let current = token();
  const client = new ProfileInventoryClient(async request => {
    calls.push(clone(request));
    if (request.command === 'profile_resources_token') return { resourceToken: { ...current, access: request.access } };
    if (!resourceTokensEqual(request.resourceToken, { ...current, access: request.access })) throw Error('Stale native route token');
    return { valid: true };
  });
  for (const access of ['read', 'write', 'invoke']) {
    const captured = await client.token(current.resourceId, current.portId, access);
    assert.equal(captured.access, access); assert.ok(Object.isFrozen(captured));
    await client.validate(captured);
  }
  const captured = await client.token(current.resourceId, current.portId);
  current = { ...current, providerGeneration: 'new boot' };
  await assert.rejects(client.validate(captured), /Stale native route/);
  assert.equal(captured.providerGeneration, 'epoch:2/boot:3');
  assert.ok(calls.every(request => ['profile_resources_token', 'profile_resources_validate'].includes(request.command)));
});

test('mismatched/malformed tokens are rejected and provider failures propagate', async () => {
  for (const change of [{ portId: 'wrong' }, { resourceId: 'wrong' }, { access: 'invoke' }, { generation: '00' }]) await assert.rejects(new ProfileInventoryClient(async () => ({ resourceToken: { ...token(), ...change } })).token(token().resourceId, token().portId), /does not match/);
  const missing = new ProfileInventoryClient(async () => ({}));
  await assert.rejects(missing.token('r', 'p'), /does not match/);
  await assert.rejects(missing.validate(token()), /stale/);
  const local = new ProfileInventoryClient(async () => assert.fail('Mismatched token reached native'));
  await assert.rejects(local.validate(token(), 'different'), /does not match/);
  await assert.rejects(local.validate(token(), token().portId, 'invoke'), /does not match/);
  const failing = new ProfileInventoryClient(async () => { throw Error('Provider unavailable'); });
  await assert.rejects(failing.list(), /Provider unavailable/);
  await assert.rejects(failing.inspect('native/resource:1'), /Provider unavailable/);
});

test('inventory capability errors preserve dirty editor documents and captured profile sessions', async () => {
  const document = parseProfileDocument(await readFile(new URL('../../tests/fixtures/profiles/blank.json', import.meta.url), 'utf8'));
  const session = { selectedProfileId: document.id, selectionMode: 'manual', state: 'paused', appliedSnapshot: null, fault: null };
  const profileCalls = [];
  const profiles = new ProfilesClient(async request => { profileCalls.push(request.command); return request.command === 'profile_get' ? { document } : { session }; });
  await profiles.status();
  await profiles.open(document.id);
  profiles.updateDraft({ ...profiles.editor.document, name: 'Unsaved inventory draft' });
  const beforeEditor = profiles.editor, beforeSession = profiles.session;
  const inventory = new ProfileInventoryClient(async request => request.command === 'profile_resources_validate' ? { valid: false } : { ...listing([summary()]), revision: 2 });
  await assert.rejects(inventory.listAll(), /Malformed/);
  await assert.rejects(inventory.validate(token()), /stale/);
  assert.deepEqual(profiles.editor, beforeEditor);
  assert.equal(profiles.editor.document.name, 'Unsaved inventory draft');
  assert.strictEqual(profiles.session, beforeSession);
  assert.deepEqual(profileCalls, ['profile_status', 'profile_get']);
});

test('graph invocation contracts cannot be advertised as observed provider capabilities', () => {
  const graphOnly={...action(),direction:'output',contract:{delivery:'invocation',parameters:[],semantics:'one-shot'}};
  assert.ok(resourcePortError(graphOnly));
  assert.ok(resourcePortError({...descriptor(),contract:{...descriptor().contract,privateMetadata:true}}));
  assert.ok(resourcePortError({...descriptor(),contract:{...descriptor().contract,schema:{type:'boolean',rawCalibration:{min:0}}}}));
});
