import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-canvas-construction','./profile-editor']);
const {canvasPortReason:reason,canvasConnectionPlan:plan,canvasConnect:connect,canvasPortWriter:writer,canvasInsertionPosition:place}=modules.get('./profile-canvas-construction');
const {profileDiscoveredPorts:choices}=modules.get('./profile-discovery');
const {graphDropProcessing:drop,resolveProcessingDraft:resolve,graphUpdateProcessing:configure}=modules.get('./profile-transform-drafts');
const {blankProfile}=modules.get('./profile-editor');
const plain=value=>JSON.parse(JSON.stringify(value));
const state=(min=0,max=100,unit='rpm')=>({delivery:'state',schema:{type:'number',min,max,unit},freshnessMs:370,fallback:null});
const port=(id,direction,contract=state())=>({id,name:id,direction,contract});
const endpoint=(value,nodeId,portId)=>({nodeId,port:choices(value,nodeId).find(port=>port.profilePortId===portId)});
function graph(){
  const document=blankProfile('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa');
  document.resources=[{id:'source',name:'Source',kind:'game',required:false,ports:[port('rpm','output'),port('other','output'),port('g-force','output',state(-10,10,'g'))]},...['target','other-target'].map(id=>({id,name:id,kind:'device',required:false,ports:[port('in','input',state(0,1000)),port('second','input',state(0,1000))]}))];
  return {document,assignments:[]};
}

test('canvas construction connects exact endpoints and reports units, delivery, direction and pending output compatibility',()=>{
  const value=graph(),from=endpoint(value,'source','rpm'),to=endpoint(value,'target','in');
  assert.equal(reason(value.document,from,to),'');
  const connected=connect(value,from,to);assert.equal(connected.document.bindings.length,1);
  assert.deepEqual(plain(connected.document.bindings[0]),{id:'route-1',from:{nodeId:'source',portId:'rpm'},to:{nodeId:'target',portId:'in'}});
  assert.match(reason(value.document,endpoint(value,'source','g-force'),to),/units/);
  assert.match(reason(value.document,to,from),/source output/);
  const pending=drop(value,'scale',{factor:10});assert.match(reason(pending.document,endpoint(pending,'transform-1','out'),to),/input first/);
  assert.throws(()=>connect(pending,endpoint(pending,'transform-1','out'),to),/input first/);
  assert.equal(value.document.bindings.length,0);
});

test('first input resolution, configured Multiply and connection occur in one independent edit with exact output bounds',()=>{
  const value=drop(graph(),'scale',{factor:10}),before=plain(value);
  const connected=connect(value,endpoint(value,'source','rpm'),endpoint(value,'transform-1','in'));
  const node=connected.document.processingNodes[0];assert.equal(node.draft,undefined);assert.equal(node.operation.factor,10);
  assert.deepEqual(plain(node.inputs[0].contract),state());assert.deepEqual(plain(node.outputs[0].contract),state(0,1000));
  assert.deepEqual(plain(connected.document.bindings[0].to),{nodeId:'transform-1',portId:'in'});
  const finished=connect(connected,endpoint(connected,'transform-1','out'),endpoint(connected,'target','in'));assert.equal(finished.document.bindings.length,2);
  assert.deepEqual(plain(value),before);assert.ok(value.document.processingNodes[0].draft);
});

test('picker previews validate configured draft parameters and Gate resolves from its value input only',()=>{
  const overflow=drop(graph(),'scale',{factor:Number.MAX_SAFE_INTEGER}),from=endpoint(overflow,'source','rpm'),to=endpoint(overflow,'transform-1','in');
  assert.match(reason(overflow.document,from,to),/range/);assert.throws(()=>connect(overflow,from,to),/range/);assert.ok(overflow.document.processingNodes[0].draft);
  const value=drop(graph(),'gate'),gate=endpoint(value,'transform-1','gate');
  assert.match(reason(value.document,endpoint(value,'source','rpm'),gate),/value input first/);
  assert.throws(()=>plan(value,endpoint(value,'source','rpm'),gate),/value input first/);
  const connected=connect(value,endpoint(value,'source','rpm'),endpoint(value,'transform-1','value'));
  assert.equal(connected.document.processingNodes[0].draft,undefined);assert.deepEqual(plain(connected.document.processingNodes[0].outputs[0].contract),state());
});

