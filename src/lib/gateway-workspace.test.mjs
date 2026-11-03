import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';
import { fileURLToPath } from 'node:url';

const compile = async name => ts.transpileModule(await readFile(new URL(name, import.meta.url), 'utf8'), {
  fileName:fileURLToPath(new URL(name,import.meta.url)),
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022,inlineSourceMap:true,inlineSources:true },
}).outputText;
class GatewayError extends Error {}
const exported = {};
vm.runInNewContext(await compile('./gateway-workspace.ts'), {
  exports: exported, setTimeout, clearTimeout,
  require(name) {
    if (name === './gateway-automation') return { candidateBackend: port => port.backend };
    if (name === './workspace-read-models/shared') return { observeWorkspaceState: () => { throw new Error('Inject observation source.'); } };
    if (name === './gateway') return { GatewayError, gatewayRequest: () => { throw new Error('Inject transport.'); }, gatewayClientTag: async () => 'sha256:local' };
    throw new Error(`Unexpected import ${name}`);
  },
},{filename:fileURLToPath(new URL('./gateway-workspace.ts',import.meta.url))});
const { GatewayWorkspace, gatewayIdentity } = exported;
const plain = value => JSON.parse(JSON.stringify(value));
const tick = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => { let resolve, reject; const promise = new Promise((done, fail) => { resolve = done; reject = fail; }); return { promise, resolve, reject }; };
const port = { path: 'tcp:localhost:1234', serialNumber: 'AABB', backend: 'stm32' };
const first = gatewayIdentity(port), second = 'stm32:ccdd';
const module = { id: 'module', online: true, active: true, registered: true, buttons: [], axes: [] };
function row(id = first, extra = {}) {
  return { id, port: { ...port, gatewayId: id }, state: { gatewayId: id, connected: false, running: false, needsReconnect: false,
    modules: [module], claimedModules: ['public-does-not-grant-ownership'] },
    session: 'connection-1', claimOwners: {}, error: '', busy: false, present: true, stale: false, ...extra };
}
function fixture(options = {}) {
  const f = { calls: [], changes: [], timers: new Map(), stops: 0, timerId: 0, intercept: undefined };
  f.workspace = new GatewayWorkspace({
    observe: listener => { f.emit = listener; return () => f.stops++; },
    request: async request => { f.calls.push(plain(request)); return f.intercept ? f.intercept(request) : { receipt: request.command }; },
    getClientTag: async () => 'sha256:local',
    onChange: (entries, error) => f.changes.push({ entries, error }),
    setTimer: (callback, delay) => { assert.equal(delay, 1000); f.timers.set(++f.timerId, callback); return f.timerId; },
    clearTimer: id => f.timers.delete(id), ...options,
  });
  f.ready = (entries = [row()], extra = {}) => f.emit({ status: 'ready', value: { ports: [port], inventoryError: '', entries },
    generation: '1', revision: '1', clockId: 'clock', session: 'broker', ...extra });
  f.fire = async () => { const [id, callback] = [...f.timers][0] ?? []; assert.ok(callback, 'A lease timer is scheduled.'); f.timers.delete(id); callback(); await tick(); };
  f.entry = id => f.workspace.entries.find(entry => entry.id === id);
  f.workspace.start();
  return f;
}

test('start observes once and native changes never trigger queries or automation', async () => {
  const f = fixture(); f.workspace.start(); await tick();
  assert.equal(f.timers.size, 0); assert.equal(f.calls.length, 0);
  f.ready([row(), row(second)]);
  assert.equal(f.workspace.entries.length, 2);
  assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
  assert.equal(f.entry(first).state.connected, false);
  f.ready([row(second, { autoConnectPaused: true })], { revision: '2' });
  assert.equal(f.entry(first), undefined); assert.equal(f.entry(second).autoConnectPaused, true);
  assert.equal(f.calls.length, 0); assert.equal(f.timers.size, 0);
});

test('identity survives aliases, and returned views cannot mutate retained native data', () => {
  assert.equal(gatewayIdentity({ ...port, serialNumber: 'aabb', path: 'other' }), first);
  assert.equal(gatewayIdentity({ ...port, serialNumber: '' }), undefined);
  assert.equal(gatewayIdentity({ ...port, backend: undefined }), undefined);
  const f = fixture(); f.ready();
  f.entry(first).state.modules[0].online = false;
  assert.equal(f.entry(first).state.modules[0].online, true);
});

