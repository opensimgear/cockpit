import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import * as designer from '../../packages/companion-designer/src/index.ts';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profiles','./profile-runtime','./profile-runtime-controller','./games','./game-provider-settings','./game-settings-navigation','./runtime-config','./panel-input-metadata','./gateway-capabilities','./controller-lifecycle','./devices','./device-actions']);
const id='11111111-1111-4111-8111-111111111111',other='33333333-3333-4333-8333-333333333333',session='22222222-2222-4222-8222-222222222222',hash=`sha256:${'a'.repeat(64)}`;
const stopped=()=>({state:'stopped',paused:false,appliedSnapshot:null,owner:null,degradation:[],fault:null});
const running=()=>({...stopped(),state:'running',appliedSnapshot:{profileId:id,revision:3,semanticHash:hash},owner:{session,generation:4,profileId:id,profileRevision:3,gameId:'dcs'}});
const view=status=>({status,pending:null,loading:false,stale:!status,error:'',statusError:''});
const profiles=[{id,name:'Applied saved rig',revision:9,isDefault:false,semanticHash:hash},{id:other,name:'Unapplied editor selection',revision:7,isDefault:false,semanticHash:hash}];
const games=()=>({catalogue:[{...modules.get('./games').supportedGames[0],installed:true,integrationContext:{status:'ready',message:'The native owner has the matching game context.',processVerified:true,contextSession:session,providerSetupRevision:hash,catalogueRevision:hash},integrationSetup:{state:'ready',message:'Provider configured.',live:'waiting'}}],detectedGames:[{gameId:'dcs',name:'DCS World'}],detectionErrors:[],cleanup:{phase:'idle'}});
const plain=value=>JSON.parse(JSON.stringify(value)),settle=()=>new Promise(resolve=>setImmediate(resolve));
const noRequest=()=>{throw new Error('Rendering must not request native effects.');};
function evaluate(code,filename,require,extra={}){const exports={};vm.runInNewContext(ts.transpileModule(code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,...extra,require},{filename});return exports.default;}
const notifications=[],cache=new Map();async function component(name){if(cache.has(name))return cache.get(name);const filename=new URL(name,import.meta.url),{js}=compile(await readFile(filename,'utf8'),{filename:filename.pathname,generate:'server'}),dependencies=new Map();for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g)){const child=await component(match[1]);dependencies.set(match[1],{default:match[1]==='./Notification.svelte'?(payload,props)=>{notifications.push(props.message);return child(payload,props);}:child});}const result=evaluate(js.code,filename.pathname,name=>{if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(dependencies.has(name))return dependencies.get(name);if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:noRequest};throw new Error(`Unexpected overview dependency ${name}`);});cache.set(name,result);return result;}
const Overview=await component('./OverviewPanel.svelte'),Manual=await component('./ManualActions.svelte');
const overview=(profileState,extra={})=>render(Overview,{props:{games:games(),profileState,profiles,gateways:1,onlineDevices:3,totalDevices:4,inventoryError:'',openGames:noRequest,openProfiles:noRequest,openIntegrations:noRequest,openDevices:noRequest,...extra}}).body;

test('actual overview reads the native applied revision and exact profile identity rather than selected, saved-latest or retired game profile state',()=>{
 const html=overview(view(running()));assert.ok(html.includes('Applied saved rig'));assert.ok(html.includes('applied revision 3'));assert.ok(!html.includes('applied revision 9'));assert.ok(!html.includes('Unapplied editor selection'));assert.ok(html.includes('Open profiles'));assert.ok(html.includes('Native process detected'));assert.ok(!html.includes('PID'));
 assert.ok(html.includes('Matching game context verified'));assert.ok(html.includes('Context readiness does not confirm fresh telemetry'));assert.ok(!html.includes('Receiving telemetry'));
 const retired={...games(),session:{phase:'running',profileId:'legacy'},profiles:[{id:'legacy',name:'Retired independent profile'}]};const inactive=overview(view(stopped()),{games:retired});assert.ok(inactive.includes('No active profile'));assert.ok(!inactive.includes('Retired independent profile'));assert.ok(!inactive.includes('Activate a profile in Games'));
});

