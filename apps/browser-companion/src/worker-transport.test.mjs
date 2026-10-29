import test from 'node:test';
import assert from 'node:assert/strict';
import { BrowserTransport } from './transport.ts';

function workerHarness(){
 const previous=globalThis.SharedWorker,instances=[];
 class Worker{
  constructor(url,options){assert.ok(url.href.endsWith('/connection-worker.ts'));assert.deepEqual(options,{type:'module'});instances.push(this);this.sent=[];this.port={postMessage:message=>this.sent.push(message),start:()=>{},close:()=>this.portClosed=true};}
  deliver(data){this.port.onmessage({data});}
 }
 globalThis.SharedWorker=Worker;
 return{instances,restore(){globalThis.SharedWorker=previous;}};
}
function socketHarness(){
 const previousSocket=globalThis.WebSocket,previousLocation=globalThis.location,instances=[];
 class Socket{
  static OPEN=1;readyState=0;
  constructor(url,protocol){assert.equal(url,'wss://10.0.0.2:54437/socket');assert.equal(protocol,'opensimgear-companion-v1');instances.push(this);this.sent=[];queueMicrotask(()=>{if(this.readyState===0){this.readyState=1;this.onopen?.();}});}
  send(raw){this.sent.push(JSON.parse(raw));}
  close(){if(this.readyState===3)return;this.readyState=3;this.onclose?.();}
  reply(value){this.onmessage({data:JSON.stringify(value)});}
 }
 globalThis.WebSocket=Socket;globalThis.location={origin:'https://10.0.0.2:54437'};
 return{instances,restore(){globalThis.WebSocket=previousSocket;globalThis.location=previousLocation;}};
}
function startupWatchdog(){
 const original=globalThis.setTimeout;let callback,cleared=false;
 globalThis.setTimeout=(fn,delay,...args)=>{
  if(delay===4000){callback=()=>fn(...args);return 8675309;}
  return original(fn,delay,...args);
 };
 const originalClear=globalThis.clearTimeout;
 globalThis.clearTimeout=id=>{if(id===8675309)cleared=true;else originalClear(id);};
 return{fire(){assert.ok(callback,'startup watchdog was installed');callback();},cleared:()=>cleared,restore(){globalThis.setTimeout=original;globalThis.clearTimeout=originalClear;}};
}

test('shared-worker transport correlates requests and forwards autonomous feedback before resolving page replies',async()=>{
 const h=workerHarness(),transport=new BrowserTransport(),received=[];
 try{
  transport.onreply=reply=>received.push(reply.feedback);const connected=transport.connect(),worker=h.instances[0];assert.deepEqual(worker.sent,[{type:'connect'}]);worker.deliver({type:'connected'});await connected;
  const request=transport.request({type:'input',sequence:'1'}),frame=worker.sent.at(-1);assert.equal(frame.type,'request');
  worker.deliver({type:'feedback',id:12,reply:{feedback:'worker heartbeat'}});assert.deepEqual(worker.sent.at(-1),{type:'feedback-ack',id:12});worker.deliver({type:'result',id:frame.id,reply:{feedback:'page request'}});await request;
  assert.deepEqual(received,['worker heartbeat','page request']);
  const failed=transport.request({type:'input'});worker.deliver({type:'result',id:worker.sent.at(-1).id,error:'Session is obsolete'});await assert.rejects(failed,/obsolete/);assert.equal(worker.portClosed,undefined);
 }finally{transport.close();h.restore();}
});

test('worker disconnect rejects queued input, closes its port, and notifies the page once without replay',async()=>{
 const h=workerHarness(),transport=new BrowserTransport();let closes=0;
 try{
  transport.onclose=()=>closes++;const connected=transport.connect(),worker=h.instances[0];worker.deliver({type:'connected'});await connected;
  const pending=transport.request({type:'input',changes:[{controlId:'button',value:true}]}),rejected=assert.rejects(pending,/closed/);
  worker.deliver({type:'closed'});await rejected;worker.onerror();assert.equal(closes,1);assert.equal(worker.portClosed,true);
  assert.equal(worker.sent.filter(message=>message.type==='request').length,1);assert.deepEqual(worker.sent.at(-1),{type:'close'});
  await assert.rejects(transport.request({type:'input'}),/stopped/);
 }finally{transport.close();h.restore();}
});

