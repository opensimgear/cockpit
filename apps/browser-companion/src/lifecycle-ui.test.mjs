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
const assertObsoleteHidden=h=>assert.equal(JSON.stringify([h.handlers.error,h.handlers.status,h.handlers.feedbackNotice]).includes('Session is obsolete'),false,'Protocol recovery details stay out of user-facing state.');
/** Actual mounted App listeners and real queue/feedback/credential logic. */
function harness({pair=false,hold=[],rejectPanel=false,holdVerification=false,rejectPanelAfter=Infinity,failPreparationAfter=Infinity,holdPreparation=false,displayOnly=false,staleOnce=[]}={}){
 let handlers,mount,cleanup,focused=true,clock=100,advanced=0,finishVerification,finishPreparation,verifications=0,preparations=0;const preparedSurfaces=[];
 const sockets=[],intervals=new Map(),storage=new Map(),staleUsed=new Set(),window=new EventTarget(),document=new EventTarget();
 Object.assign(document,{visibilityState:'visible',hasFocus:()=>focused});
 const location={hash:pair?'#pair='+'x'.repeat(43):''},localStorage={getItem:k=>storage.get(k)??null,setItem:(k,v)=>storage.set(k,v),removeItem:k=>storage.delete(k)};
 if(!pair)credentials.saveCredential(localStorage,saved);
 class Socket{
  closed=false;requests=[];pending=[];opens=0;
  constructor(){this.index=sockets.length+1;sockets.push(this);}async connect(){}
  response(f){
   if(staleOnce.includes(f.type)&&!staleUsed.has(f.type)){staleUsed.add(f.type);throw Error('Session is obsolete; explicitly reconnect.');}
   if(['heartbeat','baseline','input','close','feedback-resync'].includes(f.type)&&this.lastActivityAt!==undefined){const lease=this.preparing?30000:1500;if(clock-this.lastActivityAt>=lease)throw Error('Session is obsolete; explicitly reconnect.');if(f.type==='heartbeat'){this.lastActivityAt=clock;this.preparing=['checking-cache','downloading','rendering'].includes(f.operation?.phase);}if(f.type==='baseline'){this.lastActivityAt=clock;this.preparing=false;}}
   if(f.type==='auth')return{...saved,assignments:[assignment]};if(f.type==='pair')return{requestId:'offer'};if(f.type==='pair-status')return{type:'paired',...saved};if(f.type==='presence')return{assignments:[assignment]};
   if(f.type==='open'){this.lastActivityAt=clock;return{clientId:'browser',sessionId:`session-${this.index}-${++this.opens}`,generation:String(this.index),controllerInstanceId:'instance',publicationHash:hash,publication:{canonicalDocument:JSON.stringify(panel),assets:[],publicationHash:hash}};}
   if(f.type==='baseline'||f.type==='heartbeat')return{ok:true,type:f.type==='baseline'?'baseline':'ack',feedback:{version:1,type:'feedback',clientId:'browser',sessionId:`session-${this.index}-${this.opens}`,generation:String(this.index),controllerInstanceId:'instance',publicationHash:hash,sequence:f.type==='baseline'?'1':'2',bindingGeneration:'1',mode:'snapshot',bindings:[{channelId:'confirmed',outputId:'confirmed'}],values:[{channelId:'confirmed',outputId:'confirmed',status:'fresh',value:true,provenance:'game-confirmed',sourceKind:'simulator',sourceAgeMs:10,unit:'boolean'}]}};
   return{ok:true,type:'ack'};
  }
  deliver(f){const reply=this.response(f);this.onreply?.(reply);return reply;}
  request(f){if(this.closed)return Promise.reject(Error('Socket closed'));this.requests.push(structuredClone(f));if(hold.includes(f.type))return new Promise((resolve,reject)=>this.pending.push({frame:f,resolve,reject}));return Promise.resolve(this.deliver(f));}
  release(type){const i=this.pending.findIndex(p=>p.frame.type===type);assert.ok(i>=0);const[p]=this.pending.splice(i,1);p.resolve(this.deliver(p.frame));}
  close(){this.closed=true;for(const p of this.pending.splice(0))p.reject(Error('Socket closed'));this.onclose?.();}
 }
 const injected=appSource.replace('</script>',`capture({poll,connect,pair,emitOutput,runtimeError,consume,resyncFeedback,queue,get epoch(){return epoch;},get active(){return active;},get panel(){return panel;},get surfaces(){return panelSurfaces;},get suppressFeedbackCues(){return suppressFeedbackCues;},get panelError(){return panelError;},get session(){return session;},get status(){return status;},get error(){return error;},get feedbackNotice(){return feedbackNotice;},get feedback(){return feedback;},get credential(){return credential;},get secret(){return secret;}});</script>`);
 const {js}=compile(injected,{filename:'App.svelte',generate:'server'}),exports={};
 vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{
  exports,structuredClone,TextEncoder,URLSearchParams,AbortController,performance:{now:()=>clock},crypto,window,document,location,history:{replaceState(){location.hash='';}},navigator:{},localStorage,
  setInterval(cb,period=0){const id=intervals.size+1;intervals.set(id,{cb,period});return id;},clearInterval:id=>intervals.delete(id),capture:v=>handlers=v,
  require(name){
   if(name==='svelte/internal/server')return server;if(name==='svelte')return{...svelte,onMount:cb=>mount=cb};
   if(name==='../../../packages/companion-ui/src/CompiledPanel.svelte')return{default(){}};if(name==='../../../packages/widget-runtime/src/artifact.ts')return{BROWSER_WIDGET_CAPABILITIES};
   if(name==='../../../packages/companion-ui/src/compiled-surface.ts')return{prepareCompiledSurfaces:async()=>{if(++preparations>failPreparationAfter)throw new RangeError('Candidate Canvas allocation failed');if(holdPreparation)await new Promise(resolve=>finishPreparation=resolve);const surfaces={retained:{pixels:[0,248,255],disposed:false,dispose(){this.disposed=true;this.pixels=[];}}};preparedSurfaces.push(surfaces);return surfaces;},disposeCompiledSurfaces:surfaces=>{for(const surface of Object.values(surfaces??{}))surface.dispose();}};
   if(name==='./compiled-cache')return{browserCompiledCacheStore:()=>({}),createCompiledAssetCache:()=>({haveAssets:async()=>['b'.repeat(64)],save:async()=>{},verifyDelivery:async()=>{if(rejectPanel||++verifications>rejectPanelAfter)throw Error('Unsupported compiled widget');if(holdVerification)await new Promise(resolve=>finishVerification=resolve);return displayOnly?{...publication,document:{...panel,ui:{...panel.ui,controls:[]}}}:publication;}})};
   if(name==='../../companion/src/lib/input-queue')return{createOrderedInputQueue};if(name==='./transport')return{...credentials,BrowserTransport:Socket};if(name==='./feedback')return feedbackApi;if(name==='./panel-resolution')return resolutionApi;throw Error(name);
  },
 });render(exports.default).body;cleanup=mount();
 return{handlers,sockets,storage,preparedSurfaces,async focus(v){focused=v;window.dispatchEvent(new Event(v?'focus':'blur'));await flush();},async visibility(v){document.visibilityState=v;document.dispatchEvent(new Event('visibilitychange'));await flush();},async pagehide(){window.dispatchEvent(new Event('pagehide'));await flush();},async pageshow(){window.dispatchEvent(new Event('pageshow'));await flush();},async tick(){for(const timer of intervals.values())timer.cb();await flush();},async advance(ms){for(let elapsed=0;elapsed<ms;elapsed+=250){clock+=250;advanced+=250;for(const timer of intervals.values())if(timer.period&&advanced%timer.period===0)timer.cb();await flush();}},close(){cleanup();},finishVerification(){finishVerification();},finishPreparation(){finishPreparation();}};
}
test('compiled delivery advertises capabilities and cached hashes before neutral baseline',async()=>{const h=harness();await flush();try{const open=h.sockets[0].requests.find(f=>f.type==='open');assert.deepEqual(open.widgetCapabilities,BROWSER_WIDGET_CAPABILITIES);assert.deepEqual(open.haveAssets,['b'.repeat(64)]);assert.deepEqual(h.sockets[0].requests.find(f=>f.type==='baseline').values,{toggle:false,selector:.5});assert.equal(h.handlers.active,true);assert.equal(h.handlers.queue.snapshot().enabled,true);}finally{h.close();}});
test('blur drops queued input and neutralizes controls without closing the socket; focus rebaselines on that socket',async()=>{
 const h=harness({hold:['input']});await flush();try{
  h.handlers.emitOutput({controlId:'toggle',value:true});h.handlers.emitOutput({controlId:'toggle',value:false});await flush();const old=h.sockets[0];assert.equal(old.requests.filter(f=>f.type==='input').length,1);assert.equal(h.handlers.queue.snapshot().pending,1);
  await h.focus(false);assert.equal(old.closed,false);assert.equal(h.sockets.length,1);assert.equal(h.handlers.queue.snapshot().pending,0);assert.equal(h.handlers.queue.snapshot().enabled,false);
  const neutral=old.requests.filter(f=>f.type==='input').at(-1);assert.deepEqual(neutral.changes,[{controlId:'toggle',value:false},{controlId:'selector',value:.5}]);assert.equal(neutral.sequence,'2');
  old.release('input');old.release('input');await flush();
  h.handlers.emitOutput({controlId:'toggle',value:true});await h.tick();assert.equal(old.requests.filter(f=>f.type==='input').length,2);assert.equal(h.handlers.feedback.confirmed.value,true);assert.ok(old.requests.some(f=>f.type==='heartbeat'));
  await h.focus(true);assert.equal(old.closed,false);assert.equal(h.sockets.length,1);assert.equal(h.handlers.queue.snapshot().enabled,true);h.handlers.emitOutput({controlId:'toggle',value:true});await flush();const next=old.requests.filter(f=>f.type==='input').at(-1);assert.equal(next.sequence,'1');assert.notEqual(next.sessionId,neutral.sessionId);assert.equal(old.requests.filter(f=>f.type==='baseline').length,2);assert.equal(old.requests.filter(f=>f.type==='auth').length,1);
 }finally{h.close();await flush();}
});
test('visible blur preserves authentication and pending pairing approval',async()=>{for(const pair of [true,false]){const kind=pair?'pair-status':'auth',h=harness({pair,hold:[kind]});await flush();try{const socket=h.sockets[0];assert.equal(socket.pending[0]?.frame.type,kind);await h.focus(false);assert.equal(socket.closed,false);socket.release(kind);await flush();if(pair){assert.ok(h.handlers.credential);assert.ok(h.storage.has('osg-browser-credential-v2'));}const current=h.sockets.at(-1);assert.ok(current.requests.some(f=>f.type==='baseline'));assert.equal(h.handlers.active,true);assert.equal(h.handlers.queue.snapshot().enabled,false);assert.equal(current.requests.some(f=>f.type==='input'),false);}finally{h.close();await flush();}}});
test('hidden pages retain authentication; actual page unload closes it',async()=>{const h=harness({hold:['auth']});await flush();try{const old=h.sockets[0];await h.visibility('hidden');assert.equal(old.closed,false);old.release('auth');await flush();await h.tick();assert.equal(h.sockets.length,1);assert.equal(h.handlers.session,null);await h.visibility('visible');await h.tick();assert.equal(h.handlers.active,true);assert.equal(h.sockets.length,1);await h.pagehide();assert.equal(old.closed,true);await h.tick();assert.equal(h.sockets.length,1);await h.pageshow();h.sockets[1].release('auth');await flush();assert.equal(h.handlers.queue.snapshot().enabled,true);}finally{h.close();await flush();}});
test('hidden consumed pairing offers cannot save credentials or replay',async()=>{const h=harness({pair:true,hold:['pair-status']});await flush();try{const old=h.sockets[0];await h.visibility('hidden');assert.equal(old.closed,true);assert.equal(h.handlers.credential,null);assert.equal(h.handlers.secret,'');assert.equal(h.storage.has('osg-browser-credential-v2'),false);await h.visibility('visible');await h.tick();assert.equal(h.sockets.length,1);assert.equal(old.requests.filter(f=>f.type==='pair').length,1);}finally{h.close();await flush();}});
test('incompatible content stays connected and disarmed with error heartbeats',async()=>{const h=harness({rejectPanel:true});await flush();try{assert.equal(h.handlers.panelError,true);assert.equal(h.handlers.active,false);assert.ok(h.handlers.session);assert.equal(h.sockets[0].closed,false);assert.equal(h.sockets[0].requests.some(f=>f.type==='baseline'||f.type==='input'),false);await h.tick();assert.ok(h.sockets[0].requests.some(f=>f.type==='heartbeat'&&f.panelError===true));assert.equal(h.handlers.queue.snapshot().enabled,false);}finally{h.close();await flush();}});
test('a verified payload can finish in the background without enabling controls',async()=>{const h=harness({holdVerification:true});await flush();try{await h.visibility('hidden');h.finishVerification();await flush();assert.equal(h.handlers.panel,publication);assert.equal(h.handlers.active,true);assert.equal(h.handlers.queue.snapshot().enabled,false);assert.equal(h.sockets[0].closed,false);}finally{h.close();await flush();}});
test('slow panel preparation renews its disarmed session until the first neutral baseline',async()=>{
 const h=harness({holdPreparation:true});await flush();try{
  const socket=h.sockets[0],firstKeepalive=socket.requests.find(f=>f.type==='heartbeat');assert.ok(firstKeepalive,'The first preparation heartbeat should be sent immediately after open.');assert.equal(socket.requests.filter(f=>f.type==='baseline').length,0);
  await h.advance(35000);
  const preparingHeartbeats=socket.requests.filter(f=>f.type==='heartbeat');assert.ok(preparingHeartbeats.length>=60,'Preparation should keep renewing the session beyond the bounded 30s preparation lease.');
  assert.ok(preparingHeartbeats.every(f=>f.operation?.phase==='checking-cache'&&f.operation.sessionId===f.sessionId&&f.operation.generation===f.generation),'Keepalives must report a current, feedback-preserving preparation operation.');
  assert.equal(h.handlers.active,false);assert.equal(h.handlers.queue.snapshot().enabled,false);
  h.finishPreparation();await flush();await flush();
  assert.equal(socket.closed,false);assert.equal(h.sockets.length,1);assert.equal(socket.requests.filter(f=>f.type==='auth').length,1);
  assert.equal(socket.requests.filter(f=>f.type==='baseline').length,1);assert.equal(h.handlers.active,true);
 }finally{h.close();await flush();}
});
test('malformed assets or candidate allocation failure retain old pixels while closing every old input path',async()=>{
 for(const options of [{rejectPanelAfter:1},{failPreparationAfter:1}]){
  const h=harness(options);await flush();try{
   const old=h.sockets[0],surface=h.handlers.surfaces.retained,snapshot=[...surface.pixels];assert.equal(h.handlers.active,true);h.handlers.emitOutput({controlId:'toggle',value:true});await flush();
   await h.focus(false);await h.focus(true);assert.equal(old.closed,false);assert.equal(h.handlers.panel,publication);assert.equal(h.handlers.surfaces.retained,surface);assert.deepEqual(surface.pixels,snapshot);assert.equal(surface.disposed,false);
   assert.equal(h.handlers.active,false);assert.equal(h.handlers.panelError,true);assert.equal(h.handlers.queue.snapshot().enabled,false);
   const candidate=h.sockets.at(-1),afterClose=candidate.requests.slice(candidate.requests.findLastIndex(f=>f.type==='close')+1);assert.equal(afterClose.some(f=>f.type==='baseline'),false);h.handlers.emitOutput({controlId:'toggle',value:true});await flush();assert.equal(candidate.requests.filter(f=>f.type==='input').length,2);assert.equal(afterClose.some(f=>f.type==='input'),false);
  }finally{h.close();await flush();}
 }
});