test('actual overview distinguishes hardware-only, awaiting-game, degradation, pause and recovery without inventing telemetry or active ownership',()=>{
 let status=running();status.owner.gameId=null;assert.ok(overview(view(status)).includes('Hardware-only profile'));
 status=running();status.state='awaiting-game';let html=overview(view(status));assert.ok(html.includes('Hardware remains neutral'));assert.ok(html.includes('Waiting for the verified game'));
 status=running();status.state='degraded';status.degradation=['Exact optional companion is offline.'];html=overview(view(status));assert.ok(html.includes('Running with unavailable branches'));assert.ok(html.includes(status.degradation[0]));
 for(const state of ['paused','faulted','recovery-needed','transitioning']){status={...stopped(),state,paused:state==='paused',fault:state==='faulted'?'Native source continuity failed.':null};notifications.length=0;html=overview(view(status));assert.ok(!html.includes('No active profile'));if(status.fault)assert.ok(notifications.includes(status.fault));}
 html=overview({...view(running()),stale:true,statusError:'Current status unavailable.'});assert.ok(html.includes('Native status unavailable'));assert.ok(!html.includes('Applied saved rig'));assert.ok(!html.includes('Matching game context verified'));
 assert.ok(overview(view(null)).includes('Checking profile…'));
 const cleanup=games();cleanup.cleanup={phase:'recovery-needed',error:'A retained output receipt remains.'};html=overview(view(stopped()),{games:cleanup});assert.ok(html.includes('Previous owned setup'));assert.ok(html.includes('No active profile'));assert.ok(html.includes('Review cleanup'));
});

test('device manual actions use the real native ownership block, with no retired binding inference or imaginary output acknowledgement',()=>{
 const state={backend:'stm32',connected:true,reconnecting:false,layoutPending:false,moduleActions:{panel:{boot:1,catalogueRevision:2,status:'ready',actions:[{id:'led',label:'Indicator',endpoint:'indicator',semantics:'setter',readable:true,parameters:[]}],states:[]}}},module={id:'panel',online:true,boot:1};
 const html=render(Manual,{props:{state,module,busy:false,request:noRequest,profileBlockedReason:'Exact native profile ownership must be released.'}}).body;assert.ok(html.includes('Exact native profile ownership must be released.'));assert.ok(!html.includes('Stop DCS-BIOS'));assert.ok(!html.includes('temporary override'));assert.match(html,/<fieldset[^>]*disabled[^>]*>[^]*?Apply[^]*?<\/fieldset>/);assert.ok(html.includes('Applied state is unknown.'));
});

async function mountedApp(send){
 const filename=new URL('../App.svelte',import.meta.url),source=(await readFile(filename,'utf8')).replace('</script>','captureApp({navigate,refreshProfileStatus,refreshProfileNames,get state(){return{profileState,profileSummaries,view};}});</script>'),{js}=compile(source,{filename:filename.pathname,generate:'server'}),mounts=[],timers=new Map(),listeners=new Map();let handlers,nextTimer=0;
 const document={hidden:false},location=new URL('http://localhost/'),window={location,addEventListener(name,fn){listeners.set(name,fn);},removeEventListener(name){listeners.delete(name);}},history={replaceState(){},pushState(){}};
 const App=evaluate(js.code,filename.pathname,name=>{
  if(name==='svelte/internal/server')return server;if(name==='svelte')return{...svelte,onMount(fn){mounts.push(fn);}};if(name.endsWith('.svelte'))return{default:()=>{}};
  const key=name.replace('./lib/','./');if(modules.has(key))return modules.get(key);
  if(name.endsWith('/gateway'))return{gatewayRequest:send,GatewayError:Error,releaseOnExit:noRequest,openCockpitLink:noRequest};if(name.endsWith('/gateway-workspace'))return{GatewayWorkspace:class{refresh(){}dispose(){}}};if(name.endsWith('/companion-startup'))return{restoreCompanionPanels:async()=>{}};if(name.endsWith('/notifications'))return{notify:noRequest};throw new Error(`Unexpected App dependency ${name}`);
 },{document,window,history,setInterval(fn){timers.set(++nextTimer,fn);return nextTimer;},clearInterval(id){timers.delete(id);},captureApp(value){handlers=value;}});
 render(App).body;return{handlers,document,timers,mount(){return mounts.map(fn=>fn());}};
}
const savedRows=()=>[{id:modules.get('./profiles').DEFAULT_PROFILE_ID,name:'Default',revision:1,isDefault:true,semanticHash:hash},...profiles];
test('actual mounted App replaces standalone simulator polling with bounded passive native status and stops polling inactive or hidden views without output controls',async()=>{
 const calls=[];let hold;const app=await mountedApp(async request=>{calls.push(plain(request));if(request.command==='profile_status'){if(hold)await hold;return{profiles:{profileSession:running()}};}if(request.command==='profile_list')return{profiles:{profiles:savedRows()}};if(request.command==='game_get_state')return{games:games()};throw new Error(`Unexpected App request ${request.command}`);});
 assert.deepEqual(calls,[]);const cleanup=app.mount();await settle();assert.deepEqual(calls.map(c=>c.command).sort(),['game_get_state','profile_list','profile_status']);assert.equal(app.handlers.state.profileState.status.appliedSnapshot.profileId,id);assert.equal(app.handlers.state.profileSummaries.length,3);
 calls.length=0;for(const fn of app.timers.values())fn();await settle();assert.deepEqual(calls.map(c=>c.command).sort(),['game_get_state','profile_status']);
 calls.length=0;app.document.hidden=true;for(const fn of app.timers.values())fn();await settle();assert.deepEqual(calls,[]);app.document.hidden=false;app.handlers.navigate('profiles');for(const fn of app.timers.values())fn();await settle();assert.deepEqual(calls,[]);
 app.handlers.navigate('overview');let release;hold=new Promise(yes=>release=yes);const pending=app.handlers.refreshProfileStatus();await settle();const before=app.handlers.state.profileState;cleanup.forEach(fn=>fn?.());release();await pending;assert.equal(app.handlers.state.profileState,before);assert.equal(app.timers.size,0);assert.ok(calls.every(c=>['profile_status','profile_list','game_get_state'].includes(c.command)));
});