test('occupied input replacement is explicit and preserves its existing route identity atomically',()=>{
  let value=graph();value=connect(value,endpoint(value,'source','rpm'),endpoint(value,'target','in'));const original=plain(value),id=value.document.bindings[0].id;
  const from=endpoint(value,'source','other'),to=endpoint(value,'target','in'),preview=plan(value,from,to);
  assert.equal(preview.error,'');assert.equal(preview.writer.id,id);assert.equal(writer(value.document,'target',to.port).id,id);assert.equal(writer(value.document,'target',to.port,id),undefined);
  assert.throws(()=>connect(value,from,to),/Replace connection/);assert.deepEqual(plain(value),original);
  const replaced=connect(value,from,to,undefined,true);assert.equal(replaced.document.bindings.length,1);assert.equal(replaced.document.bindings[0].id,id);assert.equal(replaced.document.bindings[0].from.portId,'other');
  assert.deepEqual(plain(value),original);
});

test('retargeting either endpoint keeps the selected route identity and leaves all unrelated wires untouched',()=>{
  let value=graph();value=connect(value,endpoint(value,'source','rpm'),endpoint(value,'target','in'));
  value=connect(value,endpoint(value,'source','other'),endpoint(value,'other-target','second'));const before=plain(value),id=value.document.bindings[0].id,unrelated=plain(value.document.bindings[1]);
  const retargeted=connect(value,endpoint(value,'source','other'),endpoint(value,'target','second'),id);
  assert.equal(retargeted.document.bindings.length,2);assert.deepEqual(plain(retargeted.document.bindings.find(row=>row.id===id)),{id,from:{nodeId:'source',portId:'other'},to:{nodeId:'target',portId:'second'}});
  assert.deepEqual(plain(retargeted.document.bindings.find(row=>row.id===unrelated.id)),unrelated);assert.deepEqual(plain(value),before);
  assert.throws(()=>plan(value,endpoint(value,'source','rpm'),endpoint(value,'target','second'),'removed-route'),/removed/);
});

test('retargeting into an occupied input replaces only the second writer after explicit consent',()=>{
  let value=graph();value=connect(value,endpoint(value,'source','rpm'),endpoint(value,'target','in'));
  value=connect(value,endpoint(value,'source','other'),endpoint(value,'target','second'));const [selected,occupied]=value.document.bindings,before=plain(value);
  const from=endpoint(value,'source','rpm'),to=endpoint(value,'target','second');assert.equal(plan(value,from,to,selected.id).writer.id,occupied.id);
  assert.throws(()=>connect(value,from,to,selected.id),/Replace connection/);
  const replaced=connect(value,from,to,selected.id,true);assert.equal(replaced.document.bindings.length,1);assert.equal(replaced.document.bindings[0].id,selected.id);assert.equal(replaced.document.bindings[0].to.portId,'second');assert.deepEqual(plain(value),before);
});

test('incompatible construction never mutates lazy provider pins, routes or pending source contracts',()=>{
  const value=graph();for(const resource of value.document.resources){resource.ports=[];value.assignments.push({resourceId:resource.id,sourceId:`native:${resource.id}`,ports:[]});}
  const hash=`sha256:${'a'.repeat(64)}`,observed=(id,direction,contract)=>({descriptor:port(id,direction,contract),contractHash:hash,current:true,routeIds:['native:route']});
  const discovered=(nodeId,native)=>({nodeId,port:{...native.descriptor,key:`@observed:${native.descriptor.id}`,profilePortId:null,sourceId:`native:${nodeId}`,observed:native,current:true}});
  const from=discovered('source',observed('native:g','output',state(-10,10,'g'))),to=discovered('target',observed('native:rpm','input',state(0,10000,'rpm'))),before=plain(value);
  assert.match(plan(value,from,to).error,/units/);assert.throws(()=>connect(value,from,to),/units/);assert.deepEqual(plain(value),before);
  const reassigned=plain(value);reassigned.assignments[0].sourceId='different:source';assert.throws(()=>plan(reassigned,from,to),/assignment changed/);assert.equal(reassigned.assignments[0].ports.length,0);
});

