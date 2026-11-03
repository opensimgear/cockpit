import test from 'node:test';
import assert from 'node:assert/strict';
import {SharedObservations} from '../streams/shared.ts';
import {load,blobs,deferred,until} from '../game-read-models/test-fixture.mjs';
const {WorkspaceReadModels,captureWorkspaceModel}=await load('../workspace-read-models/shared');
const producer='11111111-1111-4111-8111-111111111111',tag=`sha256:${'1'.repeat(64)}`;
const gatewayId='stm32:00112233445566778899aabb';
const port={path:'COM1',gatewayId,backend:'stm32',serialNumber:'00112233445566778899aabb'};
const module=(id='pedals')=>({id,online:true,registered:true,active:true,settings:{label:'Pedals'}});
const entry=(changes={})=>({id:gatewayId,port,state:{gatewayId,running:false,connected:true,needsReconnect:false,modules:[module()],claimedModules:['untrusted']},error:'',busy:false,present:true,session:'native-connection-1',claimOwners:{pedals:tag},...changes});
const model=(rows=[entry()])=>({ports:[port],inventoryError:'',entries:rows});
const frame=(value,changes={})=>({kind:'snapshot',topic:'workspace.state',source:'workspace',session:'broker',generation:'1',revision:'1',sequence:'1',capturedUs:'1',clockId:`workspace:${producer}`,continuity:'baseline',payload:{value},...changes});
const change=(revision=2,extra={})=>({kind:'change',continuity:'continuous',revision:String(revision),sequence:String(revision),...extra});
function harness(getPage){const opens=[],client=new WorkspaceReadModels(new SharedObservations({open(topic,observer,signal){opens.push({topic,observer,signal,delivery:0});}}),getPage);function send(value=model(),options={},open=opens.at(-1)){open.observer.frame({subscriptionId:open.topic,delivery:String(++open.delivery),frame:frame(value,options)},async()=>{});}return{client,opens,send};}

test('one refcounted workspace projection shares immutable complete rows and never issues fallback reads',async()=>{
 const h=harness(),a=[],b=[],one=h.client.observe(state=>a.push(state)),two=h.client.observe(state=>b.push(state));assert.equal(h.opens.length,1);assert.equal(h.opens[0].topic,'workspace.state');
 h.send();await until(()=>a.at(-1).status==='ready');assert.equal(a.at(-1).value,b.at(-1).value);assert.equal(Object.isFrozen(a.at(-1).value.entries[0].state.modules[0].settings),true);assert.equal(a.at(-1).value.entries[0].stale,false);
 one();one();assert.equal(h.opens[0].signal.aborted,false);two();assert.equal(h.opens[0].signal.aborted,true);
 const seen=[],off=h.client.observe(state=>seen.push(state));assert.equal(seen.at(-1).status,'connecting');assert.equal(seen.at(-1).value,undefined);assert.equal(h.opens.length,2);off();
});

test('new public scopes replace pending pages and retire late completions without overwriting current rows',async()=>{
 const f=blobs(),old=f.add({type:'leaf',items:[entry()]},'array'),next=f.add({type:'leaf',items:[entry({session:'new-connection',error:'Current'})]},'array'),gate=deferred();let oldSignal;
 const h=harness(async(...args)=>{if(args[0]===old.$gamePage.id){oldSignal=args[2];await gate.promise;}return f.get(...args);}),states=[],off=h.client.observe(state=>states.push(state));
 h.send();await until(()=>states.at(-1).status==='ready');const cached=states.at(-1).value;
 h.send({...model(),entries:old},change());await until(()=>oldSignal);assert.equal(states.at(-1).status,'lost');assert.equal(states.at(-1).value,cached);
 h.send({...model(),entries:next},change(3));await until(()=>states.at(-1).status==='ready');assert.equal(oldSignal.aborted,true);assert.equal(states.at(-1).value.entries[0].session,'new-connection');
 gate.resolve();await new Promise(resolve=>setImmediate(resolve));assert.equal(states.at(-1).revision,'3');off();
});

test('same pending hash deduplicates immutable loads and unchanged row objects remain stable',async()=>{
 const f=blobs(),row=f.add({type:'leaf',entries:Object.entries(entry())},'object'),tree={...model(),entries:[row]},gate=deferred();let calls=0;
 const h=harness(async(...args)=>{calls++;await gate.promise;return f.get(...args);}),states=[],off=h.client.observe(state=>states.push(state));h.send(tree);await until(()=>calls===1);h.send(tree,change());gate.resolve();await until(()=>states.at(-1).status==='ready');const first=states.at(-1).value.entries[0];assert.equal(states.at(-1).revision,'2');assert.equal(calls,1);
 h.send({...tree,inventoryError:'A different gateway discovery warning'},change(3));await until(()=>states.at(-1).status==='ready');assert.equal(states.at(-1).value.entries[0],first);assert.equal(calls,1);off();
});

