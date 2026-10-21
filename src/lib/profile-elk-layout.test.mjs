import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import ELK from 'elkjs/lib/elk.bundled.js';
import { loadModules } from './test-module-loader.mjs';

const { profileElkGraph, layoutElkDiagram } = (await loadModules(['./profile-elk-layout'])).get('./profile-elk-layout');
const width = 288, height = 188, plain = value => JSON.parse(JSON.stringify(value));
const edge = (from, to, id = `${from}-${to}`, fromPort = 'out', toPort = 'in') => ({ id, from: { nodeId: from, portId: fromPort }, to: { nodeId: to, portId: toPort } });
const block = (id, x = 0, y = 0) => ({ id, position: { x, y }, width, height });
const elk = new ELK();
// The browser worker structured-clones requests. Do the same at the VM test
// boundary so the bundled engine receives arrays from its own JavaScript realm.
const engine = { layout: graph => elk.layout(plain(graph)) };

function assertComplete(nodes, bindings, result) {
  result = plain(result);
  assert.deepEqual(result.positions.map(value => value.nodeId).sort(), nodes.map(value => value.id).sort());
  assert.deepEqual(result.wireRoutes.map(value => value.bindingId).sort(), bindings.map(value => value.id).sort());
  const positions = new Map(result.positions.map(value => [value.nodeId, value]));
  for (const route of result.wireRoutes) {
    const binding = bindings.find(value => value.id === route.bindingId);
    const from = nodes.find(value => value.id === binding.from.nodeId), to = nodes.find(value => value.id === binding.to.nodeId);
    const fromPosition = positions.get(from.id), toPosition = positions.get(to.id);
    const fromAnchor = from.outputAnchors?.[binding.from.portId], toAnchor = to.inputAnchors?.[binding.to.portId];
    assert.deepEqual(plain(route.points[0]), { x: fromPosition.x + (fromAnchor ? fromAnchor.x - from.position.x : from.width ?? width), y: fromPosition.y + (fromAnchor ? fromAnchor.y - from.position.y : (from.height ?? height) - 20) });
    assert.deepEqual(plain(route.points.at(-1)), { x: toPosition.x + (toAnchor ? toAnchor.x - to.position.x : 0), y: toPosition.y + (toAnchor ? toAnchor.y - to.position.y : (to.height ?? height) - 20) });
    assert.equal(route.automatic, true);
    assert.ok(route.points.length >= 2 && route.points.length <= 128);
    assert.equal(route.points[0].y, route.points[1].y);
    assert.ok(route.points[1].x > route.points[0].x);
    assert.equal(route.points.at(-1).y, route.points.at(-2).y);
    assert.ok(route.points.at(-2).x < route.points.at(-1).x);
    for (let index = 1; index < route.points.length; index++) {
      const a = route.points[index - 1], b = route.points[index];
      assert.ok(a.x === b.x || a.y === b.y);
      for (const node of nodes) {
        const position = positions.get(node.id), w = node.width ?? width, h = node.height ?? height;
        const crosses = a.x === b.x
          ? a.x > position.x && a.x < position.x + w && Math.max(a.y, b.y) > position.y && Math.min(a.y, b.y) < position.y + h
          : a.y > position.y && a.y < position.y + h && Math.max(a.x, b.x) > position.x && Math.min(a.x, b.x) < position.x + w;
        assert.equal(crosses, false, `${route.bindingId} crosses block ${node.id}`);
      }
    }
  }
}

test('ELK Layered lays out the actual DCS Feature Lab with every physical port and binding intact', async () => {
  const fixture = JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/feature-lab-geometry.json', import.meta.url), 'utf8'));
  const before = JSON.stringify(fixture);
  const result = await layoutElkDiagram(fixture.nodes, fixture.bindings, width, height, engine);
  assert.equal(result.positions.length, 42);
  assert.equal(result.wireRoutes.length, 73);
  assertComplete(fixture.nodes, fixture.bindings, result);
  assert.equal(JSON.stringify(fixture), before, 'Layout must not mutate the profile or its geometry');
  const again = await layoutElkDiagram(fixture.nodes, fixture.bindings, width, height, engine);
  assert.deepEqual(plain(again), plain(result), 'The same profile has a deterministic layout');
});

test('parallel signal edges remain distinct while sharing a physical resource footer', async () => {
  const nodes = [block('source'), block('sink', 700)], bindings = [edge('source', 'sink', 'first', 'rpm', 'axis-1'), edge('source', 'sink', 'second', 'speed', 'axis-2')];
  const prepared = profileElkGraph(nodes, bindings, width, height);
  assert.equal(prepared.graph.children[0].ports.length, 1);
  assert.equal(prepared.graph.children[1].ports.length, 1);
  assert.equal(new Set(prepared.graph.edges.map(value => value.id)).size, 2);
  assert.equal(prepared.graph.children[0].layoutOptions['elk.portConstraints'], 'FIXED_POS');
  const result = await layoutElkDiagram(nodes, bindings, width, height, engine);
  assertComplete(nodes, bindings, result);
});

