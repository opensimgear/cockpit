import test from'node:test';import assert from'node:assert/strict';import{readFile}from'node:fs/promises';import vm from'node:vm';import ts from'typescript';import{compile}from'svelte/compiler';import{render}from'svelte/server';import*as server from'svelte/internal/server';import * as svelte from'svelte';import{loadModules}from'./test-module-loader.mjs';
const modules=await loadModules(['./games','./profile-inspection']);
const id='11111111-1111-4111-8111-111111111111',hash=`sha256:${'1'.repeat(64)}`,owner={session:'22222222-2222-4222-8222-222222222222',generation:1,profileId:id,profileRevision:2,gameId:null};
const output={id:'virtual',name:'Controls',required:false,ports:[],layout:{buttons:['fire'],axes:[{portId:'steer',range:'bipolar',fallback:12}],hats:['view']}};
const applied=withOwner=>({state:withOwner?'running':'stopped',owner:withOwner?owner:null,appliedSnapshot:withOwner?{profileId:id,revision:2,semanticHash:hash}:null,degradation:[],fault:null});
const sample=port=>({owner,nodeId:'virtual',appliedSnapshot:{profileId:id,revision:2,semanticHash:hash},programHash:hash,ports:[{endpoint:{nodeId:'virtual',portId:port},direction:'input',contract:{delivery:'state',schema:{type:'number',min:-32768,max:32767,unit:null},freshnessMs:1000,fallback:null},sample:{status:'known',value:{kind:'scalar',value:-123}}}]});
async function harness(props={}){
 const file=new URL('./ProfileVirtualInputs.svelte',import.meta.url),source=(await readFile(file,'utf8')).replace('</script>','capture({mount(){mounted=true;return watchInputs();},controls:inputControls,get mode(){return mode;},get samples(){return samples;}});</script>');
 const {js}=compile(source,{filename:file.pathname,generate:'server'}),exports={},events=new Map(),runtimeListeners=new Set(),sampleObservers=[],document={hidden:false,addEventListener(name,fn){events.set(name,fn);},removeEventListener(name){events.delete(name);}};let handlers;
 const runtime={observe(listener){runtimeListeners.add(listener);listener({status:'connecting'});return()=>runtimeListeners.delete(listener);},send(value){for(const listener of [...runtimeListeners])listener(value);}};
 const samples={observe(ownerValue,nodeId,listener){const entry={owner:ownerValue,nodeId,listener,stopped:false};sampleObservers.push(entry);listener({status:'connecting'});return()=>{entry.stopped=true;};},send(entry,value){if(!entry.stopped)entry.listener(value);}};
 vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,document,capture(value){handlers=value;},require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(name.endsWith('.svelte'))return{default(){}};if(name==='profile-read-models/shared'||name==='./profile-read-models/shared')return{observeProfileRuntime:listener=>runtime.observe(listener)};if(name==='profile-samples/shared'||name==='./profile-samples/shared')return{observeProfileNodeSamples:(target,nodeId,listener)=>samples.observe(target,nodeId,listener)};for(const [key,value]of modules)if(name===key||name===key+'.ts')return value;throw Error(`Unexpected dependency ${name}`);}});
 const body=render(exports.default,{props:{output,profileId:id,revision:2,...props}}).body;return{body,handlers,events,document,runtime,samples,sampleObservers};
}
test('virtual preview subscribes only for exact applied owner and loss clears live samples',async()=>{
 const h=await harness(),cleanup=h.handlers.mount();assert.equal(h.handlers.mode,'preview');h.runtime.send({status:'ready',value:{profileSession:applied(false)}});assert.equal(h.handlers.mode,'preview');
 h.runtime.send({status:'ready',value:{profileSession:applied(true)}});assert.equal(h.handlers.mode,'unavailable');assert.equal(h.sampleObservers.length,1);assert.equal(h.sampleObservers[0].nodeId,'virtual');
 const target=h.sampleObservers[0];h.samples.send(target,{status:'ready',value:sample('steer')});assert.equal(h.handlers.mode,'live');assert.equal(h.handlers.controls()[0].value.native,-123);
 h.runtime.send({status:'ready',value:{profileSession:applied(true)}});assert.equal(h.handlers.mode,'live');assert.equal(h.sampleObservers.length,1);
 const nextOwner={...owner,generation:2};h.runtime.send({status:'ready',value:{profileSession:{...applied(true),owner:nextOwner}}});assert.equal(target.stopped,true);assert.equal(h.sampleObservers.length,2);assert.equal(h.handlers.mode,'unavailable');
 target.listener({status:'ready',value:sample('steer')});assert.equal(h.handlers.samples.length,0);assert.equal(h.handlers.mode,'unavailable');
 h.samples.send(h.sampleObservers[1],{status:'lost',error:'gap'});assert.equal(h.handlers.mode,'unavailable');assert.equal(h.handlers.controls()[0].value,null);
 h.runtime.send({status:'ready',value:{profileSession:applied(false)}});assert.equal(target.stopped,true);assert.equal(h.handlers.mode,'preview');cleanup();assert.equal(h.events.size,0);
});
test('visibility and inactive lifecycle detach streams and a visible return needs fresh runtime data',async()=>{
 const h=await harness(),cleanup=h.handlers.mount();h.runtime.send({status:'ready',value:{profileSession:applied(true)}});const first=h.sampleObservers[0];
 h.document.hidden=true;h.events.get('visibilitychange')();assert.equal(first.stopped,true);assert.equal(h.handlers.mode,'preview');
 h.document.hidden=false;h.events.get('visibilitychange')();assert.equal(h.handlers.mode,'preview');assert.equal(h.sampleObservers.length,1);
 h.runtime.send({status:'ready',value:{profileSession:applied(true)}});assert.equal(h.sampleObservers.length,2);assert.equal(h.handlers.mode,'unavailable');
 cleanup();assert.equal(h.events.size,0);assert.equal(h.sampleObservers[1].stopped,true);
 const inactive=await harness({active:false});const stop=inactive.handlers.mount();assert.equal(inactive.handlers.mode,'preview');assert.equal(inactive.sampleObservers.length,0);stop?.();
});