test('loss and invalid envelopes retain stale cache until a fresh baseline and reject producer replacement',async()=>{
 const h=harness(),states=[],off=h.client.observe(state=>states.push(state));h.send();await until(()=>states.at(-1).status==='ready');const cached=states.at(-1).value;
 h.send(undefined,{kind:'loss',continuity:'lost',generation:'2',revision:'2',sequence:'2',reason:'Native workspace lost'});assert.equal(states.at(-1).status,'lost');assert.equal(states.at(-1).value,cached);
 h.send(model(),change(3));assert.equal(states.at(-1).status,'lost');h.send(model(),{generation:'2',revision:'3',sequence:'3'});await until(()=>states.at(-1).status==='ready');
 h.send(model(),change(4,{generation:'2',clockId:'workspace:22222222-2222-4222-8222-222222222222'}));assert.equal(states.at(-1).status,'error');assert.equal(states.at(-1).value.entries.length,1);off();
});

test('page failure is explicit and a newer complete replacement recovers without a polling fallback',async()=>{
 const f=blobs(),ref=f.add({type:'leaf',items:[entry()]},'array');let expired=true;
 const h=harness(async(...args)=>{if(expired)throw Error('Public page expired');return f.get(...args);}),states=[],off=h.client.observe(state=>states.push(state));h.send({...model(),entries:ref});await until(()=>states.at(-1).status==='error');assert.match(states.at(-1).error,/expired/);
 expired=false;h.send({...model(),entries:ref},change());await until(()=>states.at(-1).status==='ready');off();
});

test('last unsubscribe aborts in-flight workspace page consumers and prevents late model revival',async()=>{
 const f=blobs(),ref=f.add({type:'leaf',items:[entry()]},'array'),gate=deferred();let signal;
 const h=harness(async(...args)=>{signal=args[2];await gate.promise;return f.get(...args);}),states=[],off=h.client.observe(state=>states.push(state));h.send({...model(),entries:ref});await until(()=>signal);off();const count=states.length;assert.equal(signal.aborted,true);assert.equal(h.opens[0].signal.aborted,true);gate.resolve();await new Promise(resolve=>setImmediate(resolve));assert.equal(states.length,count);
});

test('public workspace enforces bounded complete catalogues and digest ownership, never raw client identities',()=>{
 for(const broken of [
  {...model(),ports:Array(65).fill(port)},model(Array(65).fill(entry())),model([entry(),entry()]),{...model(),inventoryError:7},model([entry({id:'COM1'})]),model([entry({session:''})]),
  model([entry({claimOwners:{pedals:producer}})]),model([entry({claimOwners:{'':tag}})]),model([entry({present:1})]),model([entry({busy:'yes'})]),model([entry({stale:1})]),model([entry({autoConnectPaused:1})]),
  model([entry({port:{...port,gatewayId:'stm32:other'}})]),model([entry({port:{...port,path:''}})]),model([entry({port:{...port,backend:'unknown'}})]),model([entry({port:{...port,manufacturer:2}})]),
  model([entry({state:{...entry().state,gatewayId:'pico:other'}})]),model([entry({state:{...entry().state,modules:[module(),module()]}})]),model([entry({state:{...entry().state,modules:[{...module(),online:1}]}})]),{...model(),privateClientId:producer},
 ])assert.throws(()=>captureWorkspaceModel(broken));
 assert.equal(captureWorkspaceModel(model([entry({state:null,claimOwners:{}})])).entries[0].state,null);
});

test('invalid topics/sources and throwing listeners or transport cannot produce a partial ready view',async()=>{
 const h=harness(),states=[],one=h.client.observe(()=>{throw Error('consumer');}),two=h.client.observe(state=>states.push(state));h.send(model(),{source:'games'});assert.equal(states.at(-1).status,'error');
 // The underlying transport pins its producer identity even when domain capture
 // rejects a baseline. A different producer requires a new observation.
 h.send(model(),{revision:'2',sequence:'2'});assert.equal(states.at(-1).status,'lost');one();two();
 const reopened=[],stopThrow=h.client.observe(()=>{throw Error('consumer');}),stop=h.client.observe(state=>reopened.push(state));
 h.send(model());await until(()=>reopened.at(-1).status==='ready');stopThrow();stop();
 const values=[],client=new WorkspaceReadModels({subscribe(){throw Error('Open failed');}}),off=client.observe(state=>values.push(state));assert.equal(values.at(-1).status,'error');off();
});
