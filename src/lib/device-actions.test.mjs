import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
const exported = {};
vm.runInNewContext(ts.transpileModule(await readFile(new URL('./device-actions.ts', import.meta.url), 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText, { exports: exported, TextEncoder }, { filename: new URL('./device-actions.ts', import.meta.url).pathname });
const { actionReady, nextActionRead, initialValues, actionError, outputSummary } = exported;
const plain = value => JSON.parse(JSON.stringify(value));
const module = { id: 'device', online: true, boot: 100, active: false, enabled: false, directUsb: true };
const state = { backend: 'stm32', connected: true, modules: [module] };
const action = { id: 'led.set', label: 'LED', endpoint: 'led', semantics: 'setter', readable: true,
  parameters: [{ id: 'enabled', label: 'Enabled', type: 'boolean' }, { id: 'level', label: 'Level', type: 'integer', minimum: -1, maximum: 10 },
    { id: 'mode', label: 'Mode', type: 'enum', options: ['auto', 'off'] }, { id: 'color', label: 'Color', type: 'rgb' }] };
const ready = { boot: 100, catalogueRevision: 1, status: 'ready', actions: [action], states: [{ id: action.id, values: [true, 5, 'off', 'abcdef'] }] };

test('action routing remains available for disabled HID and direct USB input ownership', () => {
  assert.equal(actionReady(state, module), true);
  for (const change of [{ connected: false }, { backend: 'pico' }, { reconnecting: true }, { layoutPending: true }]) assert.equal(actionReady({ ...state, ...change }, module), false);
  assert.equal(actionReady(state, { ...module, online: false }), false);
  assert.equal(actionReady(state, { ...module, boot: undefined }), false);
  assert.equal(actionReady(null, module), false);
  assert.equal(actionReady(state), false);
});

test('discovery only probes unread routes and does not loop on unsupported or failed firmware', () => {
  assert.equal(nextActionRead(state), module.id);
  for (const status of ['ready', 'unsupported', 'error', 'uncertain']) assert.equal(nextActionRead({ ...state, moduleActions: { device: { ...ready, status } } }), undefined);
  assert.equal(nextActionRead(null), undefined);
  assert.equal(nextActionRead({ ...state, connected: false }), undefined);
});

test('manual argument defaults use readback and respect all descriptor types', () => {
  assert.deepEqual(plain(initialValues(action)), [false, -1, 'auto', '000000']);
  assert.deepEqual(plain(initialValues(action, ready)), [true, 5, 'off', 'abcdef']);
  const text = { ...action, parameters: [{ id: 'text', label: 'Text', type: 'string', maxBytes: 4 }] };
  assert.deepEqual(plain(initialValues(text)), ['']);
  assert.equal(actionError(text, ['éé']), '');
  for (const invalid of ['ééé', '\n', '\u007f', '\u0085', 4]) assert.match(actionError(text, [invalid]), /Check text/);
  assert.equal(actionError({ ...action, parameters: [] }, []), '');
});

test('manual argument validation rejects incomplete, out of range and wrong typed values', () => {
  assert.equal(actionError(action, [false, -1, 'auto', '123456']), '');
  assert.match(actionError(action, []), /incomplete/);
  for (const values of [[1, 1, 'auto', '123456'], [true, 11, 'auto', '123456'], [true, 1.5, 'auto', '123456'], [true, 1, 'invalid', '123456'], [true, 1, 'auto', '#123456']]) assert.match(actionError(action, values), /Check/);
});

test('readback labels are explicit about unknown and unreadable states', () => {
  assert.match(outputSummary(action, ready), /Enabled: On.*Level: 5.*Mode: off.*Color: #abcdef/);
  assert.match(outputSummary(action, { ...ready, states: [{ id: action.id, values: [false, 0, 'auto', '000000'] }] }), /Enabled: Off/);
  assert.match(outputSummary(action, { ...ready, status: 'uncertain' }), /unknown/);
  assert.match(outputSummary(action), /unknown/);
  assert.match(outputSummary({ ...action, readable: false }, ready), /does not provide/);
  const empty = { ...action, parameters: [] };
  assert.equal(outputSummary(empty, { ...ready, states: [{ id: action.id, values: [] }] }), 'Applied.');
});