async function companionCompactor(send){
 const filename=new URL('./CompanionDesigner.svelte',import.meta.url),source=(await readFile(filename,'utf8')).replace('</script>','captureCompactor({compactSnapshots,setStore(next){store=next;}});</script>'),{js}=compile(source,{filename:filename.pathname,generate:'server'});let handlers;
 const Designer=evaluate(js.code,filename.pathname,name=>{
  if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(name.endsWith('.svelte'))return{default:()=>{}};if(modules.has(name))return modules.get(name);
  if(name==='@opensimgear/companion-designer')return designer;if(name==='@opensimgear/companion-ui')return{PanelCanvas:()=>{}};
  if(name==='./gateway')return{gatewayRequest:send};if(name==='./companions')return{sendPublication:noRequest};if(name==='./panel-actions')return{synchronizePanel:noRequest,removePanelAssignments:noRequest};if(name==='./companion-startup')return{restoreCompanionPanels:noRequest};if(name==='./firmware-widgets')return{firmwareWidgetPresets:[],simplifyFirmwareWidgets:document=>({document,removed:0})};throw new Error(`Unexpected compactor dependency ${name}`);
 },{crypto:{randomUUID:()=>id},captureCompactor(value){handlers=value;}});render(Designer,{props:{openSettings:noRequest}}).body;return handlers;
}
test('actual panel compaction preserves everything when unified publication references are missing, corrupt or unavailable; valid native references pin retained revisions',async()=>{
 const a=hash,b=`sha256:${'b'.repeat(64)}`,c=`sha256:${'c'.repeat(64)}`,calls=[],compactions=[];let native;
 const handler=await companionCompactor(async request=>{calls.push(plain(request));if(request.command==='profile_companion_references')return{profiles:{publicationReferences:native}};if(request.command==='companion_prune')return{companions:{}};throw new Error(`Unexpected compaction request ${request.command}`);});handler.setStore({async compact(pins){compactions.push([...pins]);return{library:{schemaVersion:1,drafts:[]},publications:{}};}});
 for(const value of [undefined,{schemaVersion:1,libraryFingerprint:a,hashes:[b,a]},{schemaVersion:1,libraryFingerprint:a,hashes:[a],permission:true}]){native=value;await handler.compactSnapshots({host:{instances:[{publicationHash:c}]}});assert.equal(compactions.length,0);assert.ok(calls.every(c=>c.command==='profile_companion_references'));}
 calls.length=0;native={schemaVersion:1,libraryFingerprint:a,hashes:[a,b]};await handler.compactSnapshots({host:{instances:[{publicationHash:c}]}});assert.deepEqual(new Set(compactions[0]),new Set([a,b,c]));assert.deepEqual(calls.map(c=>c.command),['profile_companion_references','companion_prune']);assert.deepEqual(new Set(calls[1].companion.hashes),new Set([a,b,c]));
 calls.length=0;await handler.compactSnapshots({host:{instances:[]}},true);assert.equal(compactions.length,1);assert.deepEqual(calls.map(c=>c.command),['profile_companion_references','companion_prune']);
});
