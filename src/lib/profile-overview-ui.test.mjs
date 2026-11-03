import {readModelFixture} from './profile-read-models/test-fixture.mjs';
import {discoveredGames} from './game-test-fixtures.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import {loadModules,libModuleName} from './test-module-loader.mjs';

const modules=await loadModules(['./persistence/ui-preferences','./ui-settings-guard','./system-inventory','./profiles','./profile-runtime','./profile-runtime-controller','./profile-automatic','./profile-automatic-controller','./games','./game-provider-settings','./game-settings-navigation','./cockpit-history','./runtime-config','./panel-input-metadata','./gateway-capabilities','./controller-lifecycle','./devices','./device-actions']);
modules.get('./games').updateGameCatalogue(discoveredGames);
modules.set('./profile-read-models/shared',readModelFixture());
const id='11111111-1111-4111-8111-111111111111',other='33333333-3333-4333-8333-333333333333',session='22222222-2222-4222-8222-222222222222',hash=`sha256:${'a'.repeat(64)}`;
const stopped=()=>({state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null});
const automatic=(mode='manual')=>({schemaVersion:1,mode,manualProfileId:modules.get('./profiles').DEFAULT_PROFILE_ID,rules:[],status:mode==='manual'?'manual':'waiting',message:'Native profile selection policy.',conflicts:[],lastAttempt:null,target:null,readOnly:false});
const running=()=>({...stopped(),state:'running',appliedSnapshot:{profileId:id,revision:3,semanticHash:hash},owner:{session,generation:4,profileId:id,profileRevision:3,gameId:'dcs'}});
const view=status=>({status,pending:null,loading:false,stale:!status,error:'',statusError:''});
const profiles=[{id,name:'Applied saved rig',revision:9,isDefault:false,semanticHash:hash},{id:other,name:'Unapplied editor selection',revision:7,isDefault:false,semanticHash:hash}];
const games=()=>({catalogue:[{...discoveredGames[0],installed:true,integrationContext:{status:'ready',message:'The native owner has the matching game context.',processVerified:true,contextSession:session,providerSetupRevision:hash,catalogueRevision:hash},integrationSetup:{state:'ready',message:'Provider configured.',live:'waiting'}}],detectedGames:[{gameId:'dcs',name:'DCS World'}],detectionErrors:[],cleanup:{phase:'idle'}});
const plain=value=>JSON.parse(JSON.stringify(value)),settle=()=>new Promise(resolve=>setImmediate(resolve));
const noRequest=()=>{throw new Error('Rendering must not request native effects.');};
function evaluate(code,filename,require,extra={}){const exports={};vm.runInNewContext(ts.transpileModule(code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,...extra,require},{filename});return exports.default;}
const notifications=[],cache=new Map();async function component(name){if(cache.has(name))return cache.get(name);const filename=new URL(name,import.meta.url),{js}=compile(await readFile(filename,'utf8'),{filename:filename.pathname,generate:'server'}),dependencies=new Map();for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g)){const child=await component(match[1]);dependencies.set(match[1],{default:match[1]==='./Notification.svelte'?(payload,props)=>{notifications.push(props.message);return child(payload,props);}:child});}const result=evaluate(js.code,filename.pathname,name=>{if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(dependencies.has(name))return dependencies.get(name);if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:noRequest};if(name==='./notifications')return{notify:noRequest};throw new Error(`Unexpected overview dependency ${name}`);});cache.set(name,result);return result;}
const Overview=await component('./OverviewPanel.svelte'),Manual=await component('./ManualActions.svelte');
const overview=(profileState,extra={})=>render(Overview,{props:{games:games(),profileState,profiles,gateways:1,onlineDevices:3,totalDevices:4,inventoryError:'',openGames:noRequest,openProfiles:noRequest,openDevices:noRequest,...extra}}).body;

