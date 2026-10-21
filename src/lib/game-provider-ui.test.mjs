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
const modules = await loadModules(['./game-profile-context','./profiles','./profile-runtime','./profile-runtime-controller','./profile-automatic','./profile-automatic-controller','./runtime-config','./panel-input-metadata','./games','./game-provider-settings','./game-settings-navigation']);
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
    if(name==='./gateway')return {gatewayRequest:noRequest,pickGameExecutable:noRequest,pickGameIntegrationFolder:noRequest};
    if(name==='./notifications')return {notify:noRequest};
    throw new Error(`Unexpected component dependency ${name}`);
  }},{filename:filename.pathname});
  componentModules.set(name,exports.default);return exports.default;
}
const GamesPanel = await component('./GamesPanel.svelte');
const { supportedGames } = modules.get('./games');
const { defaultProviderConnection } = modules.get('./game-provider-settings');
const game = id => ({...supportedGames.find(game=>game.id===id),integrationSetup:{state:'setup-required',message:'Configure this provider',providerSetupRevision:`sha256:${'1'.repeat(64)}`,userFolder:'C:\\Users\\Pilot\\Saved Games\\DCS',connection:{...defaultProviderConnection(id),...(id==='dcs'?{cataloguePath:'C:\\DCS-BIOS\\doc\\json'}:{})}},integrationContext:{status:'waiting-profile',message:'An active matching profile is required',processVerified:false,contextSession:null,providerSetupRevision:null,catalogueRevision:null}});
const state = g => ({catalogue:[g],cleanup:{phase:'idle'}});

test('DCS game details group native locations and autosaved network settings without aircraft or manual save controls',()=>{
  const g=game('dcs');g.integrationSetup.recoveryNeeded=true;g.integrationSetup.activeAircraftModule='F-86F Sabre';
  const html=render(GamesPanel,{props:{status:state(g),selectedGameId:'dcs',busy:false,request:noRequest,returnLabel:'Return to profile',returnToEditor:noRequest}}).body;
  for(const label of ['Matching profiles','Matching profiles will appear here.','DCS-BIOS locations','Network and timing','DCS Saved Games folder','DCS-BIOS doc/json directory','Command UDP port','Game context','Telemetry freshness','Waiting for an active profile','No verified matching process','Recover integration setup','Return to profile'])assert.ok(html.includes(label),label);
  for(const obsolete of ['Aircraft module','Save game settings','Discard changes','Start DCS-BIOS','Use saved integration','Update from saved integration','Save and open integrations','Synthetic test telemetry'])assert.ok(!html.includes(obsolete),obsolete);
  assert.ok(html.includes('C:\\DCS-BIOS\\doc\\json'));
  assert.ok(html.includes('DCS-BIOS follows the loaded aircraft automatically'));assert.ok(html.includes('F-86F Sabre'));assert.ok(html.includes('Base uses shared flight telemetry'));
});
test('telemetry-only game forms expose no simulator command or synthetic/process-identity editor',()=>{
  for(const id of ['assetto-corsa-rally','x-plane-11','x-plane-12']){
    const g=game(id);const html=render(GamesPanel,{props:{status:state(g),selectedGameId:id,busy:false,request:noRequest}}).body;
    assert.ok(html.includes('Network and timing'));assert.ok(!html.includes('Save game settings'));
    assert.ok(!html.includes('Command UDP port'));assert.ok(!html.includes('Aircraft module'));assert.ok(!html.includes('Synthetic test telemetry'));
    if(id.startsWith('x-plane')){assert.ok(html.includes('Simulator UDP port'));assert.ok(html.includes(`X-Plane ${id.slice(-2)}`));}
  }
});
test('App exposes one profile authoring path and retains only a previously opened Profiles editor during game settings',async()=>{
  const source=await readFile(new URL('../App.svelte',import.meta.url),'utf8');
  for(const retired of ['DcsBiosPanel','MappingsPanel',"id: 'mappings'","command: 'dcs_start'","command: 'integration_start'"])assert.ok(!source.includes(retired),retired);
  assert.match(source,/view === 'profiles' \|\| settingsVisit\?\.origin.view === 'profiles'/);
  assert.match(source,/<ProfilesPanel[^\n]* active=\{view\s*===\s*'profiles'\}/);
});

