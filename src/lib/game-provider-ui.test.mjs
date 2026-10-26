import {discoveredGames} from './game-test-fixtures.mjs';
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
const modules = await loadModules(['./game-discovery','./ui-settings-guard','./game-profile-context','./profiles','./profile-runtime','./profile-runtime-controller','./profile-automatic','./profile-automatic-controller','./runtime-config','./panel-input-metadata','./games','./game-artwork','./game-background-crop','./game-provider-settings','./game-settings-navigation']);
modules.get('./games').updateGameCatalogue(discoveredGames);
const noRequest = () => { throw new Error('Rendering game settings must not send a request.'); };
const componentModules = new Map();
async function component(name, hook) {
  if (!hook && componentModules.has(name)) return componentModules.get(name);
  const filename = new URL(name, import.meta.url);
  const source = await readFile(filename,'utf8');
  const { js } = compile(hook ? hook.source(source) : source, {filename:filename.pathname,generate:'server'});
  const dependencies = new Map();
  for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g)) dependencies.set(match[1],{default:await component(match[1])});
  const exports = {};
  const code = ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  vm.runInNewContext(code,{exports,TextEncoder,captureGameRequests:hook?.receive,require(name){
    if(name==='svelte/internal/server')return server;
    if(name==='svelte')return svelte;
    if(dependencies.has(name))return dependencies.get(name);
    if(modules.has(name))return modules.get(name);
    if(name==='./gateway')return {gatewayRequest:noRequest,pickGameExecutable:noRequest,pickGameIntegrationFolder:noRequest};
    if(name==='./notifications')return {notify:noRequest};
    throw new Error(`Unexpected component dependency ${name}`);
  }},{filename:filename.pathname});
  if (!hook) componentModules.set(name,exports.default);return exports.default;
}
const GamesPanel = await component('./GamesPanel.svelte');
const supportedGames = discoveredGames;
const { defaultProviderConnection } = modules.get('./game-provider-settings');
const game = id => ({...supportedGames.find(game=>game.id===id),integrationSetup:{state:'setup-required',message:'Configure this provider',providerSetupRevision:`sha256:${'1'.repeat(64)}`,userFolder:'C:\\Users\\Pilot\\Saved Games\\DCS',connection:{...defaultProviderConnection(id),...(id==='dcs'?{cataloguePath:'C:\\DCS-BIOS\\doc\\json'}:{})}},integrationContext:{status:'waiting-profile',message:'An active matching profile is required',processVerified:false,contextSession:null,providerSetupRevision:null,catalogueRevision:null}});
const state = g => ({catalogue:[g],cleanup:{phase:'idle'}});