test('overview distinguishes a successful empty Windows detection from missing status and actual process scan failures',()=>{
 const empty={...games(),detectedGames:[],detectionErrors:[]};
 empty.catalogue.push({...discoveredGames[3],installed:false,profileContext:{status:'not-installed'}});
 const html=overview(view(stopped()),{games:empty});
 assert.ok(html.includes('No game running'));assert.ok(!html.includes('Detection unavailable'));
 const failed={...empty,detectionErrors:['Cannot inspect game process: access denied']};notifications.length=0;
 assert.ok(overview(view(stopped()),{games:failed}).includes('Detection unavailable'));
 assert.ok(notifications.includes(failed.detectionErrors[0]));
 assert.ok(overview(view(stopped()),{games:{catalogue:[]}}).includes('Detection unavailable'));
 assert.ok(overview(view(stopped()),{games:undefined}).includes('Checking games…'));
});

test('actual overview reads the native applied revision and exact profile identity rather than selected, saved-latest or retired game profile state',()=>{
 const html=overview(view(running()));assert.ok(html.includes('Applied saved rig'));assert.ok(!html.includes('applied revision'));assert.ok(!html.includes('applied revision 9'));assert.ok(!html.includes('Unapplied editor selection'));assert.ok(html.includes('Open profiles'));assert.ok(html.includes('Native process detected'));assert.ok(!html.includes('PID'));
 assert.ok(html.includes('Matching game context verified'));assert.ok(html.includes('Context readiness does not confirm fresh telemetry'));assert.ok(!html.includes('Receiving telemetry'));
 const retired={...games(),session:{phase:'running',profileId:'legacy'},profiles:[{id:'legacy',name:'Retired independent profile'}]};const inactive=overview(view(stopped()),{games:retired});assert.ok(inactive.includes('No profile active'));assert.ok(!inactive.includes('Stopped'));assert.ok(!inactive.includes('Retired independent profile'));assert.ok(!inactive.includes('Activate a profile in Games'));
});

test('actual overview distinguishes hardware-only, awaiting-game, degradation and recovery without inventing telemetry or active ownership',()=>{
 let status=running();status.owner.gameId=null;assert.ok(overview(view(status)).includes('Hardware-only profile'));
 status=running();status.state='awaiting-game';let html=overview(view(status));assert.ok(html.includes('Hardware remains neutral'));assert.ok(html.includes('Waiting for the verified game'));
 status=running();status.state='degraded';status.degradation=['Exact optional companion is offline.'];html=overview(view(status));assert.ok(html.includes('Degraded · available branches active'));assert.ok(html.includes(status.degradation[0]));
 for(const state of ['faulted','recovery-needed','transitioning']){status={...stopped(),state,fault:state==='faulted'?'Native source continuity failed.':null};notifications.length=0;html=overview(view(status));assert.ok(!html.includes('No profile active'));if(status.fault)assert.ok(notifications.includes(status.fault));}
 html=overview({...view(running()),stale:true,statusError:'Current status unavailable.'});assert.ok(html.includes('Native status unavailable'));assert.ok(!html.includes('Applied saved rig'));assert.ok(!html.includes('Matching game context verified'));
 assert.ok(overview(view(null)).includes('Checking profile…'));
 const cleanup=games();cleanup.cleanup={phase:'recovery-needed',error:'A retained output receipt remains.'};html=overview(view(stopped()),{games:cleanup});assert.ok(!html.includes('Previous owned setup'));assert.ok(html.includes('No profile active'));assert.ok(!html.includes('Review cleanup'));
});

test('device manual actions use the real native ownership block, with no retired binding inference or imaginary output acknowledgement',()=>{
 const state={backend:'stm32',connected:true,reconnecting:false,layoutPending:false,moduleActions:{panel:{boot:1,catalogueRevision:2,status:'ready',actions:[{id:'led',label:'Indicator',endpoint:'indicator',semantics:'setter',readable:true,parameters:[]}],states:[]}}},module={id:'panel',online:true,boot:1};
 const html=render(Manual,{props:{state,module,busy:false,request:noRequest,profileBlockedReason:'Exact native profile ownership must be released.'}}).body;assert.ok(html.includes('Exact native profile ownership must be released.'));assert.ok(!html.includes('Stop DCS-BIOS'));assert.ok(!html.includes('temporary override'));assert.match(html,/<fieldset[^>]*disabled[^>]*>[^]*?Apply[^]*?<\/fieldset>/);assert.ok(html.includes('Applied state is unknown.'));
});