test('source loss retains stale data, labels errors and blocks every request until fresh observation', async () => {
  const f = fixture(); f.ready();
  f.emit({ status: 'lost', error: 'Stream disconnected' });
  assert.equal(f.entry(first).stale, true); assert.equal(f.entry(first).state.modules.length, 1);
  assert.match(f.entry(first).error, /Stream disconnected/);
  await assert.rejects(f.workspace.request(first, { command: 'claim' }), /current gateway workspace/);
  await assert.rejects(f.workspace.retry(first), /current gateway workspace/);
  assert.equal(f.calls.length, 0);
  f.ready(); assert.equal(f.entry(first).stale, false); assert.equal(f.entry(first).error, '');
  f.emit({ status: 'connecting' }); assert.match(f.workspace.inventoryError, /Connecting/);
  f.emit({ status: 'error', error: 'Invalid frame' }); assert.equal(f.workspace.inventoryError, 'Invalid frame');
});

test('manual retry and disconnect are foreground commands; only native observations change pause or state', async () => {
  const f = fixture(); f.ready([row(first, { autoConnectPaused: true })]);
  await f.workspace.retry(first); await f.workspace.disconnect(first);
  assert.deepEqual(f.calls, [{ command: 'connect', backend: 'stm32', port: port.path, gatewayId: first }, { command: 'disconnect', gatewayId: first }]);
  assert.equal(f.entry(first).autoConnectPaused, true); assert.equal(f.entry(first).state.connected, false);
  f.ready([row(first, { autoConnectPaused: false })], { revision: '2' });
  assert.equal(f.entry(first).autoConnectPaused, false);
  await assert.rejects(f.workspace.retry('unknown'), /no longer available/);
});

test('manual operations serialize per gateway and independent gateways remain responsive', async () => {
  const f = fixture(); f.ready([row(), row(second)]);
  const blocked = deferred(); f.intercept = request => request.command === 'config_read' ? blocked.promise : { gatewayId: request.gatewayId };
  const read = f.workspace.request(first, { command: 'config_read' }, true); await tick();
  assert.equal(f.entry(first).busy, false);
  const write = f.workspace.request(first, { command: 'config_save' });
  assert.equal(f.entry(first).busy, true);
  await f.workspace.request(second, { command: 'claim' });
  assert.deepEqual(f.calls.map(call => call.command), ['config_read', 'claim']);
  blocked.resolve({ gatewayId: first }); await Promise.all([read, write]); await tick();
  assert.equal(f.calls.at(-1).command, 'config_save'); assert.equal(f.entry(first).busy, false);
  await assert.rejects(f.workspace.request(first, { command: 'claim', gatewayId: second }), /different gateway/);
});

test('command receipts and errors never replace the observed catalogue', async () => {
  const f = fixture(); f.ready();
  const receipt = { gatewayId: first, connected: true, modules: [], moduleConfigs: { private: 'dialog-only' } };
  f.intercept = () => receipt;
  assert.equal(await f.workspace.request(first, { command: 'config_read' }), receipt);
  assert.equal(f.entry(first).state.connected, false); assert.equal(f.entry(first).state.moduleConfigs, undefined);
  f.intercept = () => { const error = new GatewayError('Denied'); error.state = receipt; throw error; };
  await assert.rejects(f.workspace.request(first, { command: 'claim' }), /Denied/);
  assert.equal(f.entry(first).state.modules.length, 1); assert.equal(f.entry(first).error, 'Denied');
});

test('native busy state combines with local queued actions and survives their settlement', async () => {
  const f = fixture(); f.ready([row(first, { busy: true })]);
  assert.equal(f.entry(first).busy, true);
  await f.workspace.request(first, { command: 'config_read' }); await tick();
  assert.equal(f.entry(first).busy, true);
  const blocked = deferred(); f.intercept = () => blocked.promise;
  const action = f.workspace.request(first, { command: 'config_save' }); await tick();
  f.ready([row(first, { busy: false })], { revision: '2' });
  assert.equal(f.entry(first).busy, true);
  blocked.resolve({}); await action; await tick();
  assert.equal(f.entry(first).busy, false);
});

