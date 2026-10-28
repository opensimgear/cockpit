import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from '../test-module-loader.mjs';

const { integrationPortPage, integrationPortTypeLabel } = (await loadModules([
  './profile-inspector/integration-ports',
])).get('./profile-inspector/integration-ports');

const numeric = (unit = null) => ({ delivery: 'state', schema: { type: 'number', min: 0, max: 100, unit }, freshnessMs: 1000, fallback: null });
const output = (id, contract = numeric(), name = id) => ({ id, name, direction: 'output', contract });

test('large integration catalogues expose both full directions beyond graph port limits', () => {
  const ports = Array.from({length:20000}, (_,index) => ({...output(`port-${index}`),direction:index%2?'input':'output'}));
  assert.equal(integrationPortPage(ports,'',0,32,'input').total,10000);
  assert.equal(integrationPortPage(ports,'',0,32,'output').total,10000);
  assert.equal(integrationPortPage(ports,'port-19999',0,32,'input').rows[0],ports[19999]);
  assert.equal(integrationPortPage(ports,'port-19998',0,32,'output').rows[0],ports[19998]);
});

test('output search filters the entire catalogue before paging and preserves exact choices and source order', () => {
  const ports = [
    ...Array.from({ length: 40 }, (_, index) => output(`channel-${index}`, numeric(), `Channel ${index}`)),
    output('speed', numeric('km/h'), 'Ground speed'),
    { id: 'speed-input', name: 'Speed input', direction: 'input', contract: numeric('km/h') },
  ];
  const result = integrationPortPage(ports, '  KM/H ', 0, 1);

  assert.equal(result.total, 1);
  assert.equal(result.pages, 1);
  assert.equal(result.offset, 0);
  assert.equal(result.rows[0], ports[40]);
});

test('outputs are the default direction and paginate in original order', () => {
  const ports = [output('first'), { id: 'input', name: 'Input', direction: 'input', contract: numeric() }, output('second')];
  const result = integrationPortPage(ports, '', 1, 1);

  assert.equal(result.total, 2);
  assert.equal(result.pages, 2);
  assert.equal(result.offset, 1);
  assert.equal(result.rows[0], ports[2]);
});

test('input direction includes action ports and filters on their compact label', () => {
  const input = { id: 'set-light', name: 'Set light', direction: 'input', contract: { delivery: 'action', semantics: 'setter', endpoint: 'light', parameters: [], readable: false, display: null, ownership: null } };
  const ports = [output('lamp-state'), input];
  const result = integrationPortPage(ports, 'action · setter', 0, 32, 'input');

  assert.equal(result.total, 1);
  assert.equal(result.rows[0], input);
  assert.equal(integrationPortPage(ports, '', 0, 32, 'input').rows[0], input);
});

test('filter matches the compact event label shown in an output row', () => {
  const pulse = output('pulse', { delivery: 'event', schema: { type: 'boolean' } });
  assert.equal(integrationPortPage([output('speed'), pulse], 'event · boolean').rows[0], pulse);
});

test('input search filters the entire catalogue before paging and preserves exact choice identity', () => {
  const ports = [
    ...Array.from({ length: 40 }, (_, index) => ({ id: `command-${index}`, name: `Command ${index}`, direction: 'input', contract: { delivery: 'action', semantics: 'setter', endpoint: `command-${index}`, parameters: [], readable: false, display: null, ownership: null } })),
    { id: 'set-flaps', name: 'Set flaps', direction: 'input', contract: { delivery: 'action', semantics: 'setter', endpoint: 'flaps', parameters: [], readable: false, display: null, ownership: null } },
    output('state-input', numeric(), 'Flaps state'),
  ];
  const result = integrationPortPage(ports, 'SET FLAPS', 0, 1, 'input');

  assert.equal(result.total, 1);
  assert.equal(result.pages, 1);
  assert.equal(result.offset, 0);
  assert.equal(result.rows[0], ports[40]);
});

test('page and size inputs are finite, integer, bounded and empty results stay at page zero', () => {
  const ports = [output('first'), output('second'), output('third')];
  const nonFinite = integrationPortPage(ports, '', Number.NaN, Number.POSITIVE_INFINITY);
  assert.equal(nonFinite.total, 3);
  assert.equal(nonFinite.offset, 0);
  assert.equal(nonFinite.page, 0);
  assert.equal(nonFinite.pages, 1);
  assert.equal(nonFinite.rows.length, ports.length);
  assert.ok(nonFinite.rows.every((row, index) => row === ports[index]));
  const clamped = integrationPortPage(ports, '', Number.NEGATIVE_INFINITY, 0);
  assert.equal(clamped.total, 3);
  assert.equal(clamped.offset, 0);
  assert.equal(clamped.page, 0);
  assert.equal(clamped.pages, 3);
  assert.equal(clamped.rows.length, 1);
  assert.equal(clamped.rows[0], ports[0]);
  const empty = integrationPortPage(ports, 'missing', 12, 4);
  assert.equal(empty.total, 0);
  assert.equal(empty.offset, 0);
  assert.equal(empty.page, 0);
  assert.equal(empty.pages, 0);
  assert.equal(empty.rows.length, 0);
  const oversized = integrationPortPage(Array.from({ length: 200 }, (_, index) => output(`p-${index}`)), '', 99, 1000);
  assert.equal(oversized.rows.length, 72);
  assert.equal(oversized.offset, 128);
  assert.equal(oversized.page, 1);
  assert.equal(oversized.pages, 2);
});

test('filtering does not mutate or copy choice objects', () => {
  const ports = [output('altitude', numeric('ft')), output('rpm', numeric('rpm'))];
  const before = structuredClone(ports);
  const result = integrationPortPage(ports, 'rpm');

  assert.deepEqual(ports, before);
  assert.equal(result.rows[0], ports[1]);
});

test('labels show compact scalar, event, and closed action semantics only', () => {
  assert.equal(integrationPortTypeLabel(numeric('ft')), 'number · ft');
  assert.equal(integrationPortTypeLabel({ delivery: 'event', schema: { type: 'boolean' } }), 'event · boolean');
  assert.equal(integrationPortTypeLabel({ delivery: 'invocation', semantics: 'one-shot', parameters: [{ id: 'secret', name: 'Secret parameter', schema: { type: 'text', maxBytes: 8, encoding: 'utf8' } }] }), 'invocation · one-shot');
  assert.equal(integrationPortTypeLabel({ delivery: 'action', semantics: 'setter', endpoint: 'private-endpoint', parameters: [], readable: true, display: null, ownership: null }), 'action · setter');
});
