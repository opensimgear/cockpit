import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import * as designer from '../../../packages/companion-designer/src/index.ts';
import * as controls from '../../../packages/companion-ui/src/controller.ts';
import {createOrderedInputQueue} from '../../companion/src/lib/input-queue.ts';
import * as feedbackApi from './feedback.ts';
import * as credentials from './transport.ts';

const appSource=await readFile(new URL('./App.svelte',import.meta.url),'utf8');
const panel=structuredClone(designer.flightTemplate);
const hash='sha256:'+'a'.repeat(64);
const assignment={instanceId:'instance',publicationHash:hash,name:'Flight'};
const saved={token:'a'.repeat(43),hostId:'host',companionId:'browser'};
const flush=async()=>{for(let i=0;i<20;i++)await Promise.resolve();};

/** Execute the actual App and its mounted browser listeners, with no DOM input,
 * network or native service. Controller/queue/credential/feedback code is real. */
function harness({pair=false,hold=[],rejectPanel=false}={}){
  let handlers,mount,cleanup,focused=true,clock=100;
  const sockets=[],intervals=new Map(),storage=new Map();
  const window=new EventTarget(),document=new EventTarget();
  Object.assign(document,{visibilityState:'visible',hasFocus:()=>focused});
  const location={hash:pair?'#pair='+'x'.repeat(43):''};
  const localStorage={getItem:key=>storage.get(key)??null,setItem:(key,value)=>storage.set(key,value),removeItem:key=>storage.delete(key)};
  if(!pair)credentials.saveCredential(localStorage,saved);
  class Socket {
    closed=false;requests=[];pending=[];
    constructor(){this.index=sockets.length+1;sockets.push(this);}
    async connect(){}
    response(frame){
      if(frame.type==='auth')return{...saved,assignments:[assignment]};
      if(frame.type==='pair')return{requestId:'offer'};
      if(frame.type==='pair-status')return{type:'paired',...saved};
      if(frame.type==='presence')return{assignments:[assignment]};
      if(frame.type==='open')return{clientId:'browser',sessionId:'session-'+this.index,generation:String(this.index),controllerInstanceId:'instance',publicationHash:hash,publication:{canonicalDocument:JSON.stringify(panel)}};
      if(frame.type==='baseline'||frame.type==='heartbeat'){
        const output=panel.ui.feedbackWidgets.find(output=>output.kind==='text');
        return{ok:true,type:frame.type==='baseline'?'baseline':'ack',feedback:{version:1,type:'feedback',clientId:'browser',sessionId:'session-'+this.index,generation:String(this.index),controllerInstanceId:'instance',publicationHash:hash,sequence:frame.type==='baseline'?'1':'2',bindingGeneration:'1',mode:'snapshot',bindings:[{channelId:output.channelId,outputId:output.id}],values:[{channelId:output.channelId,outputId:output.id,status:'fresh',value:'Background altitude',provenance:'requested',sourceKind:'host',sourceAgeMs:10,unit:'text'}]}};
      }
      return{ok:true,type:'ack'};
    }
    deliver(frame){const reply=this.response(frame);this.onreply?.(reply);return reply;}
    request(frame){
      if(this.closed)return Promise.reject(Error('Socket closed'));
      this.requests.push(structuredClone(frame));
      if(hold.includes(frame.type))return new Promise((resolve,reject)=>this.pending.push({frame,resolve,reject}));
      return Promise.resolve(this.deliver(frame));
    }
    release(type){const index=this.pending.findIndex(item=>item.frame.type===type);assert.ok(index>=0,'The intended operation is pending');const[item]=this.pending.splice(index,1);item.resolve(this.deliver(item.frame));}
    close(){this.closed=true;for(const item of this.pending.splice(0))item.reject(Error('Socket closed'));this.onclose?.();}
  }
  const injected=appSource.replace('</script>',`capture({poll,connect,pair,controller,queue,get epoch(){return epoch;},get active(){return active;},get panelError(){return panelError;},get session(){return session;},get status(){return status;},get feedback(){return feedback;},get credential(){return credential;},get secret(){return secret;}});</script>`);
  const{js}=compile(injected,{filename:'App.svelte',generate:'server'}),exports={};
  vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{
    exports,structuredClone,TextEncoder,URLSearchParams,performance:{now:()=>clock},crypto,
    window,document,location,history:{replaceState(){location.hash='';}},navigator:{},localStorage,
    setInterval(callback){const id=intervals.size+1;intervals.set(id,callback);return id;},clearInterval:id=>intervals.delete(id),
    capture:value=>handlers=value,
    require(name){
      if(name==='svelte/internal/server')return server;
      if(name==='svelte')return{...svelte,onMount:callback=>mount=callback};
      if(name==='@opensimgear/companion-ui')return{...controls,PanelCanvas(){}};
      if(name==='@opensimgear/companion-designer')return designer;
      if(name==='../../companion/src/lib/companion-cache')return{verifyPublishedPayload:async()=>{if(rejectPanel)throw Error('Unsupported widget');return {document:panel,assets:[],publicationHash:hash};}};
      if(name==='../../companion/src/lib/input-queue')return{createOrderedInputQueue};
      if(name==='./transport')return{...credentials,BrowserTransport:Socket};
      if(name==='./feedback')return feedbackApi;
      throw Error(name);
    },
  });
  render(exports.default).body;cleanup=mount();
  return{handlers,sockets,storage,
    async focus(value){focused=value;window.dispatchEvent(new Event(value?'focus':'blur'));await flush();},
    async visibility(value){document.visibilityState=value;document.dispatchEvent(new Event('visibilitychange'));await flush();},
    async pagehide(){window.dispatchEvent(new Event('pagehide'));await flush();},
    async pageshow(){window.dispatchEvent(new Event('pageshow'));await flush();},
    async tick(){for(const callback of intervals.values())callback();await flush();},
    close(){cleanup();},advance(ms){clock+=ms;},
  };
}

