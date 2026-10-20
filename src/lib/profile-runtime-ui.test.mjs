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
const modules=await loadModules(['./profiles','./profile-runtime','./profile-runtime-controller']);
const {ProfileRuntimeClient}=modules.get('./profile-runtime'),{ProfileRuntimeController}=modules.get('./profile-runtime-controller');
const id='11111111-1111-4111-8111-111111111111',other='33333333-3333-4333-8333-333333333333',session='22222222-2222-4222-8222-222222222222',hash=`sha256:${'1'.repeat(64)}`;
const stopped=()=>({state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null});
const running=()=>({...stopped(),state:'running',appliedSnapshot:{profileId:id,revision:3,semanticHash:hash},owner:{session,generation:4,profileId:id,profileRevision:3,gameId:'dcs'}});
const editor=()=>({id,revision:3,baseRevision:3,dirty:false,readOnly:false});
const profiles=[{id,name:'DCS rig',revision:3,isDefault:false,semanticHash:hash}];
const plain=value=>JSON.parse(JSON.stringify(value));
const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};};
function harness(){const calls=[];let response=stopped();const client=new ProfileRuntimeClient(async request=>{calls.push(plain(request));if(request.command==='profile_status')return response;if(request.command==='profile_apply')return running();return stopped();});return{calls,client,controller:new ProfileRuntimeController(client),setStatus(value){response=value;}};}

// Expose the actual Svelte handlers and poll lifecycle; SSR has no DOM or mounted effects.
async function component({gameContext=''}={}){
  const filename=new URL('./ProfileRuntimeControls.svelte',import.meta.url);
  const source=(await readFile(filename,'utf8')).replace("gameId = $state('');","gameId = $state(initialGameContext);").replace('</script>',`captureRuntimeControls({apply,stop,recover,watchStatus,mountStatus(){mounted=true;return watchStatus();},setActive(value){active=value;},get view(){return view;}});</script>`);
  const {js}=compile(source,{filename:filename.pathname,generate:'server'}),exports={};let handlers;
  const timers=new Map(),events=new Map(),document={hidden:false,addEventListener(name,listener){events.set(name,listener);},removeEventListener(name,listener){if(events.get(name)===listener)events.delete(name);}};
  const window={setInterval(callback){timers.set(1,callback);return 1;},clearInterval(id){timers.delete(id);}};
  vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,window,document,initialGameContext:gameContext,captureRuntimeControls(value){handlers=value;},require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(modules.has(name))return modules.get(name);throw new Error(`Unexpected dependency ${name}`);}},{filename:filename.pathname});
  return{render(props){const html=render(exports.default,{props}).body;return{html,handlers};},timers,events,document};
}
const button=(html,label)=>html.match(new RegExp(`<button\\b[^>]*>\\s*${label}[^]*?<\\/button>`))?.[0]??'';