test('paged-tree loading preserves in-flight receipts and busy state while gating new mutations', async () => {
  const f = fixture(); f.ready(); const blocked = deferred();
  f.intercept = () => blocked.promise;
  const receipt = { moduleConfigs: { private: 'dialog-only' } };
  const active = f.workspace.request(first, { command: 'config_read' }); await tick();
  f.emit({ status: 'lost', loading: true, error: 'Loading current gateways.' });
  assert.equal(f.entry(first).stale, true); assert.equal(f.entry(first).busy, true);
  await assert.rejects(f.workspace.request(first, { command: 'claim' }), /current gateway workspace/);
  blocked.resolve(receipt); assert.equal(await active, receipt); await tick();
  assert.equal(f.entry(first).state.moduleConfigs, undefined);
  assert.equal(f.entry(first).busy, false); assert.equal(f.entry(first).stale, true);
  f.ready([row()], { revision: '2' }); assert.equal(f.entry(first).stale, false);
});

test('loading allows unchanged-owner renewal withdrawal without replacing the observed catalogue', async () => {
  const f = fixture(); await tick();
  const owned = () => row(first, { claimOwners: { module: 'sha256:local' } });
  f.ready([owned()]); const blocked = deferred(); f.intercept = () => blocked.promise;
  await f.fire(); f.emit({ status: 'lost', loading: true, error: 'Loading' });
  blocked.resolve({ workspaceRenewal: { gatewayId: first, claimedModules: [] } }); await tick();
  assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
  assert.equal(f.entry(first).stale, true); assert.equal(f.timers.size, 0);
  f.ready([owned()]); assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
});

test('a paged replacement with a changed row session fences the old in-flight receipt', async () => {
  const f = fixture(); f.ready(); const blocked = deferred(); f.intercept = () => blocked.promise;
  const active = f.workspace.request(first, { command: 'config_read' });
  const result = Promise.allSettled([active]); await tick();
  f.emit({ status: 'lost', loading: true }); f.ready([row(first, { session: 'replacement' })]);
  blocked.resolve({}); assert.equal((await result)[0].status, 'rejected');
});

for (const replacement of ['connection', 'generation', 'broker', 'loss', 'removal', 'dispose']) {
  test(`late action receipt and queued action are fenced by ${replacement}`, async () => {
    const f = fixture(); f.ready();
    const blocked = deferred(); f.intercept = () => blocked.promise;
    const active = f.workspace.request(first, { command: 'claim' });
    const queued = f.workspace.request(first, { command: 'config_save' });
    const outcome = Promise.allSettled([active, queued]); await tick();
    if (replacement === 'connection') f.ready([row(first, { session: 'new-connection' })]);
    if (replacement === 'generation') f.ready([row()], { generation: '2' });
    if (replacement === 'broker') f.ready([row()], { session: 'new-broker' });
    if (replacement === 'loss') f.emit({ status: 'lost' });
    if (replacement === 'removal') f.ready([]);
    if (replacement === 'dispose') f.workspace.dispose();
    blocked.resolve({ connected: true });
    assert.deepEqual((await outcome).map(result => result.status), ['rejected', 'rejected']);
    assert.equal(f.calls.length, 1);
  });
}

test('claims map only hashed local ownership and lease timer sends only bounded renewal commands', async () => {
  const f = fixture(); await tick();
  f.ready([row(first, { claimOwners: { module: 'sha256:local', other: 'sha256:foreign' } })]);
  assert.deepEqual(plain(f.entry(first).state.claimedModules), ['module']); assert.equal(f.timers.size, 1);
  const blocked = deferred(); f.intercept = () => blocked.promise;
  await f.fire(); assert.equal(f.timers.size, 1);
  assert.deepEqual(f.calls, [{ command: 'workspace_client_renew', gatewayId: first }]);
  blocked.resolve({ workspaceRenewal: { gatewayId: first, claimedModules: [] } }); await tick();
  assert.deepEqual(plain(f.entry(first).state.claimedModules), []); assert.equal(f.timers.size, 0);
});

test('renewal never adds claims and frequent frames with unchanged owners cannot hide withdrawal', async () => {
  const f = fixture(); await tick();
  const owned = () => row(first, { claimOwners: { module: 'sha256:local' } });
  f.ready([owned()]); f.intercept = () => ({ workspaceRenewal: { gatewayId: first, claimedModules: ['module', 'invented'] } });
  await f.fire(); assert.deepEqual(plain(f.entry(first).state.claimedModules), ['module']);
  const blocked = deferred(); f.intercept = () => blocked.promise;
  await f.fire(); f.ready([owned()], { revision: '2' });
  blocked.resolve({ workspaceRenewal: { gatewayId: first, claimedModules: [] } }); await tick();
  assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
  assert.equal(f.timers.size, 0);
  f.ready([owned()], { revision: '2' });
  assert.deepEqual(plain(f.entry(first).state.claimedModules), [], 'The same frame cannot revive expired ownership.');
  f.ready([owned()], { revision: '3' });
  assert.deepEqual(plain(f.entry(first).state.claimedModules), [], 'Newer revisions carrying an unchanged expired hash cannot restore ownership.');
  f.ready([row()], { revision: '3' }); assert.equal(f.timers.size, 0);
  assert.equal(f.calls.every(call => call.command === 'workspace_client_renew'), true);
});