test('invalid autonomous feedback tears down the worker instead of renewing a productive session',async()=>{
 const h=workerHarness(),transport=new BrowserTransport();let closes=0;
 try{
  transport.onclose=()=>closes++;transport.onreply=()=>{throw Error('Invalid feedback identity');};const connected=transport.connect(),worker=h.instances[0];worker.deliver({type:'connected'});await connected;
  worker.deliver({type:'feedback',reply:{feedback:'invalid'}});assert.equal(closes,1);assert.equal(worker.portClosed,true);assert.deepEqual(worker.sent.at(-1),{type:'close'});
 }finally{transport.close();h.restore();}
});

test('a worker startup that hangs falls back to direct transport and ignores stale worker events',async()=>{
 const h=workerHarness(),s=socketHarness(),watchdog=startupWatchdog(),transport=new BrowserTransport();let closes=0;
 try{
  transport.onclose=()=>closes++;
  const connected=transport.connect(),worker=h.instances[0],staleError=worker.onerror;
  assert.equal(s.instances.length,0);watchdog.fire();await connected;
  assert.deepEqual(worker.sent,[{type:'connect'},{type:'close'}]);assert.equal(worker.portClosed,true);assert.equal(s.instances.length,1);
  worker.deliver({type:'connected'});worker.deliver({type:'closed'});staleError?.({});
  assert.equal(closes,0);assert.equal(s.instances[0].readyState,1);
  const request=transport.request({type:'auth'});await new Promise(resolve=>setImmediate(resolve));assert.deepEqual(s.instances[0].sent,[{type:'auth'}]);s.instances[0].reply({ok:true,type:'auth'});await request;
 }finally{transport.close();watchdog.restore();s.restore();h.restore();}
});

test('a worker startup error falls back, while a post-connect worker error never replays over direct transport',async()=>{
 const h=workerHarness(),s=socketHarness(),watchdog=startupWatchdog(),transport=new BrowserTransport();let closes=0;
 try{
  transport.onclose=()=>closes++;
  const connected=transport.connect(),worker=h.instances[0];worker.onerror?.({});await connected;
  assert.equal(s.instances.length,1);assert.equal(worker.portClosed,true);assert.equal(closes,0);
  worker.deliver({type:'connected'});assert.equal(closes,0);
 }finally{transport.close();watchdog.restore();s.restore();h.restore();}

 const h2=workerHarness(),s2=socketHarness(),transport2=new BrowserTransport();let closes2=0;
 try{
  transport2.onclose=()=>closes2++;
  const connected=transport2.connect(),worker=h2.instances[0];worker.deliver({type:'connected'});await connected;
  const input=transport2.request({type:'input',sequence:'1'});const failed=assert.rejects(input,/Background Cockpit connection failed/);
  assert.equal(worker.sent.at(-1).type,'request');worker.onerror?.({});await failed;
  assert.equal(closes2,1);assert.equal(s2.instances.length,0);assert.equal(worker.sent.filter(message=>message.type==='request').length,1);
 }finally{transport2.close();s2.restore();h2.restore();}
});

test('closing during worker startup clears the watchdog and prevents direct fallback',async()=>{
 const h=workerHarness(),s=socketHarness(),watchdog=startupWatchdog(),transport=new BrowserTransport();
 try{
  const connected=transport.connect(),worker=h.instances[0];transport.close();await assert.rejects(connected,/stopped/);
  assert.equal(watchdog.cleared(),true);assert.equal(worker.portClosed,true);watchdog.fire();
  assert.equal(s.instances.length,0);
 }finally{transport.close();watchdog.restore();s.restore();h.restore();}
});
