import assert from'node:assert/strict';import test from'node:test';import{readFile}from'node:fs/promises';import vm from'node:vm';import ts from'typescript';import{compile}from'svelte/compiler';import{render}from'svelte/server';import * as server from'svelte/internal/server';import * as svelte from'svelte';import{loadModules}from'./test-module-loader.mjs';
const modules=await loadModules(['./profiles','./profile-desktop','./profile-desktop-runtime']),plain=value=>JSON.parse(JSON.stringify(value));
async function component(name,send){
 const filename=new URL(name,import.meta.url);let source=await readFile(filename,'utf8'),handlers;
 source=source.replace('</script>',name.includes('Editor')?'captureDesktopUI({beginCapture,keyCapture,save,field,setPicker(value){picker=value;},get state(){return{draft,capture,error,captureBusy}}});</script>':'captureDesktopUI({request,configure,get state(){return{status,busy,error}}});</script>');
 const{js}=compile(source,{filename:filename.pathname,generate:'server'}),exports={};
 vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,captureDesktopUI:value=>handlers=value,require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:send};throw Error(`Unexpected dependency ${name}`);}},{filename:filename.pathname});
 return{render(props={}){return{html:render(exports.default,{props}).body,get handlers(){return handlers;}};}};
}
const preset=modules.get('./profile-desktop').desktopPreset('key-hold','desktop').output;
test('desktop action authoring is passive and physical key capture pauses output before owning picker focus',async()=>{
 const calls=[],changes=[],view=await component('./ProfileDesktopEditor.svelte',async request=>{calls.push(plain(request));return{};});
 const rendered=view.render({output:preset,change:output=>changes.push(plain(output))});assert.deepEqual(calls,[]);assert.match(rendered.html,/Physical key/);assert.match(rendered.html,/Save, import and preview send no input/);
 const h=rendered.handlers;let prevented=0;
 const event=code=>({code,key:'a',preventDefault(){prevented++;},stopPropagation(){prevented++;}});
 h.keyCapture(event('KeyQ'),0);assert.equal(h.state.draft.actions[0].action.key,44);assert.equal(prevented,0);
 await h.beginCapture('action-1');assert.deepEqual(calls,[{command:'profile_pause',paused:true}]);assert.equal(h.state.capture,'action-1');
 h.keyCapture(event('ArrowRight'),0);assert.equal(h.state.draft.actions[0].action.key,79);assert.equal(prevented,2);assert.equal(h.state.capture,'');h.save();assert.equal(changes[0].actions[0].action.key,79);assert.equal(calls.length,1);
 await h.beginCapture('action-1');h.keyCapture(event('AudioVolumeUp'),0);assert.match(h.state.error,/no supported USB HID mapping/);assert.equal(h.state.draft.actions[0].action.key,79);
});
test('rejected native pause cannot install key capture or mutate a desktop action',async()=>{
 let focused=0;const view=await component('./ProfileDesktopEditor.svelte',async()=>{throw Error('Native pause unavailable');});const{handlers:h}=view.render({output:preset,change(){throw Error('No edit expected');}});h.setPicker({focus(){focused++;}});await h.beginCapture('action-1');assert.equal(h.state.capture,'');assert.match(h.state.error,/Native pause unavailable/);assert.equal(h.state.draft.actions[0].action.key,44);assert.equal(focused,0);
});
test('key capture waits for native pause before focusing the rendered picker',async()=>{
 let resolvePause,focused=0;const view=await component('./ProfileDesktopEditor.svelte',()=>new Promise(resolve=>resolvePause=resolve));
 const{handlers:h}=view.render({output:preset,change(){throw Error('No edit expected');}});h.setPicker({focus(){focused++;assert.equal(h.state.capture,'action-1');}});
 const pending=h.beginCapture('action-1');assert.equal(h.state.capture,'');assert.equal(focused,0);resolvePause({});await pending;assert.equal(focused,1);assert.equal(h.state.captureBusy,false);
});
test('desktop controls keep preferences separate, send exact stop and ignore an older status after Stop',async()=>{
 const calls=[];let resolveOld;const stopped={enabled:false,qualified:false,state:'paused',reason:'Explicit Stop',scope:{kind:'application'},application:null,runtime:null};
 const view=await component('./ProfileDesktopControls.svelte',async request=>{calls.push(plain(request));if(request.command==='profile_desktop_status')return new Promise(resolve=>resolveOld=resolve);return{desktop:stopped};});
 const{html,handlers:h}=view.render({requested:true});assert.deepEqual(calls,[]);assert.match(html,/requires keyboard and mouse output/);assert.match(html,/Stop keyboard and mouse output/);
 const old=h.request({command:'profile_desktop_status'});await h.request({command:'profile_desktop_stop'});resolveOld({desktop:{...stopped,state:'active'}});await old;assert.equal(h.state.status.state,'paused');assert.equal(h.state.error,'');
 h.configure(false,{kind:'desktop-wide'});await new Promise(resolve=>setImmediate(resolve));assert.deepEqual(calls.at(-1),{command:'profile_desktop_configure',enabled:false,scope:{kind:'desktop-wide'}});assert.ok(calls.every(request=>!Object.hasOwn(request,'document')&&!Object.hasOwn(request,'pid')&&!Object.hasOwn(request,'executable')));
});
