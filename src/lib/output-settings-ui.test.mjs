import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./ui-settings-guard','./profile-desktop-runtime','./visible-observation']);
const plain = value => JSON.parse(JSON.stringify(value));
const initial = () => ({
  desktop: { enabled: false, qualified: true, state: 'stopped', reason: '', scope: { kind: 'application' } },
  visibility: { enabled: true, qualified: true, settingsError: null, selfAccess: { ready: true, message: 'Capture access configured.' } },
  inventory: { virtualDevices: [{ id: 'vjoy:1', name: 'vJoy Device 1', status: 'free' }], diagnostics: [],
    drivers: { supported: true, vjoy: { installed: true, version: '2.2.2.0', error: null }, hidhide: { installed: true, version: '1.5.230.0', error: null } } },
});
async function harness(send, { nativeApp = true, overrides = {} } = {}) {
  const file = new URL('./OutputSettings.svelte', import.meta.url);
  const source = (await readFile(file, 'utf8')).replace('</script>', 'visibility=seed.visibility;inventory=seed.inventory;capture({refresh,enableDesktop,manageDriver,observeDesktopSettings,get state(){return {desktop,visibility,inventory,busy,error};}});</script>');
  const { js } = compile(source, { filename: file.pathname, generate: 'server' });
  const exports = {}, requests = [], invocations = [], setups = [], seed = { ...initial(), ...overrides };
  let handlers,settingsVersion=1;
  const native=async request=>{
    if(send)return send(request,seed);
    if(request.command.startsWith('profile_local_settings_')){
      if(request.command==='profile_local_settings_save'){seed.desktop.enabled=request.settings.enabled;seed.desktop.scope=request.settings.scope;settingsVersion++;}
      return{localSettings:{scope:'desktop',gameId:null,revision:`sha256:${String(settingsVersion).padStart(64,'0')}`,settings:{enabled:seed.desktop.enabled,scope:seed.desktop.scope},profileSession:{owner:null}}};
    }
    if(request.command==='profile_desktop_status')return{desktop:{...seed.desktop}};
    return{systemInventory:seed.inventory,controllers:{visibility:{...seed.visibility,...(request.command==='controller_visibility_enable'?{enabled:request.enabled}:{})}}};
  };
  vm.runInNewContext(ts.transpileModule(js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
    exports, seed, TextEncoder, capture(value) { handlers = value; },
    require(name) {
      if (name === 'svelte/internal/server') return server;
      if (name === 'svelte') return svelte;
      if (name === './OptionalDependencies.svelte') return { default: (_payload, props) => { setups.push(props.only); } };
      if (name.endsWith('.svelte')) return { default: () => {} };
      if (name === '@tauri-apps/api/core') return { isTauri: () => nativeApp, invoke: async command => { invocations.push(command); } };
      if (modules.has(name)) return modules.get(name);
      if (name === './gateway') return { gatewayRequest: async request => { requests.push(plain(request));return native(request); } };
      if (name === './streams/shared') return { SharedObservations: class { subscribe() {} close() {} } };
      throw Error(name);
    },
  });
  const html = render(exports.default).body;
  const observe = (payload = seed.desktop, { source = 'desktop-settings', topic = 'settings.desktop', kind = 'snapshot' } = {}) => handlers.observeDesktopSettings({ status:'ready', frame:{ source, topic, kind, payload:{enabled:payload.enabled,qualified:payload.qualified,scope:payload.scope,reason:payload.reason,error:payload.error??null} } });
  return { html, handlers, requests, invocations, setups, observe, seed };
}

test('Settings owns driver tools and machine enablement while application scope belongs to profile blocks', async () => {
  const h = await harness();
  for (const label of ['Manage', 'Enable desktop output on this computer', 'vJoy Device 1', 'free']) assert.ok(h.html.includes(label), label);
  assert.ok(!h.html.includes('Enable device hiding on this computer'));
  assert.ok(!h.html.includes('Capture access configured.'));
  assert.equal((h.html.match(/role="status">Enabled/g) ?? []).length, 2);
  assert.ok(!h.html.includes('1 configured'));
  assert.ok(h.html.includes('Choose the application scope in each profile'));
  assert.ok(!h.html.includes('<select'));
  assert.deepEqual(h.requests, []);
  assert.equal(h.handlers.state.desktop, null, 'desktop settings wait for their observation baseline');
  h.observe();
  assert.equal(h.handlers.state.desktop.enabled, false);
  await h.handlers.refresh();
  assert.deepEqual(h.requests.map(request=>request.command),['system_inventory','controller_visibility_status']);h.requests.length=0;
  await h.handlers.refresh(true);
  assert.deepEqual(h.requests[0], { command:'system_inventory', refresh:true });h.requests.length=0;
  await h.handlers.enableDesktop(true);
  assert.deepEqual(h.requests, [
    { command:'profile_local_settings_get',scope:'desktop' },
    { command:'profile_desktop_status' },
    { command: 'profile_local_settings_save',scope:'desktop',expectedSettingsRevision:`sha256:${'1'.padStart(64,'0')}`,expectedActiveOwner:null,settings:{enabled:true,scope:{kind:'application'}} },
    {command:'profile_desktop_status'},
  ]);
  assert.equal(h.handlers.state.desktop.enabled, false, 'only the committed observation updates displayed state');
  await h.handlers.manageDriver('open_vjoy');
  await h.handlers.manageDriver('open_hidhide');
  assert.deepEqual(h.invocations, ['open_vjoy', 'open_hidhide']);
});