const plain=value=>JSON.parse(JSON.stringify(value)),rev=n=>`sha256:${String(n).repeat(64)}`;
const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};};
const settle=()=>new Promise(resolve=>setImmediate(resolve));
async function providerView(initial=game('dcs'),options={}){
  let handlers;const destroyers=[],calls=[],notifications=[],busyChanges=[],picks=[];
  const file=new URL('./GameProviderSettings.svelte',import.meta.url),source=(await readFile(file,'utf8')).replace('</script>',`capture({commit,flush,syncSettings,browseFolder,edit(fn){fn(draft);},setGame(value){game=value;syncSettings();},setBusy(value){busy=value;},get fields(){return {draft,base,saving,picking,error:attempted?providerSettingsError(draft):''};}});</script>`),{js}=compile(source,{filename:file.pathname,generate:'server'}),exports={};
  vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,Error,TextEncoder,structuredClone,capture(value){handlers=value;},require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return {...svelte,onDestroy(callback){destroyers.push(callback);}};if(name==='./Icon.svelte')return {default:()=>{}};if(name==='./notifications')return {notify(message,kind){notifications.push({message,kind});}};if(name==='./gateway')return {async pickGameIntegrationFolder(...args){picks.push(args);return options.pick?.(...args)??null;}};if(modules.has(name))return modules.get(name);throw Error(name);}});
  let revision=1;
  const props={game:initial,busy:false,onBusyChange(value){busyChanges.push(value);},async request(request){calls.push(plain(request));if(options.request)return options.request(request);const next=plain(initial);next.integrationSetup={...next.integrationSetup,providerSetupRevision:rev(++revision),connection:request.connection,...(request.userFolder?{userFolder:request.userFolder}:{}),...(request.aircraftModule!==undefined?{aircraftModule:request.aircraftModule}:{})};handlers.setGame(next);return {catalogue:[next]};}};
  const html=render(exports.default,{props}).body;
  return {html,handlers,calls,notifications,busyChanges,picks,destroy(){destroyers.forEach(callback=>callback());}};
}

test('provider autosave stays quiet on mount, unchanged commit and remote polling',async()=>{
  const view=await providerView();assert.deepEqual(view.calls,[]);assert.deepEqual(view.notifications,[]);view.handlers.commit();await settle();assert.deepEqual(view.calls,[]);
  const remote=game('dcs');remote.integrationSetup.providerSetupRevision=rev(2);remote.integrationSetup.connection.listenPort=5011;view.handlers.setGame(remote);await settle();assert.equal(view.handlers.fields.draft.connection.listenPort,5011);assert.deepEqual(view.calls,[]);assert.deepEqual(view.notifications,[]);
});

test('committed provider fields save once without writing the automatically selected aircraft module',async()=>{
  const initial=game('dcs');initial.integrationSetup.aircraftModule='A-10C';const view=await providerView(initial);view.handlers.edit(draft=>{draft.connection.commandPort=7779;});assert.deepEqual(view.calls,[]);view.handlers.commit();await settle();assert.equal(view.calls.length,1);assert.equal(view.calls[0].expectedSetupRevision,rev(1));assert.equal(view.calls[0].connection.commandPort,7779);assert.equal(view.calls[0].aircraftModule,undefined);assert.equal(view.handlers.fields.base.baseRevision,rev(2));view.handlers.commit();await settle();assert.equal(view.calls.length,1);assert.deepEqual(view.notifications,[]);assert.equal(view.busyChanges.at(-1),false);
});

test('invalid committed values stay inline and actual failures notify once while retaining edits',async()=>{
  const view=await providerView(game('x-plane-12'),{request(){throw Error('Provider transport failed.');}});view.handlers.edit(draft=>{draft.connection.port=0;});view.handlers.commit();await settle();assert.equal(view.calls.length,0);assert.match(view.handlers.fields.error,/UDP ports/);assert.deepEqual(view.notifications,[]);
  view.handlers.edit(draft=>{draft.connection.port=49001;});view.handlers.commit();await settle();assert.equal(view.calls.length,1);assert.equal(view.handlers.fields.draft.connection.port,49001);assert.equal(view.handlers.fields.saving,false);assert.deepEqual(view.notifications,[{message:'Provider transport failed.',kind:'error'}]);
});

