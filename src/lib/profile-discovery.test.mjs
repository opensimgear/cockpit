import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./profile-discovery', './profile-editor', './profile-graph']);
const { profileDiscoverResource: discover, profileDiscoverCompanion: discoverCompanion, profileDiscoveredPorts: choices, profilePrepareDiscoveredPorts: prepare, profileConnectDiscoveredPorts: connect } = modules.get('./profile-discovery');
const { blankProfile } = modules.get('./profile-editor');
const { graphConnectionError, graphSelectProviderPort, graphSetVirtual } = modules.get('./profile-graph');
const plain = value => JSON.parse(JSON.stringify(value));
const state = (type = 'boolean') => ({ delivery: 'state', schema: type === 'boolean' ? { type } : { type, min: 0, max: 1, unit: null }, freshnessMs: 1000, fallback: null });
const hash = digit => `sha256:${digit.repeat(64)}`;
const observed = (id, direction = 'output', contract = state(), current = true, contractHash = hash('a')) => ({ descriptor: { id, name: `Port ${id}`, direction, contract }, current, contractHash, routeIds: current ? ['native:route'] : [] });
const summary = id => ({ id, name: 'Device', resourceType: 'osg-module', availability: 'online', generation: '1', capabilityRevision: 'capabilities', portCount: 0, cachedPortCount: 0, connections: [], attributes: {}, diagnostics: [] });
const details = (id, ports, offset = 0, limit = ports.length) => ({ session: 'inventory', revision: '1', resource: { ...summary(id), portCount: ports.filter(port => port.current).length, cachedPortCount: ports.length }, evidence: [], ports: ports.slice(offset, offset + limit), offset, total: ports.length });
function draft() {
  const document = blankProfile('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa');
  document.resources = ['source', 'target'].map(id => ({ id, name: id, kind: 'device', required: false, ports: [] }));
  return { document, assignments: ['source', 'target'].map(id => ({ resourceId: id, sourceId: `native:${id}`, ports: [] })) };
}
const endpoint = (nodeId, port) => ({ nodeId, port });
const companionPage = (ports, offset = 0, limit = ports.length, publicationHash = hash('b')) => ({ schemaVersion: 1, resourceId: 'native:target', publicationHash, capabilityFingerprint: hash('c'), identityFingerprint: hash('d'), offset, total: ports.length, ports: ports.slice(offset, offset + limit), diagnostics: [] });
const prospective = (id, direction = 'input') => observed(id, direction, state(), false, hash('e'));
function companionDraft() {
  const value = draft();value.document.resources[1].kind = 'companion';
  value.document.companionAssignments = [{ resourceId: 'target', panelId: 'selected-panel-b', publicationHash: hash('b') }];
  return value;
}

test('discovery loads every short page without declaring any of a large catalogue in the graph', async () => {
  const ports = Array.from({ length: 180 }, (_, index) => observed(`native:${index}`)), offsets = [];
  const result = await discover({ async inspect(id, offset, limit) { offsets.push([offset, limit]); return details(id, ports, offset, offset ? 32 : 7); } }, 'native:source');
  assert.equal(result.ports.length, 180);
  assert.deepEqual(offsets.map(([offset]) => offset), [0, 7, 39, 71, 103, 135, 167]);
  assert.ok(offsets.every(([, limit]) => limit === 32));
  const graph = draft(), available = choices(graph, 'source', result);
  assert.equal(available.length, 180);
  assert.ok(available.every(port => port.id.startsWith('@observed:') && port.id === port.key && port.profilePortId === null));
  assert.equal(graph.document.resources[0].ports.length, 0);
  assert.equal(graph.assignments[0].ports.length, 0);
  ports[0].descriptor.name = 'Changed outside discovery';
  assert.notEqual(result.ports[0].descriptor.name, ports[0].descriptor.name);
  assert.ok(Object.isFrozen(result.ports[0].descriptor.contract));
});

