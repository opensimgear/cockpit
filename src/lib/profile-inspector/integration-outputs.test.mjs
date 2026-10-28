import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from '../test-module-loader.mjs';

const { integrationOutputPage, outputTypeLabel } = (await loadModules([
  './profile-inspector/integration-outputs',
])).get('./profile-inspector/integration-outputs');

const numeric = (unit = null) => ({ delivery: 'state', schema: { type: 'number', min: 0, max: 100, unit }, freshnessMs: 1000, fallback: null });
const output = (id, contract = numeric(), name = id) => ({ id, name, direction: 'output', contract });

test('search filters the entire catalogue before paging and preserves exact choices and source order', () => {
  const ports = [
    ...Array.from({ length: 40 }, (_, index) => output(`channel-${index}`, numeric(), `Channel ${index}`)),
    output('speed', numeric('km/h'), 'Ground speed'),
    { id: 'speed-input', name: 'Speed input', direction: 'input', contract: numeric('km/h') },
  ];
  const result = integrationOutputPage(ports, '  KM/H ', 0, 1);

  assert.equal(result.total, 1);
  assert.equal(result.pages, 1);
  assert.equal(result.offset, 0);
  assert.equal(result.rows[0], ports[40]);
});

test('output rows exclude inputs and paginate in original order', () => {
  const ports = [output('first'), { id: 'input', name: 'Input', direction: 'input', contract: numeric() }, output('second')];
  const result = integrationOutputPage(ports, '', 1, 1);

  assert.equal(result.total, 2);
  assert.equal(result.pages, 2);
  assert.equal(result.offset, 1);
  assert.equal(result.rows[0], ports[2]);
});

test('filter matches the compact event label shown in the row', () => {
  const pulse = output('pulse', { delivery: 'event', schema: { type: 'boolean' } });
  assert.equal(integrationOutputPage([output('speed'), pulse], 'event · boolean').rows[0], pulse);
});

test('page and size inputs are finite, integer, bounded and empty results stay at page zero', () => {
  const ports = [output('first'), output('second'), output('third')];
  const nonFinite = integrationOutputPage(ports, '', Number.NaN, Number.POSITIVE_INFINITY);
  assert.equal(nonFinite.total, 3);
  assert.equal(nonFinite.offset, 0);
  assert.equal(nonFinite.page, 0);
  assert.equal(nonFinite.pages, 1);
  assert.equal(nonFinite.rows.length, ports.length);
  assert.ok(nonFinite.rows.every((row, index) => row === ports[index]));
  const clamped = integrationOutputPage(ports, '', Number.NEGATIVE_INFINITY, 0);
  assert.equal(clamped.total, 3);
  assert.equal(clamped.offset, 0);
  assert.equal(clamped.page, 0);
  assert.equal(clamped.pages, 3);
  assert.equal(clamped.rows.length, 1);
  assert.equal(clamped.rows[0], ports[0]);
  const empty = integrationOutputPage(ports, 'missing', 12, 4);
  assert.equal(empty.total, 0);
  assert.equal(empty.offset, 0);
  assert.equal(empty.page, 0);
  assert.equal(empty.pages, 0);
  assert.equal(empty.rows.length, 0);
  const oversized = integrationOutputPage(Array.from({ length: 200 }, (_, index) => output(`p-${index}`)), '', 99, 1000);
  assert.equal(oversized.rows.length, 72);
  assert.equal(oversized.offset, 128);
  assert.equal(oversized.page, 1);
  assert.equal(oversized.pages, 2);
});

test('filtering does not mutate or copy choice objects', () => {
  const ports = [output('altitude', numeric('ft')), output('rpm', numeric('rpm'))];
  const before = structuredClone(ports);
  const result = integrationOutputPage(ports, 'rpm');

  assert.deepEqual(ports, before);
  assert.equal(result.rows[0], ports[1]);
});

test('labels show compact scalar, event, and closed action semantics only', () => {
  assert.equal(outputTypeLabel(numeric('ft')), 'number · ft');
  assert.equal(outputTypeLabel({ delivery: 'event', schema: { type: 'boolean' } }), 'event · boolean');
  assert.equal(outputTypeLabel({ delivery: 'invocation', semantics: 'one-shot', parameters: [{ id: 'secret', name: 'Secret parameter', schema: { type: 'text', maxBytes: 8, encoding: 'utf8' } }] }), 'invocation · one-shot');
  assert.equal(outputTypeLabel({ delivery: 'action', semantics: 'setter', endpoint: 'private-endpoint', parameters: [], readable: true, display: null, ownership: null }), 'action · setter');
});