test('focus returning before neutral acknowledgement waits before reopening and enabling controls',async()=>{
 const h=harness({hold:['input']});await flush();try{
  const socket=h.sockets[0];await h.focus(false);await h.focus(true);
  assert.equal(socket.requests.filter(f=>f.type==='open').length,1);assert.equal(h.handlers.queue.snapshot().enabled,false);
  h.handlers.emitOutput({controlId:'toggle',value:true});assert.equal(socket.requests.filter(f=>f.type==='input').length,1);
  socket.release('input');await flush();assert.equal(socket.requests.filter(f=>f.type==='open').length,2);assert.equal(h.handlers.queue.snapshot().enabled,true);assert.equal(socket.closed,false);
 }finally{h.close();await flush();}
});

test('hidden active companions retain their socket and feedback while inputs stay disabled',async()=>{
 const h=harness();await flush();try{
  const socket=h.sockets[0];await h.visibility('hidden');await h.tick();
  assert.equal(socket.closed,false);assert.equal(h.sockets.length,1);assert.equal(h.handlers.queue.snapshot().enabled,false);
  assert.ok(socket.requests.some(f=>f.type==='heartbeat'));assert.equal(h.handlers.feedback.confirmed.value,true);
  await h.visibility('visible');assert.equal(h.sockets.length,1);assert.equal(h.handlers.queue.snapshot().enabled,true);
 }finally{h.close();await flush();}
});

