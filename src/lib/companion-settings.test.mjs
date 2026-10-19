import assert from 'node:assert/strict';
import test from 'node:test';
import { newDocument, publishDocument } from '../../packages/companion-designer/src/index.ts';
import { createCompanionSettingsAutosave, saveCompanionSettings } from './companion-settings.ts';
import { boardScreenSize, companionScreenSize, panelScreenReason } from './companion-screen.ts';

test('ESP screen matching uses saved dimensions and checks every page', async () => {
  const h = host();
  h.state.host.clients[0].kind = 'esp32';
  h.state.firmware = { 'screen-a': { profile: 'sunton-3248s035r', version: 'test' } };
  const size = companionScreenSize(h.state, 'screen-a');
  assert.deepEqual(size, { width: 480, height: 320 });
  assert.deepEqual(boardScreenSize('waveshare-lcd7-s3'), { width: 800, height: 480 });
  assert.equal(companionScreenSize(h.state, 'screen-b'), undefined);
  const panel = structuredClone(await publishDocument(newDocument('Screen panel'), []));
  await assert.rejects(saveCompanionSettings(input({ panelHash: panel.publicationHash, publications: [panel] }), h.send), /480 × 320/);
  assert.deepEqual(h.calls.map(call => call.command), ['companion_status']);
  for (const viewport of panel.document.presentation.viewports) { viewport.width = 480; viewport.height = 320; }
  assert.equal(panelScreenReason(panel, size), '');
  assert.match(panelScreenReason(panel), /identify its screen size/);
  panel.document.ui.pages.push({ id: 'second', title: 'Second' });
  assert.match(panelScreenReason(panel, size), /Every panel page/);
});

const input = (changes = {}) => ({ companionId: 'screen-a', name: 'Screen A', panelHash: '', publications: [], ...changes });
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
const settle = () => new Promise(resolve => setImmediate(resolve));

function host(instances = []) {
  const calls = [];
  const state = { host: { clients: [{ companionId: 'screen-a', name: 'Screen A' }, { companionId: 'screen-b', name: 'Screen B' }], instances: structuredClone(instances) } };
  const send = async (command, payload = {}) => {
    calls.push({ command, payload: structuredClone(payload) });
    if (command === 'companion_publish_begin') return { uploadId: 'upload' };
    if (command === 'companion_update') state.host.clients.find(client => client.companionId === payload.companionId).name = payload.name;
    if (command === 'companion_unassign') state.host.instances = state.host.instances.filter(instance => instance.instanceId !== payload.instanceId);
    if (command === 'companion_assign') { state.host.instances = state.host.instances.filter(instance => instance.instanceId !== payload.instanceId); state.host.instances.push({ ...payload }); }
    return structuredClone(state);
  };
  return { calls, state, send };
}

test('unchanged settings require only fresh status, including unknown library content and multiple assignments', async () => {
  const h = host([{ instanceId: 'one', companionId: 'screen-a', publicationHash: 'not-loaded' }, { instanceId: 'two', companionId: 'screen-a', publicationHash: 'another-panel' }]);
  await saveCompanionSettings(input({ panelHash: 'not-loaded' }), h.send);
  assert.deepEqual(h.calls.map(call => call.command), ['companion_status']);
});

test('renaming trims the name and never uploads, changes or disarms any assignment', async () => {
  const instances = [{ instanceId: 'one', companionId: 'screen-a', publicationHash: 'current' }, { instanceId: 'two', companionId: 'screen-a', publicationHash: 'other' }];
  const h = host(instances);
  await saveCompanionSettings(input({ name: '  Renamed screen  ', panelHash: 'current' }), h.send);
  assert.deepEqual(h.calls, [{ command: 'companion_status', payload: {} }, { command: 'companion_update', payload: { companionId: 'screen-a', name: 'Renamed screen' } }]);
  assert.deepEqual(h.state.host.instances, instances);
});

test('name-only edits preserve a panel changed externally after the editor snapshot', async () => {
  const panel = await publishDocument(newDocument('Old editor panel'), []);
  const instances = [{ instanceId: 'owned', companionId: 'screen-a', publicationHash: 'externally-changed-panel' }];
  const h = host(instances);
  await saveCompanionSettings(input({ name: 'New name', panelHash: panel.publicationHash, publications: [panel], saveName: true, savePanel: false }), h.send);
  assert.deepEqual(h.calls.map(call => call.command), ['companion_status', 'companion_update']);
  assert.deepEqual(h.state.host.instances, instances);
});

test('panel-only edits preserve a name changed externally after the editor snapshot', async () => {
  const panel = await publishDocument(newDocument('New panel'), []);
  const h = host([{ instanceId: 'owned', companionId: 'screen-a', publicationHash: 'old-panel' }]);
  h.state.host.clients[0].name = 'Externally changed name';
  await saveCompanionSettings(input({ panelHash: panel.publicationHash, publications: [panel], saveName: false, savePanel: true }), h.send);
  assert.equal(h.state.host.clients[0].name, 'Externally changed name');
  assert.equal(h.calls.filter(call => call.command === 'companion_update').length, 0);
  assert.equal(h.state.host.instances[0].publicationHash, panel.publicationHash);
});