test('named transform ports keep their actual offsets and fixed dimensions', async () => {
  const nodes = [block('source', 12, 24), { id: 'transform', position: { x: 500, y: 300 }, width: 224, height: 146, inputAnchors: { first: { x: 500, y: 350 }, second: { x: 500, y: 382 } }, outputAnchors: { result: { x: 724, y: 350 } } }, block('sink', 900)];
  const bindings = [edge('source', 'transform', 'first', 'a', 'first'), edge('source', 'transform', 'second', 'b', 'second'), edge('transform', 'sink', 'result', 'result')];
  const prepared = profileElkGraph(nodes, bindings, width, height);
  assert.equal(prepared.graph.children[1].ports.length, 3);
  assertComplete(nodes, bindings, await layoutElkDiagram(nodes, bindings, width, height, engine));
});

test('cycles, backwards edges, self connections, and isolated blocks have complete valid routes', async () => {
  const nodes = ['a', 'b', 'sink', 'isolated'].map(id => block(id));
  const bindings = [edge('a', 'b'), edge('b', 'a'), edge('b', 'sink'), edge('a', 'a')];
  const result = await layoutElkDiagram(nodes, bindings, width, height, engine);
  assertComplete(nodes, bindings, result);
  assert.ok(result.positions.every(value => Number.isFinite(value.x) && Number.isFinite(value.y)));
});

test('empty graphs and graphs with only isolated blocks are supported', async () => {
  assert.deepEqual(plain(await layoutElkDiagram([], [], width, height, engine)), { positions: [], wireRoutes: [] });
  const nodes = [block('one'), block('two')];
  const result = await layoutElkDiagram(nodes, [], width, height, engine);
  assertComplete(nodes, [], result);
  assert.notDeepEqual(plain(result.positions[0]), plain(result.positions[1]));
});

test('invalid source geometry and missing endpoints fail before the engine is invoked', async () => {
  const never = { layout: () => { throw new Error('Engine must not be called'); } };
  await assert.rejects(layoutElkDiagram([block('a'), block('a')], [], width, height, never), /identifiers must be unique/);
  await assert.rejects(layoutElkDiagram([block('a')], [edge('a', 'missing')], width, height, never), /missing block missing/);
  await assert.rejects(layoutElkDiagram([{ ...block('a'), width: NaN }], [], width, height, never), /invalid dimensions/);
  await assert.rejects(layoutElkDiagram([{ ...block('a'), outputAnchors: { out: { x: 10, y: 80 } } }, block('b')], [edge('a', 'b')], width, height, never), /output border/);
});

test('generated engine IDs never collide with block or binding IDs', () => {
  const nodes = [block('__cockpit_elk_root'), block('__cockpit_elk_port_0')];
  const bindings = [edge(nodes[0].id, nodes[1].id, '__cockpit_elk_port_1')];
  const { graph } = profileElkGraph(nodes, bindings, width, height);
  const ids = [graph.id, ...graph.children.map(node => node.id), ...graph.children.flatMap(node => node.ports.map(port => port.id)), ...graph.edges.map(binding => binding.id)];
  assert.equal(new Set(ids).size, ids.length);
});

test('malformed, partial, shifted-port, and obstructed engine responses never produce partial changes', async () => {
  const nodes = [block('a'), block('b')], bindings = [edge('a', 'b')];
  const alter = change => ({ layout: async graph => { const result = await engine.layout(graph); change(result); return result; } });
  await assert.rejects(layoutElkDiagram(nodes, bindings, width, height, alter(result => result.children.pop())), /incomplete graph/);
  await assert.rejects(layoutElkDiagram(nodes, bindings, width, height, alter(result => result.edges.pop())), /incomplete graph/);
  await assert.rejects(layoutElkDiagram(nodes, bindings, width, height, alter(result => result.children[0].x = Infinity)), /invalid coordinates/);
  await assert.rejects(layoutElkDiagram(nodes, bindings, width, height, alter(result => result.children[0].ports[0].y++)), /moved its physical ports/);
  await assert.rejects(layoutElkDiagram(nodes, bindings, width, height, alter(result => result.edges[0].sections = [])), /missing sections/);
  await assert.rejects(layoutElkDiagram(nodes, bindings, width, height, alter(result => result.edges[0].sections[0].endPoint.y++)), /physical ports/);
  await assert.rejects(layoutElkDiagram(nodes, bindings, width, height, alter(result => {
    const section = result.edges[0].sections[0], a = section.startPoint, b = section.endPoint;
    section.bendPoints = [{ x: a.x + 32, y: a.y }, { x: a.x + 32, y: a.y - 40 }, { x: b.x + width / 2, y: a.y - 40 }, { x: b.x + width / 2, y: b.y }, { x: b.x - 16, y: b.y }];
  })), /crosses block b/);
});