async function mountedApp(send){
 const models=readModelFixture();models.emit('library',{status:'connecting'});models.emit('runtime',{status:'connecting'});let oldRuntime,oldLibrary;
 const observations={...models,observeProfileRuntime(listener){oldRuntime=listener;return models.observeProfileRuntime(listener);},observeProfileLibrary(listener){oldLibrary=listener;return models.observeProfileLibrary(listener);}};
 const filename=new URL('../App.svelte',import.meta.url),source=(await readFile(filename,'utf8')).replace('</script>','captureApp({navigate,get runtime(){return profileRuntime;},get state(){return{profileState,profileSummaries,view};}});</script>'),{js}=compile(source,{filename:filename.pathname,generate:'server'}),mounts=[],timers=new Map(),listeners=new Map();let handlers,nextTimer=0;
 const document={hidden:false},location=new URL('http://localhost/'),window={location,addEventListener(name,fn){listeners.set(name,fn);},removeEventListener(name){listeners.delete(name);}},history={state:null,replaceState(state,_unused,url){this.state=state;if(url)location.href=new URL(url,location).href;},pushState(state,_unused,url){this.state=state;if(url)location.href=new URL(url,location).href;},go(){},back(){},forward(){}};window.history=history;
 const App=evaluate(js.code,filename.pathname,name=>{
  if(name==='svelte/internal/server')return server;if(name==='svelte')return{...svelte,onMount(fn){mounts.push(fn);}};if(name.endsWith('.svelte'))return{default:()=>{}};
  const key=name.replace('./lib/','./');if(key==='./profile-read-models/shared')return observations;if(key==='./cockpit-history'){const actual=modules.get(key);return{...actual,createCockpitHistory:(initial,options)=>actual.createCockpitHistory(initial,{...options,host:window})};}if(modules.has(key))return modules.get(key);
  if(name.endsWith('/gateway'))return{gatewayRequest:send,GatewayError:Error,releaseOnExit:noRequest,openCockpitLink:noRequest};if(name.endsWith('/gateway-workspace'))return{GatewayWorkspace:class{refresh(){}dispose(){}}};if(name.endsWith('/companion-startup'))return{restoreCompanionPanels:async()=>{}};if(name.endsWith('/notifications'))return{notify:noRequest};throw new Error('Unexpected App dependency '+name);
 },{document,window,history,setInterval(fn){timers.set(++nextTimer,fn);return nextTimer;},clearInterval(id){timers.delete(id);},captureApp(value){handlers=value;}});
 render(App).body;return{handlers,document,timers,models,mount(){return mounts.map(fn=>fn());},lateRuntime(state){oldRuntime?.(state);},lateLibrary(state){oldLibrary?.(state);}};
}
const savedRows=()=>[{id:modules.get('./profiles').DEFAULT_PROFILE_ID,name:'Default',revision:1,isDefault:true,semanticHash:hash},...profiles];
test('actual mounted App keeps passive profile streams fresh across views while hidden game polling remains suspended',async()=>{
 const calls=[],app=await mountedApp(async request=>{calls.push(plain(request));if(request.command==='game_get_state')return{games:games()};throw new Error('Unexpected App request '+request.command);});
 assert.deepEqual(calls,[]);const cleanup=app.mount();await settle();assert.deepEqual(calls.map(c=>c.command),['game_get_state']);assert.equal(app.handlers.state.profileState.stale,true);assert.equal(app.models.count('runtime'),1);assert.equal(app.models.count('library'),1);
 app.models.emit('runtime',{status:'ready',generation:'1',revision:'1',value:{profileSession:running(),automatic:automatic(),readOnly:false}});app.models.emit('library',{status:'ready',value:{profiles:savedRows()}});
 assert.equal(app.handlers.state.profileState.status.appliedSnapshot.profileId,id);assert.equal(app.handlers.state.profileSummaries.length,3);assert.ok(!Object.hasOwn(app.handlers.state,'profileMode'));
 calls.length=0;for(const fn of app.timers.values())fn();await settle();assert.deepEqual(calls.map(c=>c.command),['game_get_state']);
 calls.length=0;app.document.hidden=true;for(const fn of app.timers.values())fn();await settle();assert.deepEqual(calls,[]);
 app.models.emit('runtime',{status:'lost',error:'Native source continuity lost.'});assert.equal(app.handlers.state.profileState.stale,true);app.models.emit('runtime',{status:'ready',generation:'2',revision:'1',value:{profileSession:stopped(),automatic:automatic(),readOnly:false}});assert.equal(app.handlers.state.profileState.status.owner,null);assert.equal(app.handlers.state.profileState.stale,false);
 app.document.hidden=false;for(const page of ['profiles','devices','topology','preferences']){app.handlers.navigate(page);await settle();calls.length=0;for(const fn of app.timers.values())fn();await settle();assert.deepEqual(calls.map(c=>c.command),['game_get_state'],page);assert.equal(app.models.count('runtime'),1);assert.equal(app.models.count('library'),1);}
 app.handlers.navigate('overview');const before=plain(app.handlers.state);cleanup.forEach(fn=>fn?.());app.lateRuntime({status:'ready',generation:'1',revision:'999',value:{profileSession:running(),automatic:automatic(),readOnly:false}});app.lateLibrary({status:'ready',value:{profiles:[]}});assert.deepEqual(plain(app.handlers.state),before);assert.equal(app.timers.size,0);assert.equal(app.models.count('runtime'),0);assert.equal(app.models.count('library'),0);assert.ok(calls.every(c=>c.command==='game_get_state'));
});