test('failed setup refresh and enablement report native errors without claiming a saved change', async () => {
  const h = await harness(async () => { throw new Error('Stop the running profile before changing setup.'); });
  h.observe();
  await h.handlers.enableDesktop(true);
  assert.equal(h.handlers.state.desktop.enabled, false);
  assert.match(h.handlers.state.error, /Stop the running profile/);
  await h.handlers.refresh();
  assert.match(h.handlers.state.error, /Stop the running profile/);
  assert.equal(h.handlers.state.busy, '');
  assert.deepEqual(h.requests.map(request => request.command), ['profile_local_settings_get','system_inventory','controller_visibility_status']);
});

test('desktop settings observation validates committed snapshots and clears the toggle on loss', async () => {
  const h = await harness();
  h.observe();
  assert.equal(h.handlers.state.desktop.enabled, false);
  h.observe({ ...initial().desktop, enabled:true, scope:{kind:'desktop-wide'} }, { kind:'change' });
  assert.equal(h.handlers.state.desktop.enabled, true);
  assert.equal(h.handlers.state.desktop.scope.kind, 'desktop-wide');
  h.observe(initial().desktop, { source:'profiles' });
  assert.equal(h.handlers.state.desktop, null, 'wrong source data is rejected');
  h.handlers.observeDesktopSettings({status:'lost',error:'overflow'});
  assert.equal(h.handlers.state.desktop, null, 'loss withdraws the last settings baseline');
});

test('desktop toggle refuses to configure when the committed settings differ from the shown baseline', async () => {
  const h = await harness();
  h.observe();
  h.seed.desktop.enabled = true;
  await h.handlers.enableDesktop(true);
  assert.deepEqual(h.requests.map(request=>request.command), ['profile_local_settings_get','profile_desktop_status']);
  assert.equal(h.handlers.state.desktop.enabled, false);
  assert.match(h.handlers.state.error, /settings changed/i);
});

test('separate output cards retain unavailable reasons and disable unsupported switches', async () => {
  const h = await harness(null, { overrides: {
    desktop: { ...initial().desktop, qualified: false, reason: 'Desktop output is unsupported.' },
    visibility: { ...initial().visibility, qualified: false, reason: 'HidHide is not installed.' },
    inventory: { virtualDevices: [], diagnostics: ['vJoy driver is unavailable.'] },
  } });
  for (const label of ['Virtual controllers', 'Device hiding', 'Keyboard and mouse']) assert.ok(h.html.includes(`aria-label="${label}"`));
  for (const reason of ['HidHide is not installed.', 'vJoy driver is unavailable.']) assert.ok(h.html.includes(reason));
  assert.equal(h.handlers.state.desktop, null, 'unsupported desktop reason is unavailable before a baseline');
  const switches = h.html.match(/<input[^>]*role="switch"[^>]*>/g);
  assert.equal(switches.length, 1);
  assert.ok(switches.every(input => /\bdisabled\b/.test(input)));
  assert.deepEqual(h.requests, []);
});

test('browser settings explain where native driver tools are available', async () => {
  const h = await harness(null, { nativeApp: false });
  assert.ok(h.html.includes('Driver installers and configuration apps open from the native Cockpit desktop app.'));
  assert.ok(!h.html.includes('>Manage</button>'));
  assert.deepEqual(h.invocations, []);
});

test('installed driver versions sit beside their names and setup stays inside missing driver cards', async () => {
  const installed = await harness();
  assert.ok(installed.html.includes('2.2.2.0')); assert.ok(installed.html.includes('1.5.230.0'));
  assert.deepEqual(installed.setups, []);
  assert.ok(!installed.html.includes('Driver installers'));
  const missing = await harness(null, { overrides: { inventory: { ...initial().inventory, drivers: { supported: true,
    vjoy: { installed: false, version: null, error: null }, hidhide: { installed: false, version: null, error: null } } } } });
  assert.deepEqual(missing.setups, ['vjoy', 'hidhide']);
  assert.ok(!missing.html.includes('>Enabled</span>'));
  assert.ok(!missing.html.includes('>Manage</button>'));
  const unknown = await harness(null, { overrides: { inventory: { ...initial().inventory, drivers: { ...initial().inventory.drivers,
    vjoy: { installed: null, version: null, error: 'Driver metadata could not be read.' } } } } });
  assert.ok(unknown.html.includes('Driver metadata could not be read.'));
  assert.deepEqual(unknown.setups, [], 'Failed inspection must not claim a driver is missing.');
  const unready = await harness(null, { overrides: { visibility: { ...initial().visibility, enabled: false,
    selfAccess: { ready: false, message: 'Restart Cockpit after installing HidHide.' } } } });
  assert.ok(unready.html.includes('Restart Cockpit after installing HidHide.'));
  assert.ok(unready.html.includes('Needs attention'));
});
