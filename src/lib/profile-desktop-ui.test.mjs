import assert from'node:assert/strict';import test from'node:test';import{readFile}from'node:fs/promises';import vm from'node:vm';import ts from'typescript';import{compile}from'svelte/compiler';import{render}from'svelte/server';import * as server from'svelte/internal/server';import * as svelte from'svelte';import{loadModules}from'./test-module-loader.mjs';
const modules=await loadModules(['./profiles','./profile-desktop','./profile-desktop-runtime','./profile-runtime']),plain=value=>JSON.parse(JSON.stringify(value));
async function component(name,send){
 const filename=new URL(name,import.meta.url);let source=await readFile(filename,'utf8'),handlers;
 source=source.replace('</script>',name.includes('Editor')?'captureDesktopUI({beginCapture,keyCapture,save,field,scope,captureApplication,setPicker(value){picker=value;},get state(){return{draft,capture,error,captureBusy,applications,applicationBusy}}});</script>':'captureDesktopUI({request,configure,get state(){return{status,busy,error}}});</script>');
 const{js}=compile(source,{filename:filename.pathname,generate:'server'}),exports={};
 vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,captureDesktopUI:value=>handlers=value,require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:send};throw Error(`Unexpected dependency ${name}`);}},{filename:filename.pathname});
 return{render(props={}){return{html:render(exports.default,{props}).body,get handlers(){return handlers;}};}};
}
const preset=modules.get('./profile-desktop').desktopPreset('key-hold','desktop').output;
const stopped=()=>({state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null});
const stoppedReply=()=>({profiles:{profileSession:stopped()}});
test('desktop action authoring is passive and physical key capture stops output before owning picker focus',async()=>{
 const calls=[],changes=[],view=await component('./ProfileDesktopEditor.svelte',async request=>{calls.push(plain(request));return stoppedReply();});
 const rendered=view.render({output:preset,change:output=>changes.push(plain(output))});assert.deepEqual(calls,[]);assert.match(rendered.html,/Physical key/);assert.match(rendered.html,/Save updates an active profile/);assert.match(rendered.html,/Saving an inactive profile sends no input/);assert.match(rendered.html,/activate it when ready/);assert.doesNotMatch(rendered.html,/revision|Apply|Start profile|running profile/i);
 const h=rendered.handlers;let prevented=0;
 const event=code=>({code,key:'a',preventDefault(){prevented++;},stopPropagation(){prevented++;}});
 h.keyCapture(event('KeyQ'),0);assert.equal(h.state.draft.actions[0].action.key,44);assert.equal(prevented,0);
 await h.beginCapture('action-1');assert.deepEqual(calls,[{command:'profile_stop'}]);assert.equal(h.state.capture,'action-1');
 h.keyCapture(event('ArrowRight'),0);assert.equal(h.state.draft.actions[0].action.key,79);assert.equal(prevented,2);assert.equal(h.state.capture,'');h.save();assert.equal(changes[0].actions[0].action.key,79);assert.equal(calls.length,1);
 await h.beginCapture('action-1');h.keyCapture(event('AudioVolumeUp'),0);assert.match(h.state.error,/no supported USB HID mapping/);assert.equal(h.state.draft.actions[0].action.key,79);
});
test('rejected native Stop cannot install key capture or mutate a desktop action',async()=>{
 let focused=0;const view=await component('./ProfileDesktopEditor.svelte',async()=>{throw Error('Native Stop unavailable');});const{handlers:h}=view.render({output:preset,change(){throw Error('No edit expected');}});h.setPicker({focus(){focused++;}});await h.beginCapture('action-1');assert.equal(h.state.capture,'');assert.match(h.state.error,/Native Stop unavailable/);assert.equal(h.state.draft.actions[0].action.key,44);assert.equal(focused,0);
});
test('key capture waits for native Stop before focusing the rendered picker',async()=>{
 let resolveStop,focused=0;const view=await component('./ProfileDesktopEditor.svelte',()=>new Promise(resolve=>resolveStop=resolve));
 const{handlers:h}=view.render({output:preset,change(){throw Error('No edit expected');}});h.setPicker({focus(){focused++;assert.equal(h.state.capture,'action-1');}});
 const pending=h.beginCapture('action-1');assert.equal(h.state.capture,'');assert.equal(focused,0);resolveStop(stoppedReply());await pending;assert.equal(focused,1);assert.equal(h.state.captureBusy,false);
});
test('key capture requires native confirmation that outputs are stopped and released',async()=>{
 const snapshot={profileId:'00000000-0000-4000-8000-000000000001',revision:1,semanticHash:'sha256:'+'a'.repeat(64)};
 const owner={session:'11111111-1111-4111-8111-111111111111',generation:1,profileId:snapshot.profileId,profileRevision:1,gameId:null};
 for(const profileSession of [{...stopped(),state:'recovery-needed',fault:'Output cleanup is incomplete'}, {...stopped(),state:'running',appliedSnapshot:snapshot,owner}, {...stopped(),appliedSnapshot:snapshot}, {...stopped(),fault:'Output cleanup is incomplete'}]){
  let focused=0;const view=await component('./ProfileDesktopEditor.svelte',async()=>({profiles:{profileSession}}));const{handlers:h}=view.render({output:preset,change(){throw Error('No edit expected');}});h.setPicker({focus(){focused++;}});await h.beginCapture('action-1');assert.equal(focused,0);assert.equal(h.state.capture,'');assert.equal(h.state.captureBusy,false);assert.match(h.state.error,/Deactivate and recover/);
 }
 const view=await component('./ProfileDesktopEditor.svelte',async()=>({}));const{handlers:h}=view.render({output:preset,change(){throw Error('No edit expected');}});await h.beginCapture('action-1');assert.equal(h.state.capture,'');assert.match(h.state.error,/status is malformed/);
});
test('desktop controls keep preferences separate, send exact stop and ignore an older status after Stop',async()=>{
 const calls=[];let resolveOld;const stopped={enabled:false,qualified:false,state:'paused',reason:'Explicit Stop',scope:{kind:'application'},application:null,runtime:null};
 const view=await component('./ProfileDesktopControls.svelte',async request=>{calls.push(plain(request));if(request.command==='profile_desktop_status')return new Promise(resolve=>resolveOld=resolve);return{desktop:stopped};});
 const{html,handlers:h}=view.render({requested:true});assert.deepEqual(calls,[]);assert.match(html,/requires keyboard and mouse output/);assert.match(html,/Deactivate profile outputs/);assert.match(html,/releases its outputs/);assert.doesNotMatch(html,/Pause|Clear pause|Start profile|Stop keyboard and mouse output/);
 const old=h.request({command:'profile_desktop_status'});await h.request({command:'profile_desktop_stop'});resolveOld({desktop:{...stopped,state:'active'}});await old;assert.equal(h.state.status.state,'paused');assert.equal(h.state.error,'');
 h.configure(false);await new Promise(resolve=>setImmediate(resolve));assert.deepEqual(calls.at(-1),{command:'profile_desktop_configure',enabled:false,scope:{kind:'application'}});assert.ok(calls.every(request=>!Object.hasOwn(request,'document')&&!Object.hasOwn(request,'pid')&&!Object.hasOwn(request,'executable')));assert.doesNotMatch(html,/Application scope|Use last focused external application|Native owners/);
});
test('keyboard scope edits belong to the graph and native application capture saves only an opaque reference',async()=>{
 const calls=[],changes=[],reference='01234567-89ab-4cde-8fab-0123456789ab';
 const view=await component('./ProfileDesktopEditor.svelte',async request=>{calls.push(plain(request));return{desktop:{enabled:true,qualified:true,state:'running',reason:null,scope:{kind:'desktop-wide'},applications:[{reference,name:'Receiver.exe'}],capturedApplicationReference:reference}};});
 const{html,handlers:h}=view.render({output:preset,change:output=>changes.push(plain(output))});
 assert.match(html,/Keyboard and mouse scope/);assert.match(html,/Scope is saved with this keyboard and mouse block/);assert.deepEqual(calls,[]);
 h.scope('desktop-wide');assert.deepEqual(changes.at(-1).scope,{kind:'desktop-wide'});assert.deepEqual(calls,[]);
 h.scope('selected-game');assert.deepEqual(changes.at(-1).scope,{kind:'selected-game'});
 h.scope('application');await h.captureApplication();assert.deepEqual(calls,[{command:'profile_desktop_capture_application'}]);assert.deepEqual(changes.at(-1).scope,{kind:'application',applicationReference:reference});assert.equal(h.state.applicationBusy,false);assert.equal(h.state.error,'');assert.doesNotMatch(JSON.stringify(changes),/pid|executable|Receiver\.exe/);
});
test('a delayed application capture cannot replace a newer desktop-wide scope',async()=>{
 let resolve;const changes=[],view=await component('./ProfileDesktopEditor.svelte',()=>new Promise(done=>resolve=done));
 const{handlers:h}=view.render({output:preset,change:output=>changes.push(plain(output))});
 const pending=h.captureApplication();h.scope('desktop-wide');resolve({desktop:{enabled:true,qualified:true,state:'running',reason:null,scope:{kind:'application'},capturedApplicationReference:'01234567-89ab-4cde-8fab-0123456789ab'}});await pending;assert.deepEqual(plain(h.state.draft.scope),{kind:'desktop-wide'});assert.deepEqual(changes.at(-1).scope,{kind:'desktop-wide'});
});