test('stale resolved processing choices cannot silently substitute a changed contract or missing port',()=>{
  const value=resolve(drop(graph(),'scale'),'transform-1',state()),from=endpoint(value,'transform-1','out'),to=endpoint(value,'target','in');
  const edited=plain(value.document.processingNodes[0]);edited.operation.factor=2;const changed=configure(value,edited),before=plain(changed);
  assert.throws(()=>plan(changed,from,to),/changed/);assert.throws(()=>connect(changed,from,to),/changed/);assert.deepEqual(plain(changed),before);
  const missing=plain(from);missing.port.profilePortId='missing';assert.throws(()=>plan(value,missing,to),/changed/);
});

test('transformation insertion keeps a vacant top-left drop position and treats a 32px margin as free',()=>{
  const width=288,height=188,point={x:320,y:0},nodes=[{position:{x:0,y:0}}],before=plain(nodes);
  assert.deepEqual(plain(place([],point,width,height)),point);
  assert.deepEqual(plain(place(nodes,point,width,height)),point);
  assert.deepEqual(plain(nodes),before);assert.deepEqual(point,{x:320,y:0});
});

test('a transformation dropped into a tight wire gap moves to a vacant nearby location instead of overlapping either endpoint',()=>{
  const width=288,height=188,point={x:350,y:80},nodes=[{position:{x:80,y:80}},{position:{x:500,y:80}}],before=plain(nodes),next=place(nodes,point,width,height);
  const overlaps=node=>next.x<node.position.x+width+32&&next.x+width+32>node.position.x&&next.y<node.position.y+height+32&&next.y+height+32>node.position.y;
  assert.ok(Number.isFinite(next.x)&&Number.isFinite(next.y));assert.ok(nodes.every(node=>!overlaps(node)));assert.notDeepEqual(plain(next),point);assert.ok(Math.hypot(next.x-point.x,next.y-point.y)<=width+height+64,'Placement should remain near the intended wire.');
  assert.deepEqual(plain(nodes),before);assert.deepEqual(point,{x:350,y:80});
});

test('insertion placement finds open space around a packed group without moving existing blocks',()=>{
  const width=288,height=188,nodes=Array.from({length:9},(_,index)=>({position:{x:(index%3)*320,y:Math.floor(index/3)*220}})),before=plain(nodes),point={x:320,y:220},next=place(nodes,point,width,height);
  assert.ok(nodes.every(node=>!(next.x<node.position.x+width+32&&next.x+width+32>node.position.x&&next.y<node.position.y+height+32&&next.y+height+32>node.position.y)));
  assert.deepEqual(plain(nodes),before);assert.deepEqual(point,{x:320,y:220});
});

test('mutually exclusive vehicle writers share a destination without replacement or identity loss',()=>{
  let value=graph();value.document.eligibility.gameIds=['dcs'];
  value.document.resources.push({...plain(value.document.resources[0]),id:'viper-source'});
  value.document.vehicleTabs=[{id:'hornet',gameId:'dcs',vehicleId:'hornet',vehicleName:'Hornet',nodeIds:['source']},{id:'viper',gameId:'dcs',vehicleId:'viper',vehicleName:'Viper',nodeIds:['viper-source']}];
  value=connect(value,endpoint(value,'source','rpm'),endpoint(value,'target','in'));
  const hornet=plain(value.document.bindings[0]),before=plain(value),from=endpoint(value,'viper-source','rpm'),to=endpoint(value,'target','in');
  assert.equal(plan(value,from,to).writer,undefined);
  assert.equal(writer(value.document,'target',to.port,undefined,'viper'),undefined);
  assert.equal(writer(value.document,'target',to.port,undefined,'hornet').id,hornet.id);
  value=connect(value,from,to);
  assert.equal(value.document.bindings.length,2);assert.deepEqual(plain(value.document.bindings.find(binding=>binding.id===hornet.id)),hornet);
  const viper=value.document.bindings.find(binding=>binding.id!==hornet.id);
  const replaced=connect(value,endpoint(value,'viper-source','other'),to,undefined,true);
  assert.equal(replaced.document.bindings.length,2);
  assert.deepEqual(plain(replaced.document.bindings.find(binding=>binding.id===hornet.id)),hornet);
  assert.equal(replaced.document.bindings.find(binding=>binding.id===viper.id).from.portId,'other');
  assert.equal(before.document.bindings.length,1);
});
