import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
import ts from 'typescript';
import {SharedObservations} from '../streams/shared.ts';
import {uint64} from '../streams/protocol.ts';
const file=new URL('./shared.ts',import.meta.url),exports={};
const code=ts.transpileModule(await readFile(file,'utf8'),{fileName:fileURLToPath(file),compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,inlineSourceMap:true,inlineSources:true}}).outputText;
vm.runInNewContext(code,{exports,require(name){if(name==='../streams/shared.ts')return{SharedObservations};if(name==='../streams/protocol.ts')return{uint64};throw Error(name);}},{filename:fileURLToPath(file)});
const {ControllerReadModels,captureControllerInventory}=exports;
const id=n=>`11111111-1111-4111-8111-${String(n).padStart(12,'0')}`,producer=id(99);
const row=(number=1,changes={})=>({id:id(number),name:`Controller ${number}`,role:'pedals',origin:'host-controller',backend:'windows-raw-input',vendorId:1,productId:2,osgHid:false,online:true,generation:'1',sequence:'0',controlCount:12,baselineReady:true,identityConfidence:'serial',...changes});
const access=(changes={})=>({status:'unchanged',ready:true,changed:false,message:'Capture access verified.',...changes});
const payload=(sources=[row()],changes={})=>({session:producer,backendStatus:'ready',diagnostic:'',selfAccess:access(),sources,offset:0,total:sources.length,limit:256,...changes});
const frame=(value,changes={})=>({kind:'snapshot',topic:'controllers.inventory',source:'controller-input',session:'broker',generation:'1',revision:'1',sequence:'1',capturedUs:'1',clockId:`controllers:${producer}`,continuity:'baseline',payload:value,...changes});
const change=(revision=2,extras={})=>({kind:'change',continuity:'continuous',revision:String(revision),sequence:String(revision),...extras});
function harness(){
  const opens=[],observations=new SharedObservations({open(topic,observer,signal){opens.push({topic,observer,signal,delivery:0});}}),client=new ControllerReadModels(observations);
  function send(value=payload(),options={},open=opens.at(-1)){open.observer.frame({subscriptionId:open.topic,delivery:String(++open.delivery),frame:frame(value,options)},async()=>{});}
  return{client,opens,send};
}

test('inventory is one shared immutable metadata projection with independent refcounts and no selected samples',()=>{
  const h=harness(),a=[],b=[];const one=h.client.observeInventory(state=>a.push(state)),two=h.client.observeInventory(state=>b.push(state));
  assert.equal(h.opens.length,1);assert.equal(h.opens[0].topic,'controllers.inventory');h.send();
  assert.equal(a.at(-1).status,'ready');assert.equal(a.at(-1).value,b.at(-1).value);assert.equal(Object.isFrozen(a.at(-1).value.sources[0]),true);
  assert.equal(a.at(-1).value.sources[0].sequence,'0');assert.equal(Object.hasOwn(a.at(-1).value,'controls'),false);assert.equal(Object.hasOwn(a.at(-1).value,'source'),false);
  one();one();assert.equal(h.opens[0].signal.aborted,false);two();assert.equal(h.opens[0].signal.aborted,true);
  const c=[],off=h.client.observeInventory(state=>c.push(state));assert.equal(h.opens.length,2);assert.equal(c.at(-1).status,'connecting');assert.equal(c.at(-1).value,undefined);off();
});

test('complete replacements add, rename, disconnect and remove metadata atomically while reusing unchanged rows',()=>{
  const h=harness(),states=[],off=h.client.observeInventory(state=>states.push(state));h.send(payload([row(1),row(2)]));const original=states.at(-1).value;
  h.send(payload([row(1),row(2,{name:'Renamed pedals',online:false,baselineReady:false}),row(3)]),change());const next=states.at(-1).value;
  assert.equal(next.sources[0],original.sources[0]);assert.notEqual(next.sources[1],original.sources[1]);assert.equal(next.sources[1].name,'Renamed pedals');assert.equal(next.sources[1].online,false);assert.equal(next.sources.length,3);
  h.send(payload([row(1),row(3)]),change(3));assert.equal(states.at(-1).value.sources.length,2);assert.equal(states.at(-1).value.sources[1],next.sources[2]);
  const rows=states.at(-1).value.sources;h.send(payload([row(1),row(3)]),change(4));assert.equal(states.at(-1).value.sources,rows);off();
});

test('loss retains labelled cached metadata and only a fresh current baseline restores readiness',()=>{
  const h=harness(),states=[],off=h.client.observeInventory(state=>states.push(state));h.send();const previous=states.at(-1).value;
  h.send(undefined,{kind:'loss',continuity:'lost',generation:'2',revision:'2',sequence:'2',reason:'Controller service replaced'});
  assert.equal(states.at(-1).status,'lost');assert.equal(states.at(-1).value,previous);
  h.send(payload([row(2)]),change(3));assert.equal(states.at(-1).status,'lost');
  h.send(payload([row(2)]),{generation:'2',revision:'3',sequence:'3'});assert.equal(states.at(-1).status,'ready');assert.equal(states.at(-1).value.sources[0].id,id(2));
  const current=states.at(-1);h.send(payload(),{generation:'1',revision:'2',sequence:'2'});assert.equal(states.at(-1),current);off();
});