test('all games share presentation and only assigned categories and integration capabilities add badges',()=>{
  const titles=[{id:'dcs',name:'Renamed flight title',provider:null,installed:true,launchSupported:false},{id:'assetto-corsa',name:'Renamed driving title',provider:'assetto-corsa',installed:true,launchSupported:false},{id:'game-any-title',name:'Another title',provider:null,installed:true,launchSupported:false},{id:'game-assigned',name:'Assigned title',provider:null,categoryId:'mine',installed:true,launchSupported:false}];
  const status={catalogue:titles,categories:{mine:'My category'}};
  const html=render(GamesPanel,{props:{status,busy:false,request:noRequest}}).body;
  const badges=[...html.matchAll(/<span[^>]*class="art-category[^>]*>([^]*?)<\/span>/g)].map(match=>match[1]);assert.deepEqual(badges,['My category']);
  assert.ok(!html.includes('/game-art/'));assert.equal((html.match(/class="game-art-placeholder/g)??[]).length,titles.length);
  for(const title of titles){
    const detail=render(GamesPanel,{props:{status,selectedGameId:title.id,busy:false,request:noRequest}}).body;
    assert.ok(detail.includes('aria-label="Game name"'));assert.ok(detail.includes('aria-label="Game executable"'));assert.ok(detail.includes('placeholder="Choose an executable…"'));
    assert.ok(!detail.includes('Flight simulation'));assert.ok(!detail.includes('Racing simulation'));
    assert.equal(detail.includes('class="header-eyebrow'),!!title.categoryId);
  }
});

test('game metadata dialogs receive the original actionable native failure while ordinary panel requests retain inline errors',async()=>{
  let handlers;const failure=new Error('Deactivate this game context before changing its executable.');
  const Panel=await component('./GamesPanel.svelte',{source:value=>value.replace('</script>','captureGameRequests({send});</script>'),receive:value=>handlers=value});
  render(Panel,{props:{status:state(game('dcs')),busy:false,request:async()=>{throw failure;}}}).body;
  for(const command of ['game_save','game_background_save','game_discovery_start','game_discovery_apply'])await assert.rejects(()=>handlers.send({command}),error=>error===failure);
  assert.equal(await handlers.send({command:'game_get_state'}),false);
});

test('custom Installation edits preserve the registration identity and name and surface path failures inline',async()=>{
  let handlers,reject=false;const calls=[],id='custom-11111111-1111-4111-8111-111111111111',g={id,name:'Relocated custom game',provider:null,custom:true,installed:false,executable:null,manualPath:'C:\\Old\\Game.exe',launchSupported:false};
  const Panel=await component('./GamesPanel.svelte',{source:value=>value.replace('</script>','captureGameRequests({saveExecutable,get notice(){return notice;}});</script>'),receive:value=>handlers=value});
  render(Panel,{props:{status:state(g),selectedGameId:id,busy:false,request:async value=>{calls.push(JSON.parse(JSON.stringify(value)));if(reject)throw new Error('Choose an existing absolute game executable (.exe).');return state({...g,executable:value.executable,installed:true});}}}).body;
  await handlers.saveExecutable(g,'C:\\New\\Game.exe');assert.deepEqual(calls[0],{command:'game_save',gameId:id,executable:'C:\\New\\Game.exe'});
  reject=true;await handlers.saveExecutable(g,'C:\\Missing\\Game.exe');assert.match(handlers.notice,/Choose an existing absolute game executable/);
  reject=false;await handlers.saveExecutable(supportedGames[0],'C:\\Games\\DCS.exe');assert.deepEqual(calls[2],{command:'game_save',gameId:'dcs',executable:'C:\\Games\\DCS.exe'});
});

test('successful custom and builtin installation saves adopt the exact canonical native path',async()=>{
  let handlers;const prefix=String.fromCharCode(92,92,63,92),path='C:\\Games\\Moved.exe';
  const Panel=await component('./GamesPanel.svelte',{source:value=>value.replace('</script>','captureGameRequests({saveExecutable,get executablePath(){return executablePath;}});</script>'),receive:value=>handlers=value});
  for(const g of [{id:'custom-11111111-1111-4111-8111-111111111111',name:'Custom game',provider:null,custom:true,installed:false,launchSupported:false},game('dcs')]){
    render(Panel,{props:{status:state(g),selectedGameId:g.id,busy:false,request:async()=>state({...g,executable:prefix+path,manualPath:prefix+path,installed:true})}}).body;
    await handlers.saveExecutable(g,path);assert.equal(handlers.executablePath,prefix+path);
  }
});

test('custom games expose matching profiles, metadata and background editing without telemetry setup or vehicle controls',()=>{
  const id='custom-11111111-1111-4111-8111-111111111111',g={id,name:'Custom game',provider:null,custom:true,installed:false,launchSupported:false};
  const html=render(GamesPanel,{props:{status:state(g),selectedGameId:id,busy:false,request:noRequest}}).body;
  for(const label of ['Game name','Remove','Matching profiles','Tile background','Update image','Bindings and profile activation','Game executable'])assert.ok(html.includes(label),label);
  assert.ok(!html.includes('Game executable path'));assert.ok(!/>Edit<\/button>/.test(html));assert.equal((html.match(/aria-label="Game executable"/g)??[]).length,1);
  for(const absent of ['Integration setup','Install integration','Native telemetry not verified','Current aircraft','Current car'])assert.ok(!html.includes(absent),absent);
  const list=render(GamesPanel,{props:{status:state(g),busy:false,request:noRequest}}).body;
  for(const label of ['Rescan','Custom game','Not found'])assert.ok(list.includes(label),label);
});

test('executable fields show installation-relative paths and save relative edits as full paths',async()=>{
  let handlers;const calls=[],id='custom-11111111-1111-4111-8111-111111111111';
  const g={id,name:'Fixture game',provider:null,installed:true,launchSupported:false,installPath:'C:/Old',executable:'C:/Games/Fixture/bin/Game.exe',selectedInstallationId:'copy',installations:[{id:'copy',installPath:'C:/Games/Fixture',source:'disk',available:true,executableCandidates:[]}]};
  const Panel=await component('./GamesPanel.svelte',{source:value=>value.replace('</script>','captureGameRequests({initializeGame,commitExecutable,setExecutable(value){executablePath=value;}});</script>'),receive:value=>handlers=value});
  const html=render(Panel,{props:{status:state(g),selectedGameId:id,busy:false,request:async command=>{calls.push(command);return state({...g,executable:command.executable});}}}).body;
  const input=html.match(/<input[^>]*aria-label="Game executable"[^>]*>/)?.[0];assert.ok(input);assert.ok(input.includes('value="bin\\Game.exe"'));
  handlers.initializeGame(g);handlers.setExecutable('bin/Alternate.exe');await handlers.commitExecutable(g);
  assert.deepEqual(plain(calls),[{command:'game_save',gameId:id,installationId:'copy',executable:'C:\\Games\\Fixture\\bin\\Alternate.exe'}]);
});

test('inline custom names save only the name and reject invalid names or builtin edits',async()=>{
  let handlers;const calls=[],id='custom-11111111-1111-4111-8111-111111111111',g={id,name:'Old name',custom:true,installed:false,manualPath:'C:\\Missing\\Game.exe',launchSupported:false};
  const Panel=await component('./GamesPanel.svelte',{source:value=>value.replace('</script>','captureGameRequests({initializeGame,saveName,setName(value){nameDraft=value;},get notice(){return notice;}});</script>'),receive:value=>handlers=value});
  render(Panel,{props:{status:state(g),selectedGameId:id,busy:false,request:async value=>{calls.push(JSON.parse(JSON.stringify(value)));return state({...g,name:value.name});}}}).body;
  handlers.initializeGame(g);handlers.setName('New name');await handlers.saveName(g);assert.deepEqual(calls,[{command:'game_save',gameId:id,name:'New name'}]);
  handlers.setName(' ');await handlers.saveName(g);assert.equal(calls.length,1);assert.match(handlers.notice,/game name/);
  handlers.setName('Discovered rename');await handlers.saveName(supportedGames[0]);assert.equal(calls.length,2);
  const builtin=render(GamesPanel,{props:{status:state(game('dcs')),selectedGameId:'dcs',busy:false,request:noRequest}}).body;assert.ok(builtin.includes('aria-label="Game name"'));
});

test('missing custom executables retain their saved path in details and relocation dialogs',async()=>{
  const id='custom-11111111-1111-4111-8111-111111111111',manualPath='C:\\OldGames\\MissingGame.exe',g={id,name:'Moved game',provider:null,custom:true,installed:false,executable:null,manualPath,launchSupported:false};
  const details=render(GamesPanel,{props:{status:state(g),selectedGameId:id,busy:false,request:noRequest}}).body;
  assert.ok(details.includes(manualPath));assert.ok(details.includes('The saved executable could not be found. Choose its new location.'));
});

test('DCS game details group native locations and autosaved network settings without aircraft or manual save controls',()=>{
  const g=game('dcs');g.integrationSetup.recoveryNeeded=true;g.integrationSetup.activeAircraftModule='F-86F Sabre';
  const html=render(GamesPanel,{props:{status:state(g),selectedGameId:'dcs',busy:false,request:noRequest,returnLabel:'Return to profile',returnToEditor:noRequest}}).body;
  for(const label of ['Matching profiles','Matching profiles will appear here.','DCS-BIOS locations','Network and timing','DCS Saved Games folder','DCS-BIOS doc/json directory','Command UDP port','Game context','Telemetry freshness','Waiting for an active profile','No verified matching process','Recover integration setup','Return to profile'])assert.ok(html.includes(label),label);
  for(const obsolete of ['Aircraft module','Save game settings','Discard changes','Start DCS-BIOS','Use saved integration','Update from saved integration','Save and open integrations','Synthetic test telemetry'])assert.ok(!html.includes(obsolete),obsolete);
  assert.ok(html.includes('C:\\DCS-BIOS\\doc\\json'));
  assert.ok(html.includes('DCS-BIOS follows the loaded aircraft automatically'));assert.ok(html.includes('F-86F Sabre'));assert.ok(html.includes('Base uses shared flight telemetry'));
});
test('telemetry-only game forms expose no simulator command or synthetic/process-identity editor',()=>{
  for(const {id} of supportedGames.filter(game=>game.id!=='dcs')){
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

const discoveryFixture=()=>({scanId:'scan-1',status:'ready',firstStart:true,libraries:[{id:'steam',name:'Steam',status:'complete',found:2},{id:'local',name:'Local paths',status:'complete',found:1}],entries:[{id:'add',name:'Found game',action:'add',library:'Steam',installPath:'C:/Games/New',executableCandidates:['C:/Games/New/game.exe']},{id:'update',name:'Moved game',action:'update',library:'Local paths',installPath:'C:/Games/Moved',previousPath:'C:/Games/Old',executableCandidates:[]},{id:'remove',name:'Missing game',action:'remove',library:'Steam',installPath:'C:/Games/Missing',executableCandidates:[]}],diagnostics:[]});
test('discovery detail renders library sections, two review lists and opt-in removals',async()=>{
 const View=await component('./GameDiscoveryView.svelte');
 const html=render(View,{props:{gameState:{catalogue:[],discovery:discoveryFixture()},busy:false,request:noRequest,close:noRequest}}).body;
 for(const label of ['Local paths','Steam','New and changed installations','Removed installations','Add location','Apply'])assert.ok(html.includes(label),label);
 assert.ok(!html.includes('Previous:'));assert.ok(!html.includes('C:\\Games\\Old'));
 assert.ok(!html.includes('<code'),'Installation roots are not repeated in review rows');
 const checkboxes=html.match(/<input\b[^>]*type="checkbox"[^>]*>/g)??[];
 const entryCheckboxes=checkboxes.filter(input=>/aria-label="(?:Add|Update|Remove) /.test(input));
 assert.equal(entryCheckboxes.length,3);
 assert.equal(entryCheckboxes.filter(input=>/\bchecked\b/.test(input)).length,2);
 const libraryCheckboxes=checkboxes.filter(input=>input.includes('Select all games in '));
 assert.equal(libraryCheckboxes.length,2);
 assert.ok(!/\bchecked\b/.test(libraryCheckboxes.find(input=>input.includes('Steam'))));
 assert.ok(/\bchecked\b/.test(libraryCheckboxes.find(input=>input.includes('Local paths'))));
 assert.ok(!html.includes('Import from library'));
});
test('first discovery shows progress on the list instead of an empty-catalogue message',()=>{
 const discovery={...discoveryFixture(),status:'scanning',entries:[]};discovery.libraries[1].status='scanning';
 const html=render(GamesPanel,{props:{status:{catalogue:[],discovery},busy:false,request:noRequest}}).body;
 assert.ok(html.includes('role="status"'));assert.ok(html.includes('Discovering your games'));assert.ok(html.includes('View results'));
 assert.ok(!html.includes('No games found'));
});

test('discovery keeps game names and executable choices without manual match controls',async()=>{
 const View=await component('./GameDiscoveryView.svelte'),discovery=discoveryFixture();
 discovery.entries=[{id:'steam',gameId:'steam-flight',name:'Flight Simulator',action:'add',library:'Steam',installPath:'C:/Steam/Flight',executableCandidates:[]},{id:'disk',gameId:'disk-flight',name:'FlightSim',action:'add',library:'Local paths',installPath:'D:/Flight',executableCandidates:[]}];
 const html=render(View,{props:{gameState:{catalogue:[],discovery},busy:false,request:noRequest,close:noRequest}}).body;
 assert.ok(!html.includes('Match and installation'));
 assert.ok(!html.includes('aria-label="FlightSim game"'));
 assert.ok(html.includes('aria-label="FlightSim executable"'));
 assert.ok(html.includes('Flight Simulator'));

});

test('discovery groups compact rows inside library accordions with action checkboxes on the right',async()=>{
 const View=await component('./GameDiscoveryView.svelte');const html=render(View,{props:{gameState:{catalogue:[],discovery:discoveryFixture()},busy:false,request:noRequest,close:noRequest}}).body;
 const steamStart=html.indexOf('data-library="steam"'),localStart=html.indexOf('data-library="local"');
 assert.ok(steamStart>=0&&localStart>steamStart);
 const steam=html.slice(steamStart,localStart),local=html.slice(localStart);
 assert.ok(steam.includes('Found game')&&steam.includes('Missing game'));assert.ok(!steam.includes('Moved game'));
 assert.ok(local.includes('Moved game'));assert.ok(steam.includes('data-platform="steam"'));
 for(const action of ['Add Found game','Update Moved game','Remove Missing game'])assert.ok(html.includes('aria-label="'+action+'"'));
 const entry=steam.slice(steam.indexOf('data-entry-id="add"'),steam.indexOf('data-entry-id="remove"'));
 assert.ok(entry.indexOf('entry-art')>=0,'Artwork or its placeholder forms the first column');
 assert.ok(entry.indexOf('entry-art')<entry.indexOf('entry-details'),'Artwork precedes the name and executable column');
 assert.ok(entry.indexOf('entry-details')<entry.indexOf('entry-choice'),'Name and executable precede the right-aligned action control');
 assert.ok(entry.includes('aria-label="Found game executable"'));assert.ok(!entry.includes('type="text"'));
 const title=entry.match(/<strong[^>]*>([^]*?)<\/strong>/)?.[1];assert.equal(title,'Found game');assert.ok(!entry.slice(0,entry.indexOf('<select')).includes('game.exe'),'Executable path is not repeated above the select');
 assert.equal((entry.match(/>game\.exe<\/option>/g)??[]).length,1,'Executable is relative to the detected installation');assert.ok(entry.includes('value="C:/Games/New/game.exe"'),'Option values retain the full path');
});
test('every supported library uses a distinct monochrome platform glyph',async()=>{
 const Icon=await component('./Icon.svelte'),glyphs=new Set();
 for(const name of ['steam','epic','battlenet','ea','ubisoft','gog','xbox','disk']){
  const html=render(Icon,{props:{name}}).body;assert.ok(html.includes(name==='disk'?'stroke="currentColor"':'fill="currentColor"'));glyphs.add(html);
 }
 assert.equal(glyphs.size,8);
});