test('display-only panels keep polling after focus loss without a neutral input batch',async()=>{
 const h=harness({displayOnly:true});await flush();try{
  const socket=h.sockets[0];await h.focus(false);await h.tick();await h.focus(true);
  assert.equal(socket.closed,false);assert.equal(h.sockets.length,1);assert.equal(socket.requests.some(f=>f.type==='input'),false);
  assert.ok(socket.requests.some(f=>f.type==='heartbeat'));assert.equal(socket.requests.filter(f=>f.type==='baseline').length,2);assert.equal(h.handlers.queue.snapshot().enabled,true);
 }finally{h.close();await flush();}
});

test('an expired background session is reopened without dropping authenticated presence',async()=>{
 const h=harness();await flush();try{
  const socket=h.sockets[0],respond=socket.response.bind(socket);let expired=true;
  socket.response=frame=>{if(frame.type==='heartbeat'&&expired){expired=false;throw Error('Session is obsolete; explicitly reconnect.');}return respond(frame);};
  await h.focus(false);await h.tick();await flush();
  assert.equal(socket.closed,false);assert.equal(h.sockets.length,1);assert.equal(socket.requests.filter(f=>f.type==='auth').length,1);
  assert.equal(socket.requests.filter(f=>f.type==='baseline').length,2);assert.equal(h.handlers.queue.snapshot().enabled,false);
 }finally{h.close();await flush();}
});
test('an obsolete input session is reopened with a neutral baseline without replaying the failed input',async()=>{
 const h=harness();await flush();try{
  const socket=h.sockets[0],respond=socket.response.bind(socket);let expired=true;
  socket.response=frame=>{if(frame.type==='input'&&expired){expired=false;throw Error('Session is obsolete; explicitly reconnect.');}return respond(frame);};
  h.handlers.emitOutput({controlId:'toggle',value:true});await flush();await flush();
  assert.equal(socket.closed,false);assert.equal(h.sockets.length,1);assert.equal(socket.requests.filter(f=>f.type==='auth').length,1);
  assert.equal(socket.requests.filter(f=>f.type==='baseline').length,2);
  assert.equal(socket.requests.filter(f=>f.type==='input').length,1,'Only the failed input request was sent; it was not replayed.');
  assert.equal(h.handlers.active,true);assert.equal(h.handlers.queue.snapshot().enabled,true);
  assert.equal(socket.requests.at(-1).type,'baseline');
 }finally{h.close();await flush();}
});
test('an obsolete feedback resync reopens the panel session without surfacing a reconnect error',async()=>{
 const h=harness();await flush();try{
  const socket=h.sockets[0],respond=socket.response.bind(socket);let expired=true;
  socket.response=frame=>{if(frame.type==='feedback-resync'&&expired){expired=false;throw Error('Session is obsolete; explicitly reconnect.');}return respond(frame);};
  await h.handlers.resyncFeedback();await flush();await flush();
  assert.equal(socket.closed,false);assert.equal(h.sockets.length,1);assert.equal(socket.requests.filter(f=>f.type==='auth').length,1);
  assert.equal(socket.requests.filter(f=>f.type==='baseline').length,2);assert.equal(h.handlers.active,true);
 }finally{h.close();await flush();}
});
test('an obsolete initial baseline retries on the authenticated socket without exposing the protocol error',async()=>{
 const h=harness({staleOnce:['baseline']});await flush();await flush();try{
  const socket=h.sockets[0];assert.equal(socket.closed,false,JSON.stringify({status:h.handlers.status,error:h.handlers.error,types:socket.requests.map(f=>f.type)}));assert.equal(h.sockets.length,1);
  assert.equal(socket.requests.filter(f=>f.type==='auth').length,1);assert.equal(socket.requests.filter(f=>f.type==='baseline').length,2);
  assert.equal(h.handlers.active,true);assertObsoleteHidden(h);
 }finally{h.close();await flush();}
});
test('an obsolete neutral input after blur reopens with controls paused and no stale error text',async()=>{
 const h=harness({staleOnce:['input']});await flush();try{
  await h.focus(false);await flush();await flush();
  const socket=h.sockets[0];assert.equal(socket.closed,false,JSON.stringify({status:h.handlers.status,error:h.handlers.error,types:socket.requests.map(f=>f.type)}));assert.equal(h.sockets.length,1);
  assert.equal(socket.requests.filter(f=>f.type==='input').length,1);assert.equal(socket.requests.filter(f=>f.type==='baseline').length,2);
  assert.equal(h.handlers.active,true);assert.equal(h.handlers.queue.snapshot().enabled,false);assertObsoleteHidden(h);
 }finally{h.close();await flush();}
});
test('a pending presence poll cannot start a second load during stale-session recovery',async()=>{
 const h=harness({hold:['presence'],staleOnce:['input']});await flush();try{
  const socket=h.sockets[0];await h.tick();assert.equal(socket.pending.filter(p=>p.frame.type==='presence').length,1);
  h.handlers.emitOutput({controlId:'toggle',value:true});await flush();
  assert.equal(socket.pending.filter(p=>p.frame.type==='presence').length,2);
  socket.release('presence');await flush();assert.equal(socket.requests.filter(f=>f.type==='open').length,1,'The older poll must not race recovery into a parallel load.');
  socket.release('presence');await flush();await flush();
  assert.equal(socket.requests.filter(f=>f.type==='open').length,2);assert.equal(socket.requests.filter(f=>f.type==='baseline').length,2);
  assert.equal(h.handlers.active,true);assertObsoleteHidden(h);
 }finally{h.close();await flush();}
});
test('an obsolete open response with no local session reconnects automatically without surfacing it',async()=>{
 const h=harness({staleOnce:['open']});await flush();try{
  assert.equal(h.sockets.length,1);assert.equal(h.handlers.session,null);assertObsoleteHidden(h);
  await h.tick();assert.equal(h.sockets.length,2);assert.equal(h.handlers.active,true);assertObsoleteHidden(h);
 }finally{h.close();await flush();}
});
test('prepared pixels from an unloaded page are disposed without committing or baselining',async()=>{
 const h=harness({holdPreparation:true});await flush();try{await h.pagehide();h.finishPreparation();await flush();assert.equal(h.handlers.panel,null);assert.equal(h.handlers.active,false);assert.equal(h.sockets[0].requests.some(f=>f.type==='baseline'),false);assert.equal(h.preparedSurfaces[0].retained.disposed,true,'A stale preparation result must release its candidate surfaces.');}finally{h.close();await flush();}
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
