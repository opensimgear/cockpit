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
const modules = await loadModules(['./profiles','./profile-runtime','./profile-automatic','./profile-automatic-controller','./runtime-config','./panel-input-metadata','./integration-profile','./games','./game-provider-settings','./game-settings-navigation']);
const noRequest = () => { throw new Error('Rendering game settings must not send a request.'); };
const componentModules = new Map();
async function component(name) {
  if (componentModules.has(name)) return componentModules.get(name);
  const filename = new URL(name, import.meta.url);
  const { js } = compile(await readFile(filename,'utf8'), {filename:filename.pathname,generate:'server'});
  const dependencies = new Map();
  for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g)) dependencies.set(match[1],{default:await component(match[1])});
  const exports = {};
  const code = ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  vm.runInNewContext(code,{exports,TextEncoder,require(name){
    if(name==='svelte/internal/server')return server;
    if(name==='svelte')return svelte;
    if(dependencies.has(name))return dependencies.get(name);
    if(modules.has(name))return modules.get(name);
    if(name==='./gateway')return {gatewayRequest:noRequest,pickGameExecutable:noRequest};
    if(name==='./mappings')return {savedAssignments:noRequest};
    if(name==='./notifications')return {notify:noRequest};
    throw new Error(`Unexpected component dependency ${name}`);
  }},{filename:filename.pathname});
  componentModules.set(name,exports.default);return exports.default;
}
const GamesPanel = await component('./GamesPanel.svelte');
const IntegrationsPanel = await component('./IntegrationsPanel.svelte');
const { supportedGames } = modules.get('./games');
const { defaultProviderConnection } = modules.get('./game-provider-settings');
const game = id => ({...supportedGames.find(game=>game.id===id),integrationSetup:{state:'setup-required',message:'Configure this provider',providerSetupRevision:`sha256:${'1'.repeat(64)}`,userFolder:'C:\\Users\\Pilot\\Saved Games\\DCS',connection:{...defaultProviderConnection(id),...(id==='dcs'?{cataloguePath:'C:\\DCS-BIOS\\doc\\json'}:{})}},integrationContext:{status:'waiting-profile',message:'An active matching profile is required',processVerified:false,contextSession:null,providerSetupRevision:null,catalogueRevision:null}});
const state = g => ({catalogue:[g],profiles:[],defaultProfiles:{},session:{phase:'idle'}});

test('DCS game details own connection, catalogue, aircraft, installer recovery and separate readiness',()=>{
  const g=game('dcs');g.integrationSetup.recoveryNeeded=true;
  const html=render(GamesPanel,{props:{status:state(g),selectedGameId:'dcs',busy:false,targets:[],request:noRequest,readConfig:noRequest,returnLabel:'Return to profile',returnToEditor:noRequest}}).body;
  for(const label of ['Automatic profile rule','Current installation executable','Saved unified profile','DCS-BIOS connection and catalogue','DCS Saved Games folder','DCS-BIOS doc/json directory','Aircraft module','Command UDP port','Provider setup','Game context','Telemetry freshness','Waiting for an active profile','No verified matching process','Recover integration setup','Save game settings','Return to profile'])assert.ok(html.includes(label),label);
  for(const obsolete of ['Start DCS-BIOS','Use saved integration','Update from saved integration','Save and open integrations','Synthetic test telemetry'])assert.ok(!html.includes(obsolete),obsolete);
  assert.ok(html.includes('C:\\DCS-BIOS\\doc\\json'));
});
test('telemetry-only game forms expose no simulator command or synthetic/process-identity editor',()=>{
  for(const id of ['assetto-corsa-rally','x-plane-11','x-plane-12']){
    const g=game(id);const html=render(GamesPanel,{props:{status:state(g),selectedGameId:id,busy:false,targets:[],request:noRequest,readConfig:noRequest}}).body;
    assert.ok(html.includes('Save game settings'));assert.ok(html.includes('telemetry only'));
    assert.ok(!html.includes('Command UDP port'));assert.ok(!html.includes('Aircraft module'));assert.ok(!html.includes('Synthetic test telemetry'));
    if(id.startsWith('x-plane')){assert.ok(html.includes('Simulator UDP port'));assert.ok(html.includes(`X-Plane ${id.slice(-2)}`));}
  }
});
test('provider resource index includes closed games and offers only owning-game settings links',()=>{
  const html=render(IntegrationsPanel,{props:{setups:supportedGames.map(g=>game(g.id)),openGameSettings:noRequest}}).body;
  for(const g of supportedGames)assert.ok(html.includes(`href="#games/${g.id}/integration"`));
  assert.ok(html.includes('Resources remain available to author while a game is closed'));
  assert.ok(!html.includes('<input'));assert.ok(!html.includes('<select'));assert.ok(!html.includes('Start integration'));
});
test('App removes standalone DCS entry points and retains only a previously mounted editor with polling suspended',async()=>{
  const source=await readFile(new URL('../App.svelte',import.meta.url),'utf8');
  assert.ok(!source.includes('DcsBiosPanel'));assert.ok(!source.includes("view === 'dcs-bios'"));assert.ok(!source.includes("command: 'dcs_start'"));assert.ok(!source.includes("command: 'integration_start'"));
  assert.match(source,/view === 'mappings' \|\| settingsVisit\?\.origin.view === 'mappings'/);
  assert.match(source,/<MappingsPanel active=\{view === 'mappings'\}/);
  const mapping=await readFile(new URL('./MappingsPanel.svelte',import.meta.url),'utf8');assert.match(mapping,/if \(!active \|\| disposed \|\| polling \|\| busy\) return/);
});