test('discovery rejects mixed snapshots, duplicate variants and non-progressing pages', async () => {
  const ports = Array.from({ length: 33 }, (_, index) => observed(`native:${index}`));
  for (const change of [page => page.session = 'other', page => page.revision = '2', page => page.total++, page => page.resource.generation = '2', page => page.resource.capabilityRevision = 'changed', page => page.resource.id = 'other', page => page.offset = 0, page => page.ports = []]) {
    await assert.rejects(discover({ async inspect(id, offset, limit) { const page = details(id, ports, offset, limit); if (offset) change(page); return page; } }, 'native:source'), /changed|Malformed/);
  }
  await assert.rejects(discover({ async inspect(id) { return details(id, [ports[0], ports[0]]); } }, 'native:source'), /duplicate/);
  assert.equal((await discover({ async inspect(id) { return details(id, []); } }, 'native:source')).ports.length, 0);
});

test('saved logical IDs stay selectable while only current unselected variants are discovered', () => {
  const original = draft(), old = observed('native:button', 'output', state(), false);
  const selected = graphSelectProviderPort(original, 'source', 'native:source', old, 'button');
  const current = { ...old, current: true, contractHash: hash('b') }, other = observed('native:other'), cached = observed('native:cached', 'output', state(), false);
  const available = choices(selected, 'source', details('native:source', [old, current, other, cached]));
  assert.equal(available.length, 3);
  assert.equal(available[0].id, 'button');
  assert.equal(available[0].profilePortId, 'button');
  assert.equal(available[0].observed.contractHash, hash('a'));
  assert.equal(available[0].current, false);
  assert.deepEqual(plain(available.slice(1).map(port => port.observed.descriptor.id)), ['native:button', 'native:other']);
  assert.equal(choices(selected, 'source', details('wrong:resource', [other])).length, 1);
  assert.equal(choices(selected, 'source')[0].id, 'button');
  assert.deepEqual(plain(choices(original, 'missing')), []);
});

test('connecting discovered endpoints pins only the chosen exact contracts in one graph edit', () => {
  const original = draft(), longId = `native:${'x'.repeat(180)}`;
  const sourceCatalogue = details('native:source', [observed(longId), ...Array.from({ length: 179 }, (_, index) => observed(`unused:${index}`))]);
  const source = choices(original, 'source', sourceCatalogue)[0];
  const target = choices(original, 'target', details('native:target', [observed('native:input', 'input')]))[0];
  const connected = connect(original, endpoint('source', source), endpoint('target', target));
  assert.equal(connected.document.bindings.length, 1);
  assert.deepEqual(plain(connected.document.resources.map(resource => resource.ports.length)), [1, 1]);
  assert.deepEqual(plain(connected.assignments.map(assignment => assignment.ports.length)), [1, 1]);
  assert.equal(connected.assignments[0].ports[0].observedPortId, longId);
  assert.equal(connected.assignments[0].ports[0].expectedContractHash, hash('a'));
  assert.ok(!JSON.stringify(connected.document).includes(longId));
  assert.equal(original.document.bindings.length, 0);
  assert.equal(original.document.resources[0].ports.length, 0);
  const savedSource = choices(connected, 'source', sourceCatalogue)[0];
  const savedTarget = choices(connected, 'target')[0];
  const replaced = connect(connected, endpoint('source', savedSource), endpoint('target', savedTarget), connected.document.bindings[0].id);
  assert.equal(replaced.document.bindings[0].id, connected.document.bindings[0].id);
  assert.equal(replaced.assignments[0].ports.length, 1);
});

test('preparing incompatible choices allows explicit conversion while failed direct wiring leaves no pins', () => {
  const original = draft();
  const source = endpoint('source', choices(original, 'source', details('native:source', [observed('button')]))[0]);
  const target = endpoint('target', choices(original, 'target', details('native:target', [observed('axis', 'input', state('number'))]))[0]);
  const prepared = prepare(original, source, target);
  assert.equal(prepared.draft.document.bindings.length, 0);
  assert.equal(prepared.draft.document.resources[0].ports.length, 1);
  assert.match(graphConnectionError(prepared.draft.document, prepared.from, prepared.to), /Types/);
  assert.throws(() => connect(original, source, target), /Types/);
  assert.deepEqual(plain(original.assignments.map(assignment => assignment.ports)), [[], []]);
});