test('panel changes publish before assignment, reuse exact instance identity and leave other companions alone', async () => {
  const panel = await publishDocument(newDocument('New panel'), []);
  const h = host([{ instanceId: 'owned', companionId: 'screen-a', publicationHash: 'old' }, { instanceId: 'unrelated', companionId: 'screen-b', publicationHash: 'keep' }]);
  await saveCompanionSettings(input({ panelHash: panel.publicationHash, publications: [panel] }), h.send);
  assert.deepEqual(h.calls.at(-1), { command: 'companion_assign', payload: { companionId: 'screen-a', instanceId: 'owned', publicationHash: panel.publicationHash } });
  assert.ok(h.calls.findIndex(call => call.command === 'companion_publish_commit') < h.calls.findIndex(call => call.command === 'companion_assign'));
  assert.equal(h.calls.filter(call => call.command === 'companion_update').length, 0);
  assert.deepEqual(h.state.host.instances.find(instance => instance.instanceId === 'unrelated'), { instanceId: 'unrelated', companionId: 'screen-b', publicationHash: 'keep' });
  await saveCompanionSettings(input({ panelHash: panel.publicationHash, publications: [panel] }), h.send);
  assert.equal(h.calls.filter(call => call.command === 'companion_assign').length, 1);
});

test('No panel removes only its captured assignment and a new assignment gets a fresh instance ID', async () => {
  const h = host([{ instanceId: 'owned', companionId: 'screen-a', publicationHash: 'old' }, { instanceId: 'keep', companionId: 'screen-b', publicationHash: 'other' }]);
  await saveCompanionSettings(input(), h.send);
  assert.deepEqual(h.calls.at(-1), { command: 'companion_unassign', payload: { instanceId: 'owned' } });
  assert.deepEqual(h.state.host.instances.map(instance => instance.instanceId), ['keep']);
  await saveCompanionSettings(input(), h.send);
  assert.equal(h.calls.filter(call => call.command === 'companion_unassign').length, 1);
  const panel = await publishDocument(newDocument('Panel'), []);
  await saveCompanionSettings(input({ panelHash: panel.publicationHash, publications: [panel] }), h.send);
  assert.match(h.calls.at(-1).payload.instanceId, /^[\da-f]{8}-(?:[\da-f]{4}-){3}[\da-f]{12}$/);
});

test('invalid names and identities fail before native requests; unpaired and ambiguous assignments fail before writes', async () => {
  for (const invalid of [input({ name: '' }), input({ name: '  ' }), input({ name: 'a'.repeat(81) }), input({ name: 'Bad\nname' }), input({ companionId: 'invalid id' })]) {
    const h = host();
    await assert.rejects(saveCompanionSettings(invalid, h.send));
    assert.deepEqual(h.calls, []);
  }
  const h = host([{ instanceId: 'one', companionId: 'screen-a', publicationHash: 'first' }, { instanceId: 'two', companionId: 'screen-a', publicationHash: 'second' }]);
  await assert.rejects(saveCompanionSettings(input({ name: 'Changed', panelHash: 'different' }), h.send), /multiple assignments/);
  await assert.rejects(saveCompanionSettings(input({ companionId: 'missing' }), h.send), /no longer paired/);
  await assert.rejects(saveCompanionSettings(input({ companionId: 'screen-b', name: 'Changed', panelHash: 'missing' }), h.send), /saved panel/);
  assert.ok(h.calls.every(call => call.command === 'companion_status'));
});

test('settings capture all routing values and the publication list before the first await', async () => {
  const panel = await publishDocument(newDocument('Captured panel'), []);
  const h = host(), gate = deferred();
  const value = input({ name: 'Captured name', panelHash: panel.publicationHash, publications: [panel] });
  const saving = saveCompanionSettings(value, async (command, payload) => { if (command === 'companion_status') await gate.promise; return h.send(command, payload); });
  value.companionId = 'screen-b'; value.name = 'Wrong name'; value.panelHash = ''; value.publications.length = 0;
  gate.resolve(); await saving;
  assert.equal(h.state.host.clients[0].name, 'Captured name');
  assert.equal(h.state.host.clients[1].name, 'Screen B');
  assert.equal(h.state.host.instances[0].companionId, 'screen-a');
  assert.equal(h.state.host.instances[0].publicationHash, panel.publicationHash);
});