test('invalid full replacements never partially replace cached rows and require a fresh snapshot',()=>{
  const h=harness(),states=[],off=h.client.observeInventory(state=>states.push(state));h.send();const previous=states.at(-1).value;
  h.send(payload([row(2),row(3,{sequence:'9'})]),change());assert.equal(states.at(-1).status,'error');assert.equal(states.at(-1).value,previous);
  h.send(payload([row(2)]),change(3));assert.equal(states.at(-1).status,'lost');
  h.send(payload([row(2)]),{revision:'4',sequence:'4'});assert.equal(states.at(-1).status,'ready');assert.equal(states.at(-1).value.sources[0].id,id(2));off();
});

test('producer clock and broker session boundaries reject late replacements and terminal transport errors preserve cache',()=>{
  const h=harness(),states=[],off=h.client.observeInventory(state=>states.push(state));h.send();const value=states.at(-1).value;
  const nextProducer=id(98);h.send(payload([row(2)],{session:nextProducer}),change(2,{clockId:`controllers:${nextProducer}`}));assert.equal(states.at(-1).status,'error');assert.equal(states.at(-1).value,value);off();
  const second=harness(),seen=[],stop=second.client.observeInventory(state=>seen.push(state));second.send();second.send(payload(),change(2,{session:'other-broker'}));assert.equal(seen.at(-1).status,'lost');stop();
  const third=harness(),errors=[],done=third.client.observeInventory(state=>errors.push(state));third.send();third.opens[0].observer.error('Capture transport unavailable');assert.equal(errors.at(-1).status,'error');assert.equal(errors.at(-1).value.sources.length,1);done();
});

test('strict full catalogue bounds, UUIDs, metadata fields and immutable startup self-access are validated',()=>{
  const valid=captureControllerInventory(frame(payload(Array.from({length:256},(_,index)=>row(index)))));
  assert.equal(valid.total,256);assert.equal(valid.selfAccess.ready,true);assert.equal(Object.isFrozen(valid.selfAccess),true);
  const diagnostic='x'.repeat(65536);
  assert.equal(captureControllerInventory(frame(payload([],{diagnostic,selfAccess:access({message:diagnostic})}))).diagnostic,diagnostic);
  for(const broken of [payload(Array.from({length:257},(_,index)=>row(index))),payload([row(1),row(1)]),payload([row(1,{id:'physical-path'})]),
    payload([row(1,{vendorId:65536})]),payload([row(1,{productId:-1})]),payload([row(1,{generation:'01'})]),payload([row(1,{sequence:'1'})]),payload([row(1,{controlCount:257})]),
    payload([row(1,{online:false})]),payload([row(1,{origin:'synthetic'})]),payload([row(1,{identityConfidence:'unknown'})]),payload([row(1,{name:'x'.repeat(513)})]),payload([row(1,{values:{}})]),
    payload([],{session:'registry'}),payload([],{offset:32}),payload([],{limit:32}),payload([],{total:1}),payload([],{backendStatus:3}),payload([],{diagnostic:null}),payload([],{diagnostic:'x'.repeat(65537)}),
    payload([],{controls:[]}),payload([],{selfAccess:undefined}),payload([],{selfAccess:access({status:'invented'})}),payload([],{selfAccess:access({ready:1})}),payload([],{selfAccess:access({changed:'yes'})}),
    payload([],{selfAccess:access({message:null})}),payload([],{selfAccess:access({executable:7})}),payload([],{selfAccess:access({unknown:true})})])assert.throws(()=>captureControllerInventory(frame(broken)));
  for(const patch of [{topic:'controllers.input.wrong'},{source:'games'},{clockId:`controllers:${id(55)}`},{kind:'loss'}])assert.throws(()=>captureControllerInventory(frame(payload(),patch)));
  for(const status of ['blocked','unsupported','error','not-requested']){const captured=captureControllerInventory(frame(payload([],{selfAccess:access({status,ready:false,changed:null,executable:'C:/Cockpit.exe'})})));assert.equal(captured.selfAccess.status,status);assert.equal(captured.selfAccess.changed,null);}
});

test('consumer exceptions and late callbacks cannot stall other listeners or revive disposed inventory',()=>{
  const h=harness(),states=[];const one=h.client.observeInventory(()=>{throw Error('consumer');}),two=h.client.observeInventory(state=>states.push(state));h.send();assert.equal(states.at(-1).status,'ready');one();two();const count=states.length;h.send(payload([row(2)]),change());assert.equal(states.length,count);
  const failures=[],client=new ControllerReadModels({subscribe(){throw Error('Transport open failed');}}),stop=client.observeInventory(state=>failures.push(state));assert.equal(failures.at(-1).status,'error');stop();
});

test('clock/session fencing and incomplete replacement rejection also hold for injected observations',()=>{
  let emit;const client=new ControllerReadModels({subscribe(_topic,listener){emit=listener;return()=>{};}}),states=[],off=client.observeInventory(state=>states.push(state));
  emit({status:'ready',frame:frame(payload(),change())});assert.equal(states.at(-1).status,'lost');
  emit({status:'ready',frame:frame(payload())});const current=states.at(-1);emit({status:'ready',frame:frame(payload([row(2)]),{revision:'0'})});assert.equal(states.at(-1),current);
  emit({status:'ready',frame:frame(payload([row(2)]),change(2,{generation:'2'}))});assert.equal(states.at(-1).status,'lost');
  emit({status:'ready',frame:frame(payload([row(2)]),{generation:'2',revision:'3'})});assert.equal(states.at(-1).status,'ready');off();
});
