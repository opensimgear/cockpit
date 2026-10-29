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