test('blur cancels old queued input, then opens a new neutral feedback-only session; focus baselines before rearming',async()=>{
  const h=harness({hold:['input']});await flush();
  try{
    assert.equal(h.handlers.controller.snapshot().armed,true);
    const widget=panel.presentation.widgets.find(item=>item.kind==='button'&&item.config.buttonMode==='momentary');
    assert.ok(widget);assert.equal(h.handlers.controller.begin(1,widget.id),true);
    h.handlers.controller.release(1);await flush();
    const old=h.sockets[0];assert.equal(old.requests.filter(frame=>frame.type==='input').length,1);
    assert.equal(h.handlers.queue.snapshot().pending,1,'The release is queued behind an actual pending press');
    await h.focus(false);
    assert.equal(old.closed,true);assert.equal(old.pending.length,0);
    assert.equal(old.requests.filter(frame=>frame.type==='input').length,1,'Old queued release cannot leak onto the closed socket');
    assert.equal(h.sockets.length,2);const background=h.sockets[1];
    const baseline=background.requests.find(frame=>frame.type==='baseline');assert.ok(baseline);
    assert.deepEqual(baseline.values,Object.fromEntries(panel.ui.controls.map(control=>[control.id,controls.fallbackValue(control)])));
    assert.equal(h.handlers.controller.snapshot().armed,false);assert.equal(h.handlers.controller.snapshot().activePointers,0);
    assert.equal(h.handlers.queue.snapshot().enabled,false);assert.equal(h.handlers.controller.begin(2,widget.id),false);
    assert.equal(h.handlers.status,'Connected · feedback active · controls paused');
    await h.tick();assert.ok(background.requests.some(frame=>frame.type==='heartbeat'));
    assert.ok(Object.values(h.handlers.feedback).some(sample=>sample.value==='Background altitude'));
    assert.equal(background.requests.some(frame=>frame.type==='input'),false);
    await h.focus(true);
    assert.equal(background.closed,true);assert.equal(h.sockets.length,3);
    const next=h.sockets[2];assert.ok(next.requests.some(frame=>frame.type==='baseline'));
    assert.equal(h.handlers.controller.snapshot().armed,true);assert.equal(h.handlers.queue.snapshot().enabled,true);
    assert.equal(h.handlers.controller.begin(3,widget.id),true);await flush();
    const baselineIndex=next.requests.findIndex(frame=>frame.type==='baseline'),inputIndex=next.requests.findIndex(frame=>frame.type==='input');
    assert.ok(inputIndex>baselineIndex);assert.equal(next.requests[inputIndex].sequence,'1');
    assert.notEqual(next.requests[inputIndex].sessionId,old.requests.find(frame=>frame.type==='input').sessionId);
  }finally{h.close();await flush();}
});

