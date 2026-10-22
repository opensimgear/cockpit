import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./ui-settings-guard','./game-profile-context','./profiles','./profile-runtime','./profile-runtime-controller','./profile-automatic','./profile-automatic-controller','./runtime-config','./panel-input-metadata','./games','./game-artwork','./game-telemetry','./game-provider-settings','./game-settings-navigation','./cockpit-history','./gateway-capabilities','./controller-lifecycle','./devices','./system-inventory',]);
const noRequest=()=>{throw new Error('Opening Games must not acquire hardware, read configurations or start a runtime.');};
const components=new Map();
function evaluate(code,filename,require,extra={}){
  const exports={};
  vm.runInNewContext(ts.transpileModule(code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,...extra,require},{filename});
  return exports.default;
}
async function component(name){
  if(components.has(name))return components.get(name);
  const filename=new URL(name,import.meta.url),{js}=compile(await readFile(filename,'utf8'),{filename:filename.pathname,generate:'server'}),dependencies=new Map();
  for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(match[1],{default:await component(match[1])});
  const result=evaluate(js.code,filename.pathname,name=>{
    if(name==='svelte/internal/server')return server;
    if(name==='svelte')return svelte;
    if(dependencies.has(name))return dependencies.get(name);
    if(modules.has(name))return modules.get(name);
    if(name==='./gateway')return{gatewayRequest:noRequest,pickGameExecutable:noRequest,pickGameIntegrationFolder:noRequest};
    if(name==='./notifications')return{notify:noRequest};
    throw new Error(`Unexpected Games dependency ${name}`);
  });
  components.set(name,result);return result;
}
async function appView(view='games',selectedGameId=''){
  const filename=new URL('../App.svelte',import.meta.url),original=await readFile(filename,'utf8');
  const source=original.replace("let view = $state<View>('overview');",`let view = $state<View>(${JSON.stringify(view)});`).replace('let games = $state<GamesState>();','let games = $state<GamesState>(fixtureGameState);').replace("let selectedGameId = $state<GameId | ''>('');","let selectedGameId = $state<GameId | ''>(fixtureSelectedGame);");
  assert.notEqual(source,original);
  const {js}=compile(source,{filename:filename.pathname,generate:'server'}),GamesPanel=await component('./GamesPanel.svelte'),Icon=await component('./Icon.svelte');let props;const debugPanels=[];
  const game={...modules.get('./games').supportedGames.find(game=>game.id==='dcs'),installed:true,launchSupported:true,executable:'C:\\DCS\\bin\\DCS.exe',integrationSetup:{state:'setup-required',message:'Fixture provider has not been set up.',providerSetupRevision:`sha256:${'1'.repeat(64)}`,connection:modules.get('./game-provider-settings').defaultProviderConnection('dcs')},integrationContext:{status:'waiting-profile',message:'No verified matching game context.',processVerified:false,contextSession:null,providerSetupRevision:null,catalogueRevision:null}};
  const App=evaluate(js.code,filename.pathname,name=>{
    if(name==='svelte/internal/server')return server;
    if(name==='svelte')return svelte;
    if(name.endsWith('/Icon.svelte'))return{default:Icon};
    if(name.endsWith('/GamesPanel.svelte'))return{default:(payload,current)=>{props=current;return GamesPanel(payload,current);}};
    if(name.endsWith('/DebugPanel.svelte'))return{default:(_payload,current)=>{debugPanels.push(current);}};
    if(name.endsWith('.svelte'))return{default:()=>{}};
    const key=name.replace('./lib/','./');if(modules.has(key))return modules.get(key);
    if(name.endsWith('/gateway'))return{gatewayRequest:noRequest,GatewayError:Error,releaseOnExit:noRequest,openCockpitLink:noRequest};
    if(name.endsWith('/gateway-workspace'))return{GatewayWorkspace:class{constructor(){noRequest();}}};
    if(name.endsWith('/companion-startup'))return{restoreCompanionPanels:noRequest};
    if(name.endsWith('/notifications'))return{notify:noRequest};
    throw new Error(`Unexpected App dependency ${name}`);
  },{fixtureGameState:{catalogue:[game],cleanup:{phase:'idle'}},fixtureSelectedGame:selectedGameId});
  return{body:render(App).body,props,source:original,debugPanels};
}
const appGames=selectedGameId=>appView('games',selectedGameId);

test('actual App gives Debug its own sidebar section and mounts it independently of Profiles',async()=>{
  const debug=await appView('debug'),body=debug.body.replace(/<!--[\s\S]*?-->/g,'');
  const navigation=body.match(/<nav\b[^>]*aria-label="Main navigation"[^>]*>([^]*?)<\/nav>/)?.[1];assert.ok(navigation);
  const entry=navigation.match(/<button\b([^>]*title="Debug"[^>]*)>([^]*?)<\/button>/);assert.ok(entry,'Debug is a top-level navigation entry.');assert.match(entry[1],/\bclass="[^"]*\bcurrent\b/);assert.equal(entry[2].replace(/<[^>]*>/g,'').trim(),'Debug');
  assert.equal(debug.debugPanels.length,1);assert.ok(debug.debugPanels[0].runtime,'Debug shares the App runtime controller.');assert.equal(typeof debug.debugPanels[0].runtimeState,'object');assert.equal(debug.props,undefined,'Debug does not render Games.');
  assert.ok(!body.includes('Profile canvas'));assert.ok(!/<h1[^>]*>Profiles<\/h1>/.test(body));
  for(const view of ['overview','games','profiles'])assert.equal((await appView(view)).debugPanels.length,0,`Debug is not mounted from ${view}.`);
});

test('actual App Games tiles expose unified launch without injecting hardware targets or configuration readers',async()=>{
  const{body,props,source}=await appGames();
  assert.match(body,/aria-label="Games"/);assert.match(body,/DCS World/);assert.match(body,/aria-label="Launch DCS World"/);assert.doesNotMatch(body,/Unified profile selection|Deactivate profile|Waiting for game data/);
  assert.doesNotMatch(body,/>Integrations</);
  assert.equal('targets' in props,false);assert.equal('readConfig' in props,false);assert.equal(typeof props.request,'function');
  assert.ok(!source.includes('readGameConfig'));
  const invocation=source.split('\n').find(line=>line.includes('<GamesPanel '));assert.ok(invocation);assert.ok(!/\b(?:targets|readConfig)=/.test(invocation));
  assert.ok(!source.includes("{ id: 'mappings', label: 'Mappings'"));assert.ok(source.includes('<SystemDevices '));assert.ok(!source.includes('<ProfileRetiredRecovery'));
  const debug = await readFile(new URL('./ProfileDebugView.svelte', import.meta.url), 'utf8');assert.ok(debug.includes('<ProfileRetiredRecovery'));
  const devices=await readFile(new URL('./SystemDevices.svelte',import.meta.url),'utf8');
  assert.match(devices,/<HostControllers\s+initialSourceId=\{t\.id\}\s+detailsOnly/);
});

test('actual App game details retain provider setup and native rule selection without the retired per-game hardware editor',async()=>{
  const{body,props}=await appGames('dcs');
  for(const label of ['Matching profiles','Matching profiles will appear here.','DCS-BIOS locations','Network and timing','aria-label="Launch DCS World"'])assert.ok(body.includes(label),label);
  for(const label of ['Capture current mappings','Capture current companion','Add hardware profile','Use default profile','Accept binding changes','Deactivate profile'])assert.ok(!body.includes(label),label);
  assert.equal(props.selectedGameId,'dcs');assert.equal('targets' in props,false);assert.equal('readConfig' in props,false);
});