test('mounted App runtime observations keep policy and library selection separate from native ownership and fence disposed callbacks',async()=>{
 const calls=[],app=await mountedApp(async request=>{calls.push(request.command);if(request.command==='game_get_state')return{games:games()};throw new Error('Unexpected App request '+request.command);});const cleanup=app.mount();await settle();
 app.models.emit('runtime',{status:'ready',value:{profileSession:stopped(),automatic:automatic('automatic'),readOnly:false}});assert.equal(app.handlers.state.profileState.stale,false);assert.equal(app.handlers.state.profileState.status.owner,null);assert.ok(!Object.hasOwn(app.handlers.state,'profileMode'));
 app.models.emit('library',{status:'ready',value:{profiles:savedRows(),session:{selectedProfileId:other,state:'running',selectionMode:'automatic'}}});assert.equal(app.handlers.state.profileState.status.owner,null,'Library selection and automatic policy cannot invent native ownership.');
 app.models.emit('runtime',{status:'error',error:'Native runtime status unavailable.'});assert.equal(app.handlers.state.profileState.stale,true);assert.match(app.handlers.state.profileState.statusError,/unavailable/);
 app.models.emit('runtime',{status:'ready',value:{profileSession:running(),automatic:automatic(),readOnly:true}});assert.equal(app.handlers.state.profileState.stale,false);assert.equal(app.handlers.state.profileState.readOnly,true);assert.equal(app.handlers.state.profileState.status.appliedSnapshot.profileId,id);
 assert.equal(await app.handlers.runtime.apply({id:other,revision:7,baseRevision:7,dirty:false,readOnly:false}),false);assert.ok(calls.every(command=>command==='game_get_state'),'Read-only observations cannot authorize Apply or any output request.');
 app.models.emit('runtime',{status:'ready',value:{profileSession:running(),automatic:automatic(),readOnly:false}});assert.equal(app.handlers.state.profileState.readOnly,false);
 const before=plain(app.handlers.state);cleanup.forEach(fn=>fn?.());app.lateRuntime({status:'ready',value:{profileSession:stopped(),automatic:automatic(),readOnly:false}});app.lateRuntime({status:'error',error:'Retired source failed.'});assert.deepEqual(plain(app.handlers.state),before,'Disposed callbacks cannot update runtime status.');assert.ok(calls.every(command=>command==='game_get_state'));
});