test('stale port and assignment choices cannot silently pin another provider or replace saved contracts', () => {
  const original = draft(), output = observed('native:button');
  const source = endpoint('source', choices(original, 'source', details('native:source', [output]))[0]);
  const target = endpoint('target', choices(original, 'target', details('native:target', [observed('native:input', 'input')]))[0]);
  const reassigned = plain(original); reassigned.assignments[0].sourceId = 'another:device';
  assert.throws(() => prepare(reassigned, source, target), /assignment changed/);
  const selected = graphSelectProviderPort(original, 'source', 'native:source', output, 'button');
  const savedSource = endpoint('source', choices(selected, 'source')[0]);
  const removed = plain(selected); removed.document.resources[0].ports = []; removed.assignments[0].ports = [];
  assert.throws(() => prepare(removed, savedSource, target), /changed/);
  const changed = choices(selected, 'source', details('native:source', [{ ...output, contractHash: hash('b') }])).find(port => !port.profilePortId);
  assert.throws(() => prepare(selected, endpoint('source', changed), target), /contract changed/);
  assert.equal(original.document.resources[0].ports.length, 0);
});

test('virtual and processing graph ports need no provider discovery or local pins', () => {
  const original = graphSetVirtual(draft(), { id: 'virtual-1', name: 'Controller', required: false, layout: { buttons: ['fire'], axes: [], hats: [] } });
  const port = choices(original, 'virtual-1')[0];
  assert.equal(port.id, 'fire'); assert.equal(port.profilePortId, 'fire');
  assert.equal(port.sourceId, null); assert.equal(port.observed, null); assert.equal(port.current, true);
  const source = choices(original, 'source', details('native:source', [observed('button')]))[0];
  const connected = connect(original, endpoint('source', source), endpoint('virtual-1', port));
  assert.equal(connected.document.bindings[0].to.portId, 'fire');
  assert.equal(connected.assignments.length, original.assignments.length);
});

test('companion discovery merges every prospective page of the selected publication into one immutable snapshot', async () => {
  const ports = Array.from({ length: 90 }, (_, index) => prospective(`panel-b:${index}`, index % 3 ? 'input' : 'output')), requests = [];
  const result = await discoverCompanion({ async inspect(sourceId, publicationHash, offset, limit) {
    requests.push([sourceId, publicationHash, offset, limit]);
    return companionPage(ports, offset, offset ? limit : 7, publicationHash);
  } }, 'native:target', hash('b'));
  assert.deepEqual(requests.map(([, , offset]) => offset), [0, 7, 39, 71]);
  assert.ok(requests.every(([sourceId, publicationHash, , limit]) => sourceId === 'native:target' && publicationHash === hash('b') && limit === 32));
  assert.equal(result.offset, 0);assert.equal(result.total, 90);assert.equal(result.ports.length, 90);
  assert.ok(result.ports.every(port => !port.current && port.routeIds.length === 0));
  const value = companionDraft(), available = choices(value, 'target', result);
  assert.equal(available.length, 90);assert.ok(available.every(port => !port.current && port.companionPage === result && !port.profilePortId));
  assert.equal(available.filter(port => port.direction === 'output').length, 30);
  assert.equal(available.filter(port => port.direction === 'input').length, 60);
  assert.equal(value.document.resources[1].ports.length, 0);assert.equal(value.assignments[1].ports.length, 0);
  ports[0].descriptor.name = 'Later native object mutation';
  assert.notEqual(result.ports[0].descriptor.name, ports[0].descriptor.name);assert.ok(Object.isFrozen(result.ports[0].descriptor.contract));
});

test('companion discovery rejects changed identity, capabilities, publication, pagination and claimed live delivery', async () => {
  const ports = Array.from({ length: 33 }, (_, index) => prospective(`panel-b:${index}`));
  for (const alter of [page => page.identityFingerprint = hash('f'), page => page.capabilityFingerprint = hash('f'), page => page.publicationHash = hash('a'), page => page.resourceId = 'other-companion', page => page.total++, page => page.offset = 0, page => page.schemaVersion = 2, page => page.ports = []]) {
    await assert.rejects(discoverCompanion({ async inspect(_id, _publication, offset, limit) {
      const page = companionPage(ports, offset, limit);if (offset) alter(page);return page;
    } }, 'native:target', hash('b')), /changed|Malformed/);
  }
  for (const alter of [page => page.ports[0].current = true, page => page.ports[0].routeIds = ['installed-panel-a-route']]) {
    const page = companionPage([prospective('panel-b:one')]);alter(page);
    await assert.rejects(discoverCompanion({ inspect: async () => page }, 'native:target', hash('b')), /live delivery authority/);
  }
  await assert.rejects(discoverCompanion({ inspect: async () => companionPage([ports[0], ports[0]]) }, 'native:target', hash('b')), /duplicate/);
  await assert.rejects(discoverCompanion({ inspect: async () => ({ ...companionPage([]), total: 1025 }) }, 'native:target', hash('b')), /Malformed/);
  const empty = await discoverCompanion({ inspect: async () => companionPage([]) }, 'native:target', hash('b'));
  assert.equal(empty.total, 0);assert.deepEqual(plain(empty.ports), []);assert.deepEqual(plain(choices(companionDraft(), 'target', empty)), []);
});