test('visible blur does not cancel pairing approval or nonproductive authentication',async()=>{
  for(const pair of [true,false]){
    const kind=pair?'pair-status':'auth',h=harness({pair,hold:[kind]});await flush();
    try{
      const socket=h.sockets[0];assert.equal(socket.pending[0]?.frame.type,kind);
      await h.focus(false);assert.equal(socket.closed,false);assert.equal(h.sockets.length,1);
      socket.release(kind);await flush();
      if(pair){assert.equal(socket.requests.filter(frame=>frame.type==='pair').length,1);assert.ok(h.handlers.credential);assert.ok(h.storage.has('osg-browser-credential-v2'));}
      const current=h.sockets.at(-1);assert.ok(current.requests.some(frame=>frame.type==='baseline'));
      assert.equal(h.handlers.active,true);assert.equal(h.handlers.controller.snapshot().armed,false);
      assert.equal(h.handlers.queue.snapshot().enabled,false);assert.equal(current.requests.some(frame=>frame.type==='input'),false);
    }finally{h.close();await flush();}
  }
});

test('hidden and unloaded pages cancel pending callbacks and cannot reconnect until visible restoration',async()=>{
  const h=harness({hold:['auth']});await flush();
  try{
    const old=h.sockets[0];assert.equal(old.pending[0]?.frame.type,'auth');
    await h.visibility('hidden');assert.equal(old.closed,true);
    await h.tick();await h.handlers.connect();assert.equal(h.sockets.length,1);
    assert.equal(h.handlers.active,false);assert.equal(h.handlers.session,null);
    await h.visibility('visible');assert.equal(h.sockets.length,2);
    h.sockets[1].release('auth');await flush();assert.equal(h.handlers.active,true);
    await h.pagehide();assert.equal(h.sockets[1].closed,true);
    await h.tick();await h.handlers.connect();assert.equal(h.sockets.length,2,'pagehide suppresses even a still-visible document');
    await h.pageshow();assert.equal(h.sockets.length,3);h.sockets[2].release('auth');await flush();
    assert.ok(h.sockets[2].requests.some(frame=>frame.type==='baseline'));assert.equal(h.handlers.controller.snapshot().armed,true);
    h.close();await h.tick();await h.handlers.connect();assert.equal(h.sockets.length,3);
  }finally{h.close();await flush();}
});

test('hiding during pairing approval cannot save or replay the consumed offer',async()=>{
  const h=harness({pair:true,hold:['pair-status']});await flush();
  try{
    const old=h.sockets[0];assert.equal(old.pending[0]?.frame.type,'pair-status');
    await h.visibility('hidden');assert.equal(old.closed,true);
    assert.equal(h.handlers.credential,null);assert.equal(h.handlers.secret,'');
    assert.equal(h.storage.has('osg-browser-credential-v2'),false);
    await h.visibility('visible');await h.tick();
    assert.equal(h.sockets.length,1);assert.equal(old.requests.filter(frame=>frame.type==='pair').length,1);
    assert.equal(h.handlers.controller.snapshot().armed,false);
  }finally{h.close();await flush();}
});

test('a rejected panel stays connected and disarmed with panel-error heartbeats',async()=>{
  const h=harness({rejectPanel:true});await flush();
  try{
    assert.equal(h.handlers.panelError,true);assert.equal(h.handlers.active,false);assert.ok(h.handlers.session);
    assert.equal(h.handlers.status,'Connected · panel error');assert.equal(h.sockets[0].closed,false);
    assert.ok(!h.sockets[0].requests.some(r=>r.type==='baseline'||r.type==='input'));
    await h.tick();assert.ok(h.sockets[0].requests.some(r=>r.type==='heartbeat'&&r.panelError===true));
    assert.equal(h.handlers.controller.snapshot().armed,false);
  }finally{h.close();await flush();}
});
