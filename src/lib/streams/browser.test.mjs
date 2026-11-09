import test from 'node:test';
import assert from 'node:assert/strict';
import { createBrowserObservationTransport } from './browser.ts';
import { SharedObservations } from './shared.ts';
import { MAX_LINE_BYTES } from './protocol.ts';
import { delivery, deferred, turn } from './fixtures.mjs';

const encode = value => new TextEncoder().encode(value);
const json = value => new Response(JSON.stringify(value), { headers: { 'content-type': 'application/json' } });
test('default browser factories share a context transport; injected dependencies are isolated',()=>{
  assert.equal(createBrowserObservationTransport(),createBrowserObservationTransport());
  assert.notEqual(createBrowserObservationTransport({}),createBrowserObservationTransport({}));
});
function harness({subscribeWait, ackWait, unsubscribeWait, initial, responseMode, ackStatus=200}={}) {
  const calls=[],physical=[],errors=[];let serial=0;
  const transport=createBrowserObservationTransport({connectionId:()=>`00000000-0000-4000-8000-${String(++serial).padStart(12,'0')}`,fetch:async(url,init)=>{
    const body=JSON.parse(init.body);calls.push({url,init,body});
    if(url==='/api/observe/') {
      if(responseMode==='http')return new Response(null,{status:500});
      if(responseMode==='body')return new Response(null,{headers:{'content-type':'application/x-ndjson'}});
      const record={id:body.connectionId,cancelled:false};physical.push(record);
      const stream=new ReadableStream({start(controller){record.controller=controller;
        if(initial)initial(controller,body.connectionId);
        else controller.enqueue(encode(`${JSON.stringify({type:'ready',connectionId:body.connectionId})}\n`));
      },cancel(){record.cancelled=true;}});
      return new Response(stream,{headers:{'content-type':responseMode==='type'?'application/json':'application/x-ndjson; charset=utf-8'}});
    }
    if(url.endsWith('/subscribe')) {
      if(subscribeWait)await subscribeWait(body);
      return json({ok:true,subscriptionId:`token-${body.requestId}`});
    }
    if(url.endsWith('/ack')) {if(ackWait)await ackWait(body);return new Response(null,{status:ackStatus});}
    if(url.endsWith('/unsubscribe')){if(unsubscribeWait)await unsubscribeWait;return json({ok:true});}
    throw Error('unexpected request '+url);
  }});
  function send(requestId,overrides={},deliveryId='1') {
    const sub=calls.find(call=>call.url.endsWith('/subscribe')&&call.body.requestId===requestId);
    const envelope={...delivery({topic:sub.body.topic,...overrides},deliveryId),subscriptionId:`token-${requestId}`};
    physical.at(-1).controller.enqueue(encode(`${JSON.stringify({requestId,envelope})}\n`));
  }
  function open(topic='preferences') {
    const controller=new AbortController(),frames=[];
    transport.open(topic,{frame(envelope,ack){frames.push(envelope);void ack();},error:value=>errors.push(value)},controller.signal);
    return {controller,frames};
  }
  return {transport,calls,physical,errors,send,open};
}

test('six topics share one physical connection across independent shared clients',async()=>{
  const h=harness(),first=new SharedObservations(h.transport),second=new SharedObservations(h.transport),states=[];
  const topics=['theme','devices-mode','canvas-navigation','canvas-styling','tray','progress'];
  const stops=topics.map((key,index)=>(index%2?first:second).subscribe(`preferences.${key}`,state=>states.push(state)));
  await turn();assert.equal(h.physical.length,1);assert.equal(h.calls.filter(call=>call.url.endsWith('/subscribe')).length,6);
  for(let index=1;index<=6;index++)h.send(String(index));await turn();
  assert.equal(states.filter(state=>state.status==='ready').length,6);
  assert.equal(h.calls.filter(call=>call.url.endsWith('/ack')).length,6);
  assert.equal(h.calls[0].init.headers['x-osg-client'],'1');assert.deepEqual(Object.keys(h.calls[0].body),['connectionId']);
  stops[0]();assert.equal(h.physical[0].cancelled,false);for(const stop of stops.slice(1))stop();await turn();
  assert.equal(h.physical[0].cancelled,true);assert.equal(h.calls.filter(call=>call.url.endsWith('/unsubscribe')).length,6);
  first.close();second.close();
});