test('autosave debounces and coalesces name edits without any write on opening or idle polling', async t => {
  t.mock.timers.enable({ apis: ['setTimeout', 'Date'], now: 0 });
  const h = host(), states = [];
  const autosave = createCompanionSettingsAutosave(h.send, state => states.push(state));
  assert.deepEqual(h.calls, []);
  autosave.schedule(input({ name: 'First' }));
  t.mock.timers.tick(200);
  autosave.schedule(input({ name: 'Latest' }));
  t.mock.timers.tick(399); await settle();
  assert.deepEqual(h.calls, []);
  t.mock.timers.tick(1); await settle();
  assert.equal(h.state.host.clients[0].name, 'Latest');
  assert.equal(h.calls.filter(call => call.command === 'companion_update').length, 1);
  assert.deepEqual(states.at(-1), { companionId: 'screen-a', saving: false, pending: false, error: '' });
  t.mock.timers.tick(10000); await settle();
  assert.equal(h.calls.length, 2);
});

test('autosave serializes requests and retains newest edits for each companion across navigation', async () => {
  const h = host(), gate = deferred(), entered = deferred();
  let active = 0, maxActive = 0, delayed = false;
  const autosave = createCompanionSettingsAutosave(async (command, payload) => {
    active++; maxActive = Math.max(maxActive, active);
    try { if (command === 'companion_update' && !delayed) { delayed = true; entered.resolve(); await gate.promise; } return await h.send(command, payload); }
    finally { active--; }
  });
  autosave.schedule(input({ name: 'First' }));
  const saving = autosave.flush();
  await entered.promise;
  autosave.schedule(input({ name: 'Superseded' }));
  autosave.schedule(input({ name: 'Latest' }));
  autosave.schedule(input({ companionId: 'screen-b', name: 'Other screen' }));
  gate.resolve(); await saving;
  assert.equal(maxActive, 1);
  assert.deepEqual(h.calls.filter(call => call.command === 'companion_update').map(call => call.payload), [{ companionId: 'screen-a', name: 'First' }, { companionId: 'screen-a', name: 'Latest' }, { companionId: 'screen-b', name: 'Other screen' }]);
});

test('failed writes retain their snapshot without timer retry, then explicit flush retries and clears the error', async t => {
  t.mock.timers.enable({ apis: ['setTimeout', 'Date'], now: 0 });
  const h = host(), states = [];
  let fail = true, attempts = 0;
  const autosave = createCompanionSettingsAutosave(async (command, payload) => { if (command === 'companion_update') { attempts++; if (fail) throw new Error('Disk unavailable'); } return h.send(command, payload); }, state => states.push(state));
  autosave.schedule(input({ name: 'Retained draft' }));
  t.mock.timers.tick(400); await settle();
  assert.deepEqual(states.at(-1), { companionId: 'screen-a', saving: false, pending: true, error: 'Disk unavailable' });
  t.mock.timers.tick(10000); await settle(); assert.equal(attempts, 1);
  fail = false; await autosave.flush();
  assert.equal(attempts, 2);
  assert.equal(h.state.host.clients[0].name, 'Retained draft');
  assert.deepEqual(states.at(-1), { companionId: 'screen-a', saving: false, pending: false, error: '' });
});

test('a newer edit recovers after an in-flight failure and a blank name supersedes an older pending name', async () => {
  const h = host(), gate = deferred(), entered = deferred(), states = [];
  let fail = true;
  const autosave = createCompanionSettingsAutosave(async (command, payload) => { if (command === 'companion_update' && fail) { entered.resolve(); await gate.promise; fail = false; throw new Error('Temporary failure'); } return h.send(command, payload); }, state => states.push(state));
  autosave.schedule(input({ name: 'Failed older edit' }));
  const saving = autosave.flush(); await entered.promise;
  autosave.schedule(input({ name: 'Recovered edit' })); gate.resolve(); await saving;
  assert.equal(h.state.host.clients[0].name, 'Recovered edit');
  assert.equal(states.at(-1).error, '');
  h.calls.length = 0;
  autosave.schedule(input({ name: 'Must not save' }));
  autosave.schedule(input({ name: ' ' })); await autosave.flush();
  assert.deepEqual(h.calls, []);
  assert.match(states.at(-1).error, /name/);
  autosave.schedule(input({ name: 'Valid again' })); assert.equal(states.at(-1).error, '');
  await autosave.flush(); assert.equal(h.state.host.clients[0].name, 'Valid again');
});

test('discard cancels pending or failed settings before removal without dropping another companion', async () => {
  const h = host(), states = [], autosave = createCompanionSettingsAutosave(h.send, state => states.push(state));
  autosave.schedule(input({ name: 'Do not save' }));
  autosave.schedule(input({ companionId: 'screen-b', name: 'Keep queued' }));
  autosave.discard('screen-a'); await autosave.flush();
  assert.equal(h.state.host.clients[0].name, 'Screen A');
  assert.equal(h.state.host.clients[1].name, 'Keep queued');
  autosave.schedule(input({ name: '' })); await autosave.flush();
  autosave.discard('screen-a'); const count = h.calls.length; await autosave.flush();
  assert.equal(h.calls.length, count);
  assert.deepEqual(states.at(-1), { companionId: 'screen-a', saving: false, pending: false, error: '' });
});
