import test from 'node:test';
import assert from 'node:assert/strict';
import {debugControllerError,debugControllerLayout,debugControllerPorts,debugControllerResource,DebugControllerClient,createDebugControllerClient} from './profile-debug-controller.ts';
const owner=()=>({session:'11111111-1111-4111-8111-111111111111',generation:2,profileId:'22222222-2222-4222-8222-222222222222',profileRevision:4,gameId:null});
const plain=value=>JSON.parse(JSON.stringify(value));
const tick=()=>new Promise(resolve=>setImmediate(resolve));
const button=value=>({portId:'button-1',value});
const axis=(value,id=1)=>({portId:`axis-${id}`,value});
const status=(values=[],extra={})=>({debugController:{owner:owner(),nodeId:'debug-1',values,...extra}});
async function harness(values=[]){
  const calls=[],pending=[],errors=[];
  const client=new DebugControllerClient(request=>{calls.push(plain(request));return request.command.endsWith('_status')?Promise.resolve(status(values,{streamId:null,sequence:null,replayed:false,pendingFrames:0})):new Promise((resolve,reject)=>pending.push({resolve,reject}));},'debug-1',error=>errors.push(error));
  await client.status(owner());
  return{client,calls,pending,errors,inputs:()=>calls.filter(call=>call.command.endsWith('_input')),settle:async()=>{pending.shift()?.resolve({});await tick();}};
}
test('resource defaults and canonical native domains',()=>{
  const resource=debugControllerResource('debug-1');assert.equal(debugControllerError(resource),'');assert.equal(resource.required,false);
  assert.deepEqual(debugControllerLayout(resource).axes,[{portId:'axis-1',range:'bipolar',fallback:0},{portId:'axis-2',range:'bipolar',fallback:0}]);
  assert.equal(debugControllerPorts({buttons:[],axes:[{portId:'axis-16',range:'unipolar',fallback:0}],hats:['hat-4']})[0].contract.schema.min,0);
  for(const layout of [{buttons:[],axes:[],hats:[]},{buttons:['button-129'],axes:[],hats:[]},{buttons:[],axes:[{portId:'axis-17',range:'bipolar',fallback:0}],hats:[]},{buttons:[],axes:[],hats:['hat-5']},{buttons:[],axes:[{portId:'axis-1',range:'unipolar',fallback:123}],hats:[]},{buttons:['button-01'],axes:[],hats:[]}])assert.throws(()=>debugControllerPorts(layout));
  assert.throws(()=>debugControllerResource('1-bad'));assert.throws(()=>debugControllerResource('debug',' Bad '));
});
test('native property/port ordering, custom labels and valid freshness survive validation',()=>{
  const sorted=value=>Array.isArray(value)?value.map(sorted):value&&typeof value==='object'?Object.fromEntries(Object.entries(value).sort().map(([key,value])=>[key,sorted(value)])):value;
  const resource=sorted(debugControllerResource('debug-1'));resource.ports.reverse();resource.ports[0].name='Custom axis';resource.ports[0].contract.freshnessMs=1200;
  assert.equal(debugControllerError(resource),'');
});
test('malformed shapes, domains, nonzero neutral and duplicate ports are rejected',()=>{
  for(const change of [value=>value.ports[0].contract.fallback=true,value=>value.ports[8].contract.fallback=1,value=>value.ports[8].contract.schema.max=30000,value=>value.ports[8].contract.schema.unit='axis',value=>value.ports[8].contract.extra=true,value=>value.ports[0].contract.freshnessMs=0,value=>value.ports.push(value.ports[0]),value=>value.ports[0].id='button-129',value=>value.ports[0].extra=true,value=>value.ports[0]=null]){const resource=plain(debugControllerResource('debug-1'));change(resource);assert.notEqual(debugControllerError(resource),'');}
  assert.notEqual(debugControllerError(null),'');assert.notEqual(debugControllerError({}),'');
});
test('status accepts native metadata and copies its exact owner and samples',async()=>{
  const h=await harness([axis(0)]);const copy=h.client.currentOwner;copy.generation=99;assert.equal(h.client.currentOwner.generation,2);h.client.close();
});
test('status rejects wrong owner, duplicate samples and malformed control domains',async()=>{
  for(const response of [status([],{owner:{...owner(),generation:3}}),status([],{nodeId:'another'}),status([axis(1),axis(2)]),status([{portId:'button-1',value:1}]),status([axis(32768)]),status([{portId:'hat-1',value:8}]),status([axis(1,17)]),status([],{owner:{...owner(),session:owner().session.toUpperCase()+'x'}})]){
    const client=new DebugControllerClient(async()=>response,'debug-1');await assert.rejects(client.status(owner()));assert.equal(client.currentOwner,null);
  }
  const client=new DebugControllerClient(async()=>null,'debug-1');await assert.rejects(client.status({...owner(),generation:0}),/exact active/);await assert.rejects(client.status(owner()),/another owner/);
});
test('rapid press release press preserves each button edge in a unique ordered request',async()=>{
  const h=await harness();h.client.push(owner(),[button(true)]);h.client.push(owner(),[button(false)]);h.client.push(owner(),[button(true)]);
  assert.equal(h.inputs().length,1);await h.settle();await h.settle();await h.settle();
  assert.deepEqual(h.inputs().map(request=>[request.sequence,request.changes]),[['1',[button(true)]],['2',[button(false)]],['3',[button(true)]]]);h.client.close();await h.settle();assert.deepEqual(h.inputs().at(-1).changes,[button(false)]);
});
test('status validates known stream metadata while accepting future native fields',async()=>{
  for(const metadata of [{streamId:'invalid'},{sequence:'01'},{sequence:'18446744073709551616'},{sequence:1},{replayed:'false'},{pendingFrames:-1}]){
    const client=new DebugControllerClient(async()=>status([] ,metadata),'debug-1');await assert.rejects(client.status(owner()),/metadata/);
  }
  const client=new DebugControllerClient(async()=>status([],{streamId:'33333333-3333-4333-8333-333333333333',sequence:'1',replayed:false,pendingFrames:0,futureNativeField:{extra:true}}),'debug-1');await client.status(owner());client.close();
});
test('only adjacent axis batches coalesce, preserving atomic XY and barriers',async()=>{
  const h=await harness();h.client.push(owner(),[button(true)]);h.client.push(owner(),[axis(1),axis(2,2)]);h.client.push(owner(),[axis(3),axis(4,2)]);h.client.push(owner(),[button(false)]);h.client.push(owner(),[axis(5),axis(6,2)]);
  for(let i=0;i<4;i++)await h.settle();
  assert.deepEqual(h.inputs().map(request=>request.changes),[[button(true)],[axis(3),axis(4,2)],[button(false)],[axis(5),axis(6,2)]]);h.client.close();
});
test('close during an in-flight press replaces queued events with final release, retaining axes',async()=>{
  const h=await harness();h.client.push(owner(),[button(true),axis(123)]);h.client.push(owner(),[button(false)]);h.client.push(owner(),[axis(444)]);h.client.close();h.client.push(owner(),[button(true)]);assert.equal(h.client.currentOwner,null);await h.settle();
  assert.deepEqual(h.inputs().map(request=>request.changes),[[button(true),axis(123)],[button(false)]]);assert.equal(h.inputs()[1].sequence,'2');await h.settle();assert.equal(h.inputs().length,2);assert.equal(h.errors.length,0);
});
test('close during in-flight release still delivers a final false and never resets axes',async()=>{
  const h=await harness();h.client.push(owner(),[button(true)]);await h.settle();h.client.push(owner(),[button(false)]);h.client.close();await h.settle();await h.settle();assert.deepEqual(h.inputs().map(request=>request.changes),[[button(true)],[button(false)],[button(false)]]);
});
test('close with only axes sends no reset, and status-discovered held buttons release',async()=>{
  const h=await harness();h.client.push(owner(),[axis(999)]);h.client.close();await h.settle();assert.equal(h.inputs().length,1);
  const held=await harness([button(true)]);held.client.close();assert.deepEqual(held.inputs()[0].changes,[button(false)]);await held.settle();
});
test('ambiguous send failure attempts a release, fences owner and reports one error',async()=>{
  const h=await harness();h.client.push(owner(),[button(true)]);h.client.push(owner(),[axis(50)]);h.pending.shift().reject(new Error('network lost'));await tick();assert.equal(h.client.currentOwner,null);assert.equal(h.errors.length,1);assert.deepEqual(h.inputs()[1].changes,[button(false)]);assert.equal(h.inputs()[1].sequence,'2');await h.settle();h.client.close();
});
test('send and cleanup failures after close produce no callbacks or retry loop',async()=>{
  const h=await harness();h.client.push(owner(),[button(true)]);h.client.close();h.pending.shift().reject(new Error('stale owner'));await tick();assert.equal(h.inputs().length,2);h.pending.shift().reject(new Error('stale owner'));await tick();assert.equal(h.inputs().length,2);assert.equal(h.errors.length,0);
});
test('owner change never retargets a pending cleanup to the new owner',async()=>{
  const h=await harness();h.client.push(owner(),[button(true)]);h.client.clearOwner();await h.settle();assert.deepEqual(h.inputs().at(-1).owner,owner());await h.settle();await h.client.status(owner());h.client.push(owner(),[axis(4)]);assert.equal(h.inputs().at(-1).sequence,'3');await h.settle();h.client.close();
});
test('stale owner input and duplicate-port batches never reach native',async()=>{
  const h=await harness();h.client.push({...owner(),generation:3},[button(true)]);h.client.push(owner(),[button(true),button(false)]);h.client.push(owner(),[axis(90000)]);assert.equal(h.inputs().length,0);assert.equal(h.errors.length,2);h.client.close();
});
test('status resolving after close cannot restore ownership',async()=>{
  let resolve;const client=new DebugControllerClient(()=>new Promise(done=>resolve=done),'debug-1');const response=client.status(owner());client.close();resolve(status());await assert.rejects(response,/no longer active/);assert.equal(client.currentOwner,null);await assert.rejects(client.status(owner()),/exact active/);
});
test('bounded button queue fails closed and releases the possible press',async()=>{
  const h=await harness();h.client.push(owner(),[button(true)]);for(let i=0;i<129;i++)h.client.push(owner(),[button(i%2===0)]);assert.equal(h.client.currentOwner,null);assert.equal(h.errors.length,1);await h.settle();assert.deepEqual(h.inputs()[1].changes,[button(false)]);await h.settle();assert.equal(h.inputs().length,2);h.client.close();
});
test('32 inspector reopen cycles use one native stream and strictly increasing counters',async()=>{
  const streams=new Map(),requests=[],errors=[];let pressed=false;
  const send=async request=>{
    if(request.command.endsWith('_status'))return status([button(pressed)]);
    if(!streams.has(request.streamId)&&streams.size>=16)throw new Error('Native debug stream capacity exhausted');
    const sequence=BigInt(request.sequence);assert.ok(sequence>(streams.get(request.streamId)??0n));streams.set(request.streamId,sequence);requests.push(plain(request));pressed=request.changes[0].value;return{};
  };
  for(let cycle=0;cycle<32;cycle++){
    const client=createDebugControllerClient(send,'debug-1',error=>errors.push(error));await client.status(owner());client.push(owner(),[button(true)]);await tick();assert.equal(pressed,true);client.close();await tick();assert.equal(pressed,false);
  }
  assert.equal(streams.size,1);assert.equal(requests.length,64);assert.equal(errors.length,0);
});
for(const failFirst of [false,true])test(`reopening during a pending ${failFirst?'failed':'successful'} press orders old cleanup before new input`,async()=>{
  const requests=[],pending=[],oldErrors=[],newErrors=[];let pressed=false;
  const send=request=>{
    if(request.command.endsWith('_status'))return Promise.resolve(status([button(pressed)]));
    requests.push(plain(request));pressed=request.changes[0].value;return new Promise((resolve,reject)=>pending.push({resolve,reject}));
  };
  const oldClient=createDebugControllerClient(send,'debug-1',error=>oldErrors.push(error));await oldClient.status(owner());oldClient.push(owner(),[button(true)]);oldClient.close();
  const newClient=createDebugControllerClient(send,'debug-1',error=>newErrors.push(error));await newClient.status(owner());newClient.push(owner(),[button(true)]);assert.equal(requests.length,1);
  const first=pending.shift();if(failFirst)first.reject(new Error('Response lost after applying press'));else first.resolve({});await tick();assert.deepEqual(requests.map(request=>request.changes),[[button(true)],[button(false)]]);
  pending.shift().resolve({});await tick();assert.deepEqual(requests.map(request=>request.changes),[[button(true)],[button(false)],[button(true)]]);assert.equal(pressed,true);
  newClient.close();pending.shift().resolve({});await tick();assert.deepEqual(requests.at(-1).changes,[button(false)]);pending.shift().resolve({});await tick();assert.equal(pressed,false);
  assert.equal(new Set(requests.map(request=>request.streamId)).size,1);assert.ok(requests.every((request,index)=>index===0||BigInt(request.sequence)>BigInt(requests[index-1].sequence)));assert.deepEqual(oldErrors,[]);assert.deepEqual(newErrors,[]);
});