test('one slow ACK never blocks independent topic delivery; one early ACK-race frame buffers',async()=>{
  const slow=deferred();const h=harness({ackWait:body=>body.subscriptionId==='token-1'&&body.delivery==='1'?slow.promise:undefined});
  const first=h.open('first'),second=h.open('second');await turn();h.send('1');h.send('1',{revision:'2'},'2');h.send('2');await turn();
  assert.equal(first.frames.length,1);assert.equal(second.frames.length,1);
  assert.equal(h.calls.filter(call=>call.url.endsWith('/ack')).length,2);assert.deepEqual(h.errors,[]);
  slow.resolve();await turn();assert.equal(first.frames.length,2);assert.deepEqual(h.errors,[]);
  first.controller.abort();second.controller.abort();
});

test('an application context admits 56 readers and reuses released capacity',async()=>{
  const h=harness();
  const readers=Array.from({length:56},(_,index)=>h.open(`topic-${index}`));
  h.open('overflow');
  await turn();
  const subscriptions=()=>h.calls.filter(call=>call.url.endsWith('/subscribe'));
  assert.equal(subscriptions().length,56);
  assert.deepEqual(h.errors,['Observation subscription limit exceeded']);
  readers[0].controller.abort();
  const replacement=h.open('replacement');
  await turn();
  assert.equal(subscriptions().at(-1).body.topic,'replacement');
  assert.equal(h.physical.length,1);
  h.send('57');
  await turn();
  assert.equal(replacement.frames.length,1);
  h.physical[0].controller.close();
  await turn();
});

test('early delivery waits for token reply; pending-open cancellation uses request counter',async()=>{
  const opening=deferred();const h=harness({subscribeWait:body=>body.requestId==='1'?opening.promise:undefined});
  const first=h.open('first'),second=h.open('second');await turn();h.send('1');h.send('2');await turn();
  assert.equal(first.frames.length,0);assert.equal(second.frames.length,1);
  first.controller.abort();await turn();
  assert.equal(h.calls.find(call=>call.url.endsWith('/subscribe')&&call.body.requestId==='1').init.signal.aborted,true);
  assert.deepEqual(h.calls.find(call=>call.url.endsWith('/unsubscribe')).body,{connectionId:h.physical[0].id,requestId:'1'});
  opening.resolve();await turn();h.send('1',{revision:'2'},'2');await turn();
  assert.equal(first.frames.length,0);assert.deepEqual(h.errors,[]);second.controller.abort();
});

test('EOF faults all entries once and a new explicit subscribe opens a fresh connection/counter',async()=>{
  const h=harness();h.open('first');h.open('second');await turn();h.send('1');await turn();
  h.physical[0].controller.close();await turn();assert.equal(h.errors.length,2);assert.ok(h.errors.every(error=>/ended/.test(error)));
  const fresh=h.open('fresh');await turn();assert.equal(h.physical.length,2);
  assert.equal(h.calls.filter(call=>call.url.endsWith('/subscribe')).at(-1).body.requestId,'3');
  h.send('3',{session:'new-service'});await turn();assert.equal(fresh.frames[0].frame.session,'new-service');fresh.controller.abort();
});

test('UTF-8 chunk boundaries preserve characters and oversized/truncated/invalid records close source',async()=>{
  const h=harness();const sub=h.open();await turn();
  const envelope={...delivery({payload:'é'}),subscriptionId:'token-1'};
  const bytes=encode(`${JSON.stringify({requestId:'1',envelope})}\n`),split=bytes.indexOf(195)+1;
  h.physical[0].controller.enqueue(bytes.subarray(0,split));h.physical[0].controller.enqueue(bytes.subarray(split));await turn();
  assert.equal(sub.frames[0].frame.payload,'é');sub.controller.abort();
  for(const mode of ['oversize','truncated','json','utf8']) {
    const bad=harness({initial(controller){
      if(mode==='oversize')controller.enqueue(new Uint8Array(MAX_LINE_BYTES+1).fill(32));
      else if(mode==='truncated'){controller.enqueue(encode('{}'));controller.close();}
      else if(mode==='json')controller.enqueue(encode('bad\n'));
      else controller.enqueue(new Uint8Array([255,10]));
    }});bad.open();await turn();assert.equal(bad.errors.length,1,mode);
    if(mode!=='truncated')assert.equal(bad.physical[0].cancelled,true,mode);
  }
});

