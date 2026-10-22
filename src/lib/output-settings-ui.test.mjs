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

const modules = await loadModules(['./ui-settings-guard','./profile-desktop-runtime']);
const plain = value => JSON.parse(JSON.stringify(value));
const initial = () => ({
  desktop: { enabled: false, qualified: true, state: 'stopped', reason: '', scope: { kind: 'application' } },
  visibility: { enabled: false, qualified: true, settingsError: null, selfAccess: { message: 'Capture access configured.' } },
  inventory: { virtualDevices: [{ id: 'vjoy:1', name: 'vJoy Device 1', status: 'free' }], diagnostics: [] },
});
async function harness(send) {
  const file = new URL('./OutputSettings.svelte', import.meta.url);
  const source = (await readFile(file, 'utf8')).replace('</script>', 'desktop=seed.desktop;visibility=seed.visibility;inventory=seed.inventory;capture({refresh,enableVisibility,enableDesktop,manageDriver,get state(){return {desktop,visibility,inventory,busy,error};}});</script>');
  const { js } = compile(source, { filename: file.pathname, generate: 'server' });
  const exports = {}, requests = [], invocations = [], seed = initial();
  let handlers,settingsVersion=1;
  const native=async request=>{
    if(send)return send(request,seed);
    if(request.command.startsWith('profile_local_settings_')){
      if(request.command==='profile_local_settings_save'){seed.desktop.enabled=request.settings.enabled;seed.desktop.scope=request.settings.scope;settingsVersion++;}
      return{localSettings:{scope:'desktop',gameId:null,revision:`sha256:${String(settingsVersion).padStart(64,'0')}`,settings:{enabled:seed.desktop.enabled,scope:seed.desktop.scope},profileSession:{owner:null}}};
    }
    return{systemInventory:seed.inventory,desktop:{...seed.desktop},controllers:{visibility:{...seed.visibility,...(request.command==='controller_visibility_enable'?{enabled:request.enabled}:{})}}};
  };
  vm.runInNewContext(ts.transpileModule(js.code, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
    exports, seed, TextEncoder, capture(value) { handlers = value; },
    require(name) {
      if (name === 'svelte/internal/server') return server;
      if (name === 'svelte') return svelte;
      if (name.endsWith('.svelte')) return { default: () => {} };
      if (name === '@tauri-apps/api/core') return { isTauri: () => true, invoke: async command => { invocations.push(command); } };
      if (modules.has(name)) return modules.get(name);
      if (name === './gateway') return { gatewayRequest: async request => { requests.push(plain(request));return native(request); } };
      throw Error(name);
    },
  });
  const html = render(exports.default).body;
  return { html, handlers, requests, invocations };
}

test('Settings owns driver tools and machine enablement while application scope belongs to profile blocks', async () => {
  const h = await harness();
  for (const label of ['Configure vJoy', 'Manage HidHide', 'Enable device hiding on this computer', 'Enable desktop output on this computer', 'vJoy Device 1: free']) assert.ok(h.html.includes(label), label);
  assert.ok(h.html.includes('Choose the application scope in each profile'));
  assert.ok(!h.html.includes('<select'));
  assert.deepEqual(h.requests, []);
  await h.handlers.refresh();
  assert.deepEqual(h.requests.map(request=>request.command),['system_inventory','controller_visibility_status','profile_local_settings_get','profile_desktop_status']);h.requests.length=0;
  await h.handlers.enableVisibility(true);
  await h.handlers.enableDesktop(true);
  assert.deepEqual(h.requests, [
    { command: 'controller_visibility_enable', enabled: true },
    { command: 'profile_local_settings_save',scope:'desktop',expectedSettingsRevision:`sha256:${'1'.padStart(64,'0')}`,expectedActiveOwner:null,settings:{enabled:true,scope:{kind:'application'}} },
    {command:'profile_desktop_status'},
  ]);
  await h.handlers.manageDriver('open_vjoy');
  await h.handlers.manageDriver('open_hidhide');
  assert.deepEqual(h.invocations, ['open_vjoy', 'open_hidhide']);
});

test('failed setup refresh and enablement report native errors without claiming a saved change', async () => {
  const h = await harness(async () => { throw new Error('Stop the running profile before changing setup.'); });
  await h.handlers.enableVisibility(true);
  assert.equal(h.handlers.state.visibility.enabled, false);
  assert.match(h.handlers.state.error, /Stop the running profile/);
  await h.handlers.enableDesktop(true);
  assert.equal(h.handlers.state.desktop.enabled, false);
  await h.handlers.refresh();
  assert.match(h.handlers.state.error, /Stop the running profile/);
  assert.equal(h.handlers.state.busy, '');
  assert.deepEqual(h.requests.slice(1).map(request => request.command), ['system_inventory', 'controller_visibility_status', 'profile_local_settings_get']);
});
