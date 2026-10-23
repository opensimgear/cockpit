import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import { BROWSER_WIDGET_CAPABILITIES } from '../../../packages/widget-runtime/src/artifact.ts';
import { createOrderedInputQueue } from '../../companion/src/lib/input-queue.ts';
import * as feedbackApi from './feedback.ts';
import * as resolutionApi from './panel-resolution.ts';
import * as credentials from './transport.ts';
const appSource=await readFile(new URL('./App.svelte',import.meta.url),'utf8');
const hash='sha256:'+'a'.repeat(64),assignment={instanceId:'instance',publicationHash:hash,name:'Compiled controls'};
const saved={token:'a'.repeat(43),hostId:'host',companionId:'browser'};
const panel={schemaVersion:4,ui:{id:'panel',title:'Compiled controls',controls:[{id:'toggle',label:'Toggle',kind:'button'},{id:'selector',label:'Selection',kind:'axis',axisMode:'unipolar',fallback:.5}],pages:[{id:'page',title:'Page'}],feedbackWidgets:[{id:'confirmed',channelId:'confirmed',kind:'indicator',label:'Game state'}],assets:[]},presentation:{format:'compiled-raster-v1',widgets:[],viewports:[]}};
const publication={document:panel,assets:[],publicationHash:hash,capabilityFingerprint:hash};
const flush=async()=>{for(let i=0;i<35;i++)await Promise.resolve();};
/** Actual mounted App listeners and real queue/feedback/credential logic. */
function harness({pair=false,hold=[],rejectPanel=false,holdVerification=false,rejectPanelAfter=Infinity,failPreparationAfter=Infinity,holdPreparation=false}={}){
 let handlers,mount,cleanup,focused=true,clock=100,finishVerification,finishPreparation,verifications=0,preparations=0;
 const sockets=[],intervals=new Map(),storage=new Map(),window=new EventTarget(),document=new EventTarget();
 Object.assign(document,{visibilityState:'visible',hasFocus:()=>focused});
 const location={hash:pair?'#pair='+'x'.repeat(43):''},localStorage={getItem:k=>storage.get(k)??null,setItem:(k,v)=>storage.set(k,v),removeItem:k=>storage.delete(k)};
 if(!pair)credentials.saveCredential(localStorage,saved);
 class Socket{
  closed=false;requests=[];pending=[];
  constructor(){this.index=sockets.length+1;sockets.push(this);}async connect(){}
  response(f){
   if(f.type==='auth')return{...saved,assignments:[assignment]};if(f.type==='pair')return{requestId:'offer'};if(f.type==='pair-status')return{type:'paired',...saved};if(f.type==='presence')return{assignments:[assignment]};
   if(f.type==='open')return{clientId:'browser',sessionId:'session-'+this.index,generation:String(this.index),controllerInstanceId:'instance',publicationHash:hash,publication:{canonicalDocument:JSON.stringify(panel),assets:[],publicationHash:hash}};
   if(f.type==='baseline'||f.type==='heartbeat')return{ok:true,type:f.type==='baseline'?'baseline':'ack',feedback:{version:1,type:'feedback',clientId:'browser',sessionId:'session-'+this.index,generation:String(this.index),controllerInstanceId:'instance',publicationHash:hash,sequence:f.type==='baseline'?'1':'2',bindingGeneration:'1',mode:'snapshot',bindings:[{channelId:'confirmed',outputId:'confirmed'}],values:[{channelId:'confirmed',outputId:'confirmed',status:'fresh',value:true,provenance:'game-confirmed',sourceKind:'simulator',sourceAgeMs:10,unit:'boolean'}]}};
   return{ok:true,type:'ack'};
  }
  deliver(f){const reply=this.response(f);this.onreply?.(reply);return reply;}
  request(f){if(this.closed)return Promise.reject(Error('Socket closed'));this.requests.push(structuredClone(f));if(hold.includes(f.type))return new Promise((resolve,reject)=>this.pending.push({frame:f,resolve,reject}));return Promise.resolve(this.deliver(f));}
  release(type){const i=this.pending.findIndex(p=>p.frame.type===type);assert.ok(i>=0);const[p]=this.pending.splice(i,1);p.resolve(this.deliver(p.frame));}
  close(){this.closed=true;for(const p of this.pending.splice(0))p.reject(Error('Socket closed'));this.onclose?.();}
 }
 const injected=appSource.replace('</script>',`capture({poll,connect,pair,emitOutput,runtimeError,consume,queue,get epoch(){return epoch;},get active(){return active;},get panel(){return panel;},get surfaces(){return panelSurfaces;},get suppressFeedbackCues(){return suppressFeedbackCues;},get panelError(){return panelError;},get session(){return session;},get status(){return status;},get feedback(){return feedback;},get credential(){return credential;},get secret(){return secret;}});</script>`);
 const {js}=compile(injected,{filename:'App.svelte',generate:'server'}),exports={};
 vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{
  exports,structuredClone,TextEncoder,URLSearchParams,AbortController,performance:{now:()=>clock},crypto,window,document,location,history:{replaceState(){location.hash='';}},navigator:{},localStorage,
  setInterval(cb){const id=intervals.size+1;intervals.set(id,cb);return id;},clearInterval:id=>intervals.delete(id),capture:v=>handlers=v,
  require(name){
   if(name==='svelte/internal/server')return server;if(name==='svelte')return{...svelte,onMount:cb=>mount=cb};
   if(name==='../../../packages/companion-ui/src/CompiledPanel.svelte')return{default(){}};if(name==='../../../packages/widget-runtime/src/artifact.ts')return{BROWSER_WIDGET_CAPABILITIES};
   if(name==='../../../packages/companion-ui/src/compiled-surface.ts')return{prepareCompiledSurfaces:async()=>{if(++preparations>failPreparationAfter)throw new RangeError('Candidate Canvas allocation failed');if(holdPreparation)await new Promise(resolve=>finishPreparation=resolve);return{retained:{pixels:[0,248,255],disposed:false,dispose(){this.disposed=true;this.pixels=[];}}};},disposeCompiledSurfaces:surfaces=>{for(const surface of Object.values(surfaces??{}))surface.dispose();}};
   if(name==='./compiled-cache')return{browserCompiledCacheStore:()=>({}),createCompiledAssetCache:()=>({haveAssets:async()=>['b'.repeat(64)],save:async()=>{},verifyDelivery:async()=>{if(rejectPanel||++verifications>rejectPanelAfter)throw Error('Unsupported compiled widget');if(holdVerification)await new Promise(resolve=>finishVerification=resolve);return publication;}})};
   if(name==='../../companion/src/lib/input-queue')return{createOrderedInputQueue};if(name==='./transport')return{...credentials,BrowserTransport:Socket};if(name==='./feedback')return feedbackApi;if(name==='./panel-resolution')return resolutionApi;throw Error(name);
  },
 });render(exports.default).body;cleanup=mount();
 return{handlers,sockets,storage,async focus(v){focused=v;window.dispatchEvent(new Event(v?'focus':'blur'));await flush();},async visibility(v){document.visibilityState=v;document.dispatchEvent(new Event('visibilitychange'));await flush();},async pagehide(){window.dispatchEvent(new Event('pagehide'));await flush();},async pageshow(){window.dispatchEvent(new Event('pageshow'));await flush();},async tick(){for(const cb of intervals.values())cb();await flush();},close(){cleanup();},finishVerification(){finishVerification();},finishPreparation(){finishPreparation();}};
}
test('compiled delivery advertises capabilities and cached hashes before neutral baseline',async()=>{const h=harness();await flush();try{const open=h.sockets[0].requests.find(f=>f.type==='open');assert.deepEqual(open.widgetCapabilities,BROWSER_WIDGET_CAPABILITIES);assert.deepEqual(open.haveAssets,['b'.repeat(64)]);assert.deepEqual(h.sockets[0].requests.find(f=>f.type==='baseline').values,{toggle:false,selector:.5});assert.equal(h.handlers.active,true);assert.equal(h.handlers.queue.snapshot().enabled,true);}finally{h.close();}});
test('blur expires queued input; focus baselines a fresh session before rearming',async()=>{
 const h=harness({hold:['input']});await flush();try{
  h.handlers.emitOutput({controlId:'toggle',value:true});h.handlers.emitOutput({controlId:'toggle',value:false});await flush();const old=h.sockets[0];assert.equal(old.requests.filter(f=>f.type==='input').length,1);assert.equal(h.handlers.queue.snapshot().pending,1);
  await h.focus(false);assert.equal(old.closed,true);assert.equal(old.pending.length,0);assert.equal(old.requests.filter(f=>f.type==='input').length,1);assert.equal(h.sockets.length,2);const background=h.sockets[1];assert.deepEqual(background.requests.find(f=>f.type==='baseline').values,{toggle:false,selector:.5});assert.equal(h.handlers.queue.snapshot().enabled,false);
  h.handlers.emitOutput({controlId:'toggle',value:true});await h.tick();assert.equal(background.requests.some(f=>f.type==='input'),false);assert.equal(h.handlers.feedback.confirmed.value,true);
  await h.focus(true);assert.equal(background.closed,true);assert.equal(h.handlers.queue.snapshot().enabled,true);const next=h.sockets[2];h.handlers.emitOutput({controlId:'toggle',value:true});await flush();assert.ok(next.requests.findIndex(f=>f.type==='input')>next.requests.findIndex(f=>f.type==='baseline'));assert.equal(next.requests.find(f=>f.type==='input').sequence,'1');assert.notEqual(next.requests.find(f=>f.type==='input').sessionId,old.requests.find(f=>f.type==='input').sessionId);
 }finally{h.close();await flush();}
});
test('visible blur preserves authentication and pending pairing approval',async()=>{for(const pair of [true,false]){const kind=pair?'pair-status':'auth',h=harness({pair,hold:[kind]});await flush();try{const socket=h.sockets[0];assert.equal(socket.pending[0]?.frame.type,kind);await h.focus(false);assert.equal(socket.closed,false);socket.release(kind);await flush();if(pair){assert.ok(h.handlers.credential);assert.ok(h.storage.has('osg-browser-credential-v2'));}const current=h.sockets.at(-1);assert.ok(current.requests.some(f=>f.type==='baseline'));assert.equal(h.handlers.active,true);assert.equal(h.handlers.queue.snapshot().enabled,false);assert.equal(current.requests.some(f=>f.type==='input'),false);}finally{h.close();await flush();}}});
test('hidden and unloaded pages cancel callbacks until visible restoration',async()=>{const h=harness({hold:['auth']});await flush();try{const old=h.sockets[0];await h.visibility('hidden');assert.equal(old.closed,true);await h.tick();await h.handlers.connect();assert.equal(h.sockets.length,1);assert.equal(h.handlers.session,null);await h.visibility('visible');assert.equal(h.sockets.length,2);h.sockets[1].release('auth');await flush();assert.equal(h.handlers.active,true);await h.pagehide();assert.equal(h.sockets[1].closed,true);await h.tick();assert.equal(h.sockets.length,2);await h.pageshow();h.sockets[2].release('auth');await flush();assert.equal(h.handlers.queue.snapshot().enabled,true);}finally{h.close();await flush();}});
test('hidden consumed pairing offers cannot save credentials or replay',async()=>{const h=harness({pair:true,hold:['pair-status']});await flush();try{const old=h.sockets[0];await h.visibility('hidden');assert.equal(old.closed,true);assert.equal(h.handlers.credential,null);assert.equal(h.handlers.secret,'');assert.equal(h.storage.has('osg-browser-credential-v2'),false);await h.visibility('visible');await h.tick();assert.equal(h.sockets.length,1);assert.equal(old.requests.filter(f=>f.type==='pair').length,1);}finally{h.close();await flush();}});
test('incompatible content stays connected and disarmed with error heartbeats',async()=>{const h=harness({rejectPanel:true});await flush();try{assert.equal(h.handlers.panelError,true);assert.equal(h.handlers.active,false);assert.ok(h.handlers.session);assert.equal(h.sockets[0].closed,false);assert.equal(h.sockets[0].requests.some(f=>f.type==='baseline'||f.type==='input'),false);await h.tick();assert.ok(h.sockets[0].requests.some(f=>f.type==='heartbeat'&&f.panelError===true));assert.equal(h.handlers.queue.snapshot().enabled,false);}finally{h.close();await flush();}});
test('late verified payload completion cannot activate an obsolete hidden epoch',async()=>{const h=harness({holdVerification:true});await flush();try{assert.equal(h.sockets[0].requests.some(f=>f.type==='open'),true);await h.visibility('hidden');h.finishVerification();await flush();assert.equal(h.handlers.panel,null);assert.equal(h.handlers.active,false);assert.equal(h.sockets[0].requests.some(f=>f.type==='baseline'),false);}finally{h.close();await flush();}});
test('malformed assets or candidate allocation failure retain old pixels while closing every old input path',async()=>{
 for(const options of [{rejectPanelAfter:1},{failPreparationAfter:1}]){
  const h=harness(options);await flush();try{
   const old=h.sockets[0],surface=h.handlers.surfaces.retained,snapshot=[...surface.pixels];assert.equal(h.handlers.active,true);h.handlers.emitOutput({controlId:'toggle',value:true});await flush();
   await h.focus(false);assert.equal(old.closed,true);assert.equal(h.handlers.panel,publication);assert.equal(h.handlers.surfaces.retained,surface);assert.deepEqual(surface.pixels,snapshot);assert.equal(surface.disposed,false);
   assert.equal(h.handlers.active,false);assert.equal(h.handlers.panelError,true);assert.equal(h.handlers.queue.snapshot().enabled,false);
   const candidate=h.sockets.at(-1);assert.equal(candidate.requests.some(f=>f.type==='baseline'),false);h.handlers.emitOutput({controlId:'toggle',value:true});await flush();assert.equal(candidate.requests.some(f=>f.type==='input'),false);assert.equal(old.requests.filter(f=>f.type==='input').length,1);
  }finally{h.close();await flush();}
 }
});
test('prepared pixels from a late hidden candidate are disposed without committing or baselining',async()=>{
 const h=harness({holdPreparation:true});await flush();try{await h.visibility('hidden');h.finishPreparation();await flush();assert.equal(h.handlers.panel,null);assert.equal(h.handlers.active,false);assert.equal(h.sockets[0].requests.some(f=>f.type==='baseline'),false);}finally{h.close();await flush();}
});
test('only accepted live feedback deltas enable cues; snapshots, resync and invalid duplicates cannot replay them',async()=>{
 const h=harness();await flush();try{assert.equal(h.handlers.suppressFeedbackCues,true);const socket=h.sockets[0],delta=socket.response({type:'heartbeat'});delta.feedback.mode='delta';delete delta.feedback.bindings;h.handlers.consume(delta,h.handlers.epoch);assert.equal(h.handlers.suppressFeedbackCues,false);
  const duplicate=socket.response({type:'heartbeat'});h.handlers.consume(duplicate,h.handlers.epoch);assert.equal(h.handlers.suppressFeedbackCues,false);
  const snapshot=structuredClone(duplicate);snapshot.feedback.sequence='3';h.handlers.consume(snapshot,h.handlers.epoch);assert.equal(h.handlers.suppressFeedbackCues,true);
  const resync=structuredClone(snapshot);resync.type='feedback-resync';resync.feedback.sequence='4';h.handlers.consume(resync,h.handlers.epoch);assert.equal(h.handlers.suppressFeedbackCues,true);
 }finally{h.close();await flush();}
});
test('a player failure releases held output, closes its session and bounds reconnect attempts',async()=>{
 const h=harness({hold:['input']});await flush();try{
  h.handlers.emitOutput({controlId:'toggle',value:true});h.handlers.emitOutput({controlId:'toggle',value:false});await flush();const socket=h.sockets[0];assert.equal(socket.requests.filter(frame=>frame.type==='input').length,1);
  h.handlers.runtimeError('Canvas context unavailable');await flush();assert.equal(socket.closed,true);assert.equal(h.handlers.active,false);assert.equal(h.handlers.session,null);assert.equal(h.handlers.queue.snapshot().enabled,false);assert.equal(h.handlers.queue.snapshot().pending,0);
  h.handlers.emitOutput({controlId:'toggle',value:true});await h.tick();await h.handlers.connect();assert.equal(h.sockets.length,1,'A failed renderer must not spin fresh sessions immediately.');assert.equal(socket.requests.filter(frame=>frame.type==='input').length,1);
 }finally{h.close();await flush();}
});