test('readiness rejects duplicate/mismatched ready and unknown future request IDs',async()=>{
  for(const mode of ['mismatch','duplicate','future','zero','numeric']){
    const h=harness({initial(controller,id){controller.enqueue(encode(`${JSON.stringify({type:'ready',connectionId:mode==='mismatch'?'wrong':id})}\n`));}});
    h.open();await turn();
    if(mode==='duplicate')h.physical[0].controller.enqueue(encode(`${JSON.stringify({type:'ready',connectionId:h.physical[0].id})}\n`));
    if(['future','zero','numeric'].includes(mode))h.physical[0].controller.enqueue(encode(`${JSON.stringify({requestId:mode==='future'?'999':mode==='zero'?'0':1,envelope:delivery()})}\n`));
    await turn();assert.equal(h.errors.length,1,mode);
  }
});

test('per-entry ACK failure/token mismatch/gap/early overflow do not close healthy readers',async()=>{
  for(const mode of ['ack','token','gap','overflow']){
    const wait=deferred();const h=harness({ackStatus:mode==='ack'?400:200,subscribeWait:body=>mode==='overflow'&&body.requestId==='1'?wait.promise:undefined});
    h.open('first');const second=h.open('second');await turn();
    if(mode==='token')h.physical[0].controller.enqueue(encode(`${JSON.stringify({requestId:'1',envelope:{...delivery({topic:'first'}),subscriptionId:'wrong'}})}\n`));
    else {h.send('1');if(mode==='overflow')h.send('1',{},'2');}
    await turn();if(mode==='gap')h.send('1',{},'3');await turn();
    assert.equal(h.errors.length,1,mode);assert.equal(h.physical[0].cancelled,false,mode);
    wait.resolve();second.controller.abort();
  }
});

test('connection HTTP/type/body errors are explicit',async()=>{
  for(const responseMode of ['http','type','body']){const h=harness({responseMode});h.open();await turn();assert.equal(h.errors.length,1,responseMode);}
});

test('already aborted entries perform no work, last disposal aborts pending physical open',async()=>{
  const controller=new AbortController();controller.abort();createBrowserObservationTransport({fetch:assert.fail}).open('preferences',{frame:assert.fail,error:assert.fail},controller.signal);
  const opening=deferred(),calls=[];let cancelled=false;
  const transport=createBrowserObservationTransport({fetch:async(url,args)=>{calls.push([url,args]);await opening.promise;return new Response(new ReadableStream({cancel(){cancelled=true;}}));}});
  const pending=new AbortController();transport.open('preferences',{frame:assert.fail,error:assert.fail},pending.signal);pending.abort();opening.resolve();await turn();
  assert.equal(calls[0][1].signal.aborted,true);assert.equal(cancelled,true);
});

test('stalled cancellation controls remain bounded and fault the physical connection at the budget',async()=>{
  const stalled=deferred(),h=harness({unsubscribeWait:stalled.promise});h.open('keeper');await turn();
  for(let index=0;index<17;index++){const entry=h.open(`transient${index}`);await turn();entry.controller.abort();}
  await turn();assert.equal(h.calls.filter(call=>call.url.endsWith('/unsubscribe')).length,16);
  assert.equal(h.errors.length,1);assert.match(h.errors[0],/cancellation budget/);assert.equal(h.physical[0].cancelled,true);
  assert.ok(h.calls.filter(call=>call.url.endsWith('/unsubscribe')).every(call=>call.init.signal.aborted));
  stalled.resolve();await turn();
});