test('all 64 claimed gateways renew each second with at most one pending request per gateway', async () => {
  const f = fixture(); await tick();
  const rows = Array.from({ length: 64 }, (_, index) => row(`stm32:${index}`, { claimOwners: { module: 'sha256:local' } }));
  f.ready(rows);
  const blocked = deferred(); f.intercept = request => blocked.promise.then(() => ({ workspaceRenewal: { gatewayId: request.gatewayId, claimedModules: ['module'] } }));
  await f.fire(); assert.equal(f.calls.length, 64);
  assert.equal(new Set(f.calls.map(call => call.gatewayId)).size, 64);
  await f.fire(); assert.equal(f.calls.length, 64, 'Pending renewals are never duplicated.');
  blocked.resolve(); await tick();
  await f.fire(); assert.equal(f.calls.length, 128);
  f.workspace.dispose(); assert.equal(f.timers.size, 0);
});

test('lease errors withdraw ownership conservatively without queries or acquisition', async () => {
  const f = fixture(); await tick(); f.ready([row(first, { claimOwners: { module: 'sha256:local' } })]);
  f.intercept = () => { throw new Error('Expired'); }; await f.fire();
  assert.equal(f.calls.length, 1); assert.equal(f.timers.size, 0);
  assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
  f.emit({ status: 'lost' }); assert.equal(f.timers.size, 0);
});

test('unknown renewal withdraws ownership and replay after source loss cannot revive it', async () => {
  const f = fixture(); await tick();
  const owned = () => row(first, { claimOwners: { module: 'sha256:local' } });
  f.ready([owned()]);
  f.intercept = () => ({ workspaceRenewal: { gatewayId: 'retired', claimedModules: ['module'] } });
  await f.fire(); assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
  f.emit({ status: 'lost' }); f.ready([owned()]);
  assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
  f.ready([row()]); f.ready([owned()]);
  assert.deepEqual(plain(f.entry(first).state.claimedModules), ['module'], 'Absence followed by presence establishes a new claim.');
});

test('renewal result cannot withdraw a newly changed ownership set or replacement session', async () => {
  const f = fixture(); await tick();
  const owned = extra => row(first, { claimOwners: { module: 'sha256:local', ...extra } });
  f.ready([owned()]); const blocked = deferred(); f.intercept = () => blocked.promise;
  await f.fire(); f.ready([owned({ new: 'sha256:local' })], { revision: '2' });
  blocked.resolve({ workspaceRenewal: { gatewayId: first, claimedModules: [] } }); await tick();
  assert.deepEqual(plain(f.entry(first).state.claimedModules), ['module', 'new']);
  const old = deferred(); f.intercept = () => old.promise; await f.fire();
  f.ready([{ ...owned(), session: 'connection-2' }], { revision: '3' });
  old.resolve({ workspaceRenewal: { gatewayId: first, claimedModules: [] } }); await tick();
  assert.deepEqual(plain(f.entry(first).state.claimedModules), ['module']);
});

test('dispose unregisters once, cancels timers and ignores late identity and observation delivery', async () => {
  const identity = deferred(), f = fixture({ getClientTag: () => identity.promise });
  f.ready([row(first, { claimOwners: { module: 'sha256:local' } })]);
  f.workspace.dispose(); f.workspace.dispose(); const changes = f.changes.length;
  identity.resolve('sha256:local'); f.ready([]); await tick();
  assert.equal(f.stops, 1); assert.equal(f.changes.length, changes); assert.equal(f.timers.size, 0);
  await assert.rejects(f.workspace.request(first, { command: 'claim' }), /current gateway workspace/);
});