test('header badges show authoritative selection mode and applied ownership without a stopped label or stale inference',async()=>{
 const Header=await component('./HeaderStatus.svelte');
 const modeController=(mode='manual',extra={})=>({state:{status:automatic(mode),pending:null,loading:false,stale:false,error:'',...extra}});
 const header=(profileState,extra={})=>render(Header,{props:{games:games(),gamesStale:false,profileState,profiles,automaticController:modeController(),...extra}}).body;
 const active=header(view(running()));assert.ok(active.includes('Active game'));assert.ok(active.includes('DCS World'));assert.ok(active.includes('Active profile'));assert.ok(!active.includes('Running'));assert.ok(active.includes('Applied saved rig'));assert.ok(active.includes('Manual'));assert.ok(!active.includes('>Auto<'));assert.ok(!active.includes('Unapplied editor selection'));assert.ok(!active.includes('applied revision'));
 const modeStart=active.indexOf('profile-mode-status'),profileStart=active.indexOf('profile-status');assert.ok(modeStart>profileStart);assert.ok(active.slice(modeStart).includes('Automatic profile selection'));assert.ok(active.slice(modeStart).includes('Manual'));assert.ok(!active.slice(profileStart,modeStart).includes('Manual'));assert.ok(!active.includes('mode-badge'));assert.match(active, /role="group" aria-label="Active profile and mode"/);assert.match(active.slice(profileStart,modeStart), /class="badge-separator[^>]*aria-hidden="true"/);assert.equal((active.match(/class="header-status\b/g)??[]).length,2);
 const idle=header(view(stopped()),{games:{catalogue:[],detectedGames:[]},automaticController:modeController('automatic')});assert.ok(idle.includes('Active profile'));assert.ok(!idle.includes('Stopped'));assert.ok(idle.includes('Auto'));assert.ok(!idle.includes('Applied saved rig'));assert.ok(!idle.includes('DCS World'));assert.ok(idle.includes('None'));
 const stale=header({...view(running()),stale:true},{gamesStale:true});assert.ok(stale.includes('Unavailable'));assert.ok(!stale.includes('Applied saved rig'));assert.ok(!stale.includes('DCS World'));
 const degraded=header(view({...running(),state:'degraded'}));assert.ok(degraded.includes('Active profile · Degraded'));assert.ok(degraded.includes('Applied saved rig'));
 const missing=header(view(stopped()),{games:{catalogue:[],detectionErrors:['Process scan unavailable']}});assert.ok(missing.includes('Unavailable'));
 const pending=header(view(stopped()),{automaticController:modeController('manual',{status:null,stale:true})});assert.ok(pending.includes('Mode unavailable'));assert.ok(!pending.includes('Manual'));assert.ok(!pending.includes('>Auto<'));
 const staleMode=header(view(running()),{automaticController:modeController('automatic',{stale:true,error:'Native policy read failed.'})});assert.ok(staleMode.includes('Mode unavailable'));assert.ok(staleMode.includes('Native policy read failed.'));assert.ok(!staleMode.includes('>Auto<'));assert.ok(staleMode.includes('Applied saved rig'));
 for(const state of ['faulted','recovery-needed','transitioning']){const html=header(view({...stopped(),state}));assert.ok(!html.includes('Stopped'));assert.ok(html.includes({faulted:'Faulted','recovery-needed':'Recovery needed',transitioning:'Activating…'}[state]));}
});