test('companion auto wiring ignores installed panel A and pins only the chosen prospective port from captured panel B', async () => {
  const value = companionDraft(), installedA = details('native:target', [observed('panel-a:installed-led', 'input')]);
  assert.deepEqual(plain(choices(value, 'target', installedA)), [], 'Generic native inventory describes the installed panel, not the profile publication.');
  assert.deepEqual(plain(choices(value, 'target', companionPage([prospective('panel-a:led')], 0, 1, hash('a')))), []);
  const ports = Array.from({ length: 50 }, (_, index) => prospective(`panel-b:${index}`));
  const selectedB = await discoverCompanion({ inspect: async (_id, _publication, offset, limit) => companionPage(ports, offset, limit) }, 'native:target', hash('b'));
  const source = endpoint('source', choices(value, 'source', details('native:source', [observed('physical:button')]))[0]);
  const destination = endpoint('target', choices(value, 'target', selectedB)[40]);
  assert.equal(destination.port.current, false);
  const connected = connect(value, source, destination);
  assert.equal(connected.document.bindings.length, 1);
  assert.equal(connected.document.resources[1].ports.length, 1);assert.equal(connected.assignments[1].ports.length, 1);
  assert.equal(connected.assignments[1].ports[0].observedPortId, 'panel-b:40');assert.equal(connected.assignments[1].ports[0].expectedContractHash, hash('e'));
  assert.equal(connected.document.companionAssignments[0].publicationHash, hash('b'));assert.ok(!JSON.stringify(connected).includes('panel-a:'));
  assert.equal(value.document.bindings.length, 0);assert.equal(value.document.resources[1].ports.length, 0);
  const saved = choices(connected, 'target', selectedB)[0];assert.equal(saved.profilePortId, connected.document.bindings[0].to.portId);assert.equal(saved.current, false);assert.equal(saved.companionPage, selectedB);
  const genericFallback = choices(connected, 'target', installedA);
  assert.equal(genericFallback.length, 1);assert.equal(genericFallback[0].profilePortId, saved.profilePortId);assert.equal(genericFallback[0].observed, null);assert.equal(genericFallback[0].companionPage, undefined);
});

test('prospective companion choices require exact captured panel evidence, including after a panel changes', () => {
  const value = companionDraft(), page = companionPage([prospective('panel-b:led')]);
  const source = endpoint('source', choices(value, 'source', details('native:source', [observed('physical:button')]))[0]);
  const target = endpoint('target', choices(value, 'target', page)[0]);
  const changed = plain(value);changed.document.companionAssignments[0].publicationHash = hash('a');
  assert.throws(() => prepare(changed, source, target), /Capture this exact panel/);
  const missing = plain(target);delete missing.port.companionPage;
  assert.throws(() => prepare(value, source, missing), /exact captured panel/);
  const forged = plain(target);forged.port.observed.descriptor.id = 'same-shaped-panel-a-port';
  assert.throws(() => prepare(value, source, forged), /Capture this exact panel/);
  const connected = connect(value, source, target), savedTarget = endpoint('target', choices(connected, 'target', page)[0]);
  const revised = plain(connected);revised.document.companionAssignments[0].publicationHash = hash('f');
  assert.throws(() => prepare(revised, source, savedTarget), /Capture this exact panel/);
  const genericCached = endpoint('source', { ...source.port, observed: { ...source.port.observed, current: false }, current: false });
  assert.throws(() => prepare(value, genericCached, target), /advertised/);
  assert.equal(value.assignments[1].ports.length, 0);assert.equal(value.document.bindings.length, 0);
});