test('queued commits serialize and coalesce while newer uncommitted edits remain local',async()=>{
  const pending=[deferred(),deferred()];let index=0;const view=await providerView(game('x-plane-12'),{request(){return pending[index++].promise;}});
  view.handlers.edit(draft=>{draft.connection.port=49001;});view.handlers.commit();view.handlers.edit(draft=>{draft.connection.port=49002;});view.handlers.commit();view.handlers.edit(draft=>{draft.connection.port=49003;});view.handlers.commit();view.handlers.edit(draft=>{draft.connection.port=49004;});assert.equal(view.calls.length,1);
  const returned=game('x-plane-12');returned.integrationSetup.providerSetupRevision=rev(2);returned.integrationSetup.connection.port=49001;view.handlers.setGame(returned);pending[0].resolve({catalogue:[returned]});await settle();assert.equal(view.calls.length,2);assert.equal(view.calls[1].connection.port,49003);assert.equal(view.calls[1].expectedSetupRevision,rev(2));
  const final=plain(returned);final.integrationSetup.providerSetupRevision=rev(3);final.integrationSetup.connection.port=49003;view.handlers.setGame(final);pending[1].resolve({catalogue:[final]});await settle();assert.equal(view.handlers.fields.draft.connection.port,49004);assert.equal(view.handlers.fields.draft.baseRevision,rev(3));assert.equal(view.handlers.fields.base.connection.port,49003);assert.equal(view.calls.length,2);assert.deepEqual(view.notifications,[]);
});

test('remote revisions preserve local edits and update untouched values before the next commit',async()=>{
  const view=await providerView(game('x-plane-12'));view.handlers.edit(draft=>{draft.connection.port=49001;});const remote=game('x-plane-12');remote.integrationSetup.providerSetupRevision=rev(2);remote.integrationSetup.connection.listenPort=49100;view.handlers.setGame(remote);assert.equal(view.handlers.fields.draft.connection.port,49001);assert.equal(view.handlers.fields.draft.connection.listenPort,49100);assert.equal(view.calls.length,0);view.handlers.commit();await settle();assert.equal(view.calls[0].expectedSetupRevision,rev(2));assert.equal(view.calls[0].connection.listenPort,49100);
});

test('committing the original value during an in-flight save persists the intended revert',async()=>{
  const pending=[deferred(),deferred()];let index=0;const view=await providerView(game('x-plane-12'),{request(){return pending[index++].promise;}});
  view.handlers.edit(draft=>{draft.connection.port=49001;});view.handlers.commit();view.handlers.edit(draft=>{draft.connection.port=49000;});view.handlers.commit();
  const returned=game('x-plane-12');returned.integrationSetup.providerSetupRevision=rev(2);returned.integrationSetup.connection.port=49001;view.handlers.setGame(returned);pending[0].resolve({catalogue:[returned]});await settle();assert.equal(view.calls.length,2);assert.equal(view.calls[1].connection.port,49000);assert.equal(view.calls[1].expectedSetupRevision,rev(2));
  const final=plain(returned);final.integrationSetup.providerSetupRevision=rev(3);final.integrationSetup.connection.port=49000;view.handlers.setGame(final);pending[1].resolve({catalogue:[final]});await settle();assert.equal(view.handlers.fields.base.connection.port,49000);assert.deepEqual(view.notifications,[]);
});

test('closed or replaced provider editors ignore late saves and picker responses',async()=>{
  for(const close of ['dispose','replace'])for(const fail of [false,true]){const pending=deferred(),view=await providerView(game('x-plane-12'),{request(){return pending.promise;}});view.handlers.edit(draft=>{draft.connection.port=49001;});view.handlers.commit();if(close==='dispose')view.destroy();else view.handlers.setGame(game('assetto-corsa-rally'));if(fail)pending.reject(Error('Old request failed.'));else pending.resolve({catalogue:[game('x-plane-12')]});await settle();assert.deepEqual(view.notifications,[]);if(close==='replace')assert.equal(view.handlers.fields.draft.gameId,'assetto-corsa-rally');assert.equal(view.busyChanges.at(-1),false);}
  const pending=deferred(),view=await providerView(game('dcs'),{pick(){return pending.promise;}}),pick=view.handlers.browseFolder('catalogue');view.destroy();pending.resolve('C:\\Different\\doc\\json');await pick;assert.deepEqual(view.calls,[]);assert.deepEqual(view.notifications,[]);assert.equal(view.busyChanges.at(-1),false);
});

test('native folder choices autosave and cancelled pickers leave settings untouched',async()=>{
  const view=await providerView(game('dcs'),{pick(kind){return kind==='catalogue'?'C:\\NewDCS-BIOS\\doc\\json':null;}});await view.handlers.browseFolder('saved-games');assert.equal(view.calls.length,0);await view.handlers.browseFolder('catalogue');await settle();assert.equal(view.calls.length,1);assert.equal(view.calls[0].connection.cataloguePath,'C:\\NewDCS-BIOS\\doc\\json');assert.deepEqual(view.picks.map(args=>args[0]),['saved-games','catalogue']);assert.equal(view.busyChanges.at(-1),false);assert.deepEqual(view.notifications,[]);
});