test('runtime SSR/navigation is read-only and unsaved, new, read-only or stale editor state cannot start a profile',async()=>{
  const view=await component(),h=harness();let result=view.render({controller:h.controller,profiles});assert.deepEqual(h.calls,[]);assert.ok(result.html.includes('Checking status'));assert.ok(!result.html.includes('Start profile'));
  await h.controller.refresh();h.calls.length=0;
  for(const patch of [{dirty:true},{readOnly:true},{baseRevision:null},{revision:4}]){result=view.render({controller:h.controller,profiles,editor:{...editor(),...patch}});assert.match(button(result.html,'Start profile'),/disabled/);await result.handlers.apply();}
  assert.deepEqual(h.calls,[]);assert.ok(result.html.includes('Save your changes before starting this profile'));
});
test('starting a saved profile sends its exact saved state and explicit game while the UI shows the active profile',async()=>{
  const view=await component({gameContext:'dcs'}),h=harness();await h.controller.refresh();h.calls.length=0;
  let result=view.render({controller:h.controller,profiles,editor:editor(),gameIds:['dcs']});assert.ok(!button(result.html,'Start profile').includes('disabled'));assert.ok(result.html.includes('DCS World'));assert.ok(!result.html.includes('X-Plane 12'));
  await result.handlers.apply();assert.deepEqual(h.calls,[{command:'profile_apply',id,expectedRevision:3,gameId:'dcs'}]);
  result=view.render({controller:h.controller,profiles,editor:{...editor(),revision:4,baseRevision:4,dirty:true}});assert.ok(result.html.includes('Active profile \u00b7 DCS rig'));assert.ok(!result.html.includes('revision'));assert.ok(!result.html.includes('Generation'));assert.ok(!result.html.includes(session));assert.ok(!result.html.includes('Start profile'));assert.ok(result.html.includes('Saving this profile applies its changes'));assert.match(button(result.html,'Retry saved changes'),/disabled/);
});
test('a game choice retained from another profile never silently falls back to hardware-only Apply',async()=>{
  const view=await component({gameContext:'x-plane-12'}),h=harness();await h.controller.refresh();h.calls.length=0;const result=view.render({controller:h.controller,profiles,editor:editor(),gameIds:['dcs']});assert.match(button(result.html,'Start profile'),/disabled/);assert.ok(result.html.includes('Choose a game context eligible for this profile'));await result.handlers.apply();assert.deepEqual(h.calls,[]);
});
test('Stop stays enabled during slow Apply and its actual handler prevents late Apply acknowledgement from reviving the run',async()=>{
  const view=await component(),h=harness();await h.controller.refresh();const pending=deferred();h.client.apply=async()=>{h.calls.push({command:'profile_apply'});return pending.promise;};const applying=h.controller.apply(editor());
  let result=view.render({controller:h.controller,profiles,editor:editor()});assert.ok(result.html.includes('Stop can interrupt'));assert.ok(!button(result.html,'Stop').includes('disabled'));assert.equal(button(result.html,'Pause'),'');
  await result.handlers.stop();pending.resolve(running());assert.equal(await applying,false);result=view.render({controller:h.controller,profiles,editor:editor()});assert.ok(result.html.includes('Active profile \u00b7 None'));assert.ok(result.html.includes('aria-label="Stop current profile"'));assert.ok(!result.html.includes(session));assert.equal(h.calls.filter(call=>call.command==='profile_stop').length,1);assert.equal(h.calls.filter(call=>call.command==='profile_apply').length,1);
});
test('recovery reports native results without retrying failed Apply',async()=>{
  const view=await component(),h=harness();let result;h.setStatus({...stopped(),state:'recovery-needed',fault:'Native output release must be retried.'});await h.controller.refresh();result=view.render({controller:h.controller,profiles,editor:editor()});assert.ok(result.html.includes('Native output release must be retried.'));assert.ok(result.html.includes('Recover outputs'));await result.handlers.recover();assert.equal(h.calls.at(-1).command,'profile_recover');
  h.client.apply=async()=>{h.calls.push({command:'profile_apply'});throw new Error('Backend unavailable.');};h.setStatus({...running(),state:'degraded',degradation:['Optional companion disconnected.']});await h.controller.apply(editor());result=view.render({controller:h.controller,profiles,editor:editor()});assert.ok(result.html.includes('Backend unavailable.'));assert.ok(result.html.includes('Some profile resources are unavailable. Open Debug for details.'));assert.ok(!result.html.includes('Optional companion disconnected.'));assert.equal(h.calls.filter(call=>call.command==='profile_apply').length,1);
});
test('actual status poll lifecycle is read-only, suspends during game settings or hidden document, and cleanup never stops a native run',async()=>{
  const view=await component(),h=harness();const result=view.render({controller:h.controller,active:false});assert.equal(result.handlers.mountStatus(),undefined);assert.deepEqual(h.calls,[]);result.handlers.setActive(true);const cleanup=result.handlers.watchStatus();await Promise.resolve();assert.deepEqual(h.calls,[{command:'profile_status'}]);await new Promise(resolve=>setImmediate(resolve));
  view.document.hidden=true;view.timers.get(1)();assert.equal(h.calls.length,1);view.document.hidden=false;view.events.get('visibilitychange')();await new Promise(resolve=>setImmediate(resolve));assert.equal(h.calls.length,2);cleanup();assert.equal(view.timers.size,0);assert.equal(view.events.size,0);result.handlers.setActive(false);assert.equal(result.handlers.watchStatus(),undefined);assert.ok(h.calls.every(call=>call.command==='profile_status'));
});

test('waiting launch displays neutral hardware, blocks Apply and keeps Stop available',async()=>{
  const view=await component(),h=harness();h.setStatus({...running(),state:'awaiting-game',owner:{...running().owner,gameId:null}});await h.controller.refresh();h.calls.length=0;
  const result=view.render({controller:h.controller,profiles,editor:{...editor(),id:other}});assert.ok(result.html.includes('held neutral'));assert.ok(result.html.includes('Processing has not started'));assert.match(button(result.html,'Start profile'),/disabled/);assert.ok(!button(result.html,'Stop').includes('disabled'));assert.equal(button(result.html,'Pause'),'');await result.handlers.apply();assert.deepEqual(h.calls,[]);await result.handlers.stop();assert.deepEqual(h.calls,[{command:'profile_stop'}]);
});