test('renewal slots survive disposal and gate a remounted workspace with the same client and gateway', async () => {
  const owned = () => row(first, { claimOwners: { module: 'sha256:local' } });
  const old = fixture(); await tick(); old.ready([owned()]);
  const pending = deferred(); old.intercept = () => pending.promise;
  await old.fire(); assert.equal(old.calls.length, 1);
  old.workspace.dispose(); const oldChanges = old.changes.length;
  const replacement = fixture(); await tick(); replacement.ready([owned()]);
  await replacement.fire(); assert.equal(replacement.calls.length, 0, 'Old in-flight renewal holds the shared client/gateway slot.');
  pending.resolve({ workspaceRenewal: { gatewayId: first, claimedModules: [] } }); await tick();
  assert.equal(old.changes.length, oldChanges); assert.equal(old.timers.size, 0);
  assert.deepEqual(plain(replacement.entry(first).state.claimedModules), ['module']);
  replacement.intercept = () => ({ workspaceRenewal: { gatewayId: first, claimedModules: ['module'] } });
  await replacement.fire(); assert.equal(replacement.calls.length, 1);
  replacement.workspace.dispose();
});

test('loading churn and observer loss preserve the original lease deadline for existing claims', async () => {
  const f = fixture(); await tick();
  const owned = () => row(first, { claimOwners: { module: 'sha256:local' } });
  f.ready([owned()]); const scheduled = [...f.timers.keys()][0];
  for (let revision = 2; revision <= 12; revision++) {
    f.emit({ status: 'lost', loading: true, error: 'Loading' });
    assert.equal(f.entry(first).loading, true);
    assert.equal([...f.timers.keys()][0], scheduled);
    f.ready([owned()], { revision: String(revision) });
    assert.equal(f.entry(first).loading, false);
    assert.equal([...f.timers.keys()][0], scheduled);
  }
  f.emit({ status: 'lost', error: 'Offline' });
  assert.equal(f.entry(first).loading, false); assert.equal([...f.timers.keys()][0], scheduled);
  f.intercept = () => ({ workspaceRenewal: { gatewayId: first, claimedModules: ['module'] } });
  await f.fire(); assert.equal(f.calls.length, 1);
  assert.equal(f.calls[0].command, 'workspace_client_renew');
  await assert.rejects(f.workspace.request(first, { command: 'claim', moduleId: 'module' }), /current gateway workspace/);
  f.workspace.dispose();
});

test('only an exact verified private claim acknowledgment restores an expired unchanged public owner', async () => {
  const f = fixture(); await tick();
  const owned = () => row(first, { claimOwners: { module: 'sha256:local' } });
  f.ready([owned()]); f.intercept = () => ({ workspaceRenewal: { gatewayId: first, claimedModules: [] } });
  await f.fire(); f.ready([owned()], { revision: '2' });
  assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
  f.intercept = () => ({ gatewayId: second, claimedModules: ['module'], modules: [] });
  await f.workspace.request(first, { command: 'claim', moduleId: 'module' });
  assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
  f.intercept = () => ({ gatewayId: first, claimedModules: ['unrequested'], modules: [] });
  await f.workspace.request(first, { command: 'claim', moduleId: 'module' });
  assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
  f.intercept = () => ({ gatewayId: first, claimedModules: ['module'], modules: [], connected: true });
  await f.workspace.request(first, { command: 'claim', moduleId: 'module' });
  assert.deepEqual(plain(f.entry(first).state.claimedModules), ['module']);
  assert.equal(f.entry(first).state.modules.length, 1); assert.equal(f.entry(first).state.connected, false);
  f.ready([owned()], { revision: '3' }); assert.deepEqual(plain(f.entry(first).state.claimedModules), ['module']);
  f.intercept = () => ({ gatewayId: first, claimedModules: [] });
  await f.workspace.request(first, { command: 'release', moduleId: 'module' });
  f.ready([owned()], { revision: '4' }); assert.deepEqual(plain(f.entry(first).state.claimedModules), []);
  f.workspace.dispose();
});

test('an older renewal cannot withdraw a newly acknowledged claim epoch', async () => {
  const f = fixture(); await tick();
  f.ready([row(first, { claimOwners: { module: 'sha256:local' } })]);
  const blocked = deferred(); f.intercept = request => request.command === 'workspace_client_renew'
    ? blocked.promise : { gatewayId: first, claimedModules: ['module'] };
  await f.fire();
  await f.workspace.request(first, { command: 'claim', moduleId: 'module' });
  blocked.resolve({ workspaceRenewal: { gatewayId: first, claimedModules: [] } }); await tick();
  assert.deepEqual(plain(f.entry(first).state.claimedModules), ['module']);
  f.workspace.dispose();
});
