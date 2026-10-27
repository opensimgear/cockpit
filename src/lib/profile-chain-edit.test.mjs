import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profiles','./profile-graph','./profile-processing','./profile-chain-edit']);
const graph=modules.get('./profile-graph');
const processing=modules.get('./profile-processing');
const chain=modules.get('./profile-chain-edit');
const plain=value=>JSON.parse(JSON.stringify(value));
const schema={type:'number',min:-100,max:100,unit:'m'};
const contract={delivery:'state',schema,freshnessMs:1000,fallback:null};
const port=(id,direction,source=contract)=>({id,name:id,direction,contract:plain(source)});
const stage=(id,kind='identity',name=kind==='identity'?'Identity':'Scale and offset')=>processing.processingTemplate(kind,id,contract,contract);
const makeDraft=()=>{
 const document={schemaVersion:2,id:'11111111-1111-4111-8111-111111111111',name:'Chain',revision:1,eligibility:{gameIds:[]},resources:[
  {id:'source',name:'Source',kind:'device',required:false,ports:[port('value','output')]},
  {id:'target',name:'Target',kind:'device',required:false,ports:[port('value','input')]},
 ],bindings:[],processingNodes:[stage('before','scale'),stage('middle'),stage('after','scale')],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[{nodeId:'before',x:10,y:20},{nodeId:'middle',x:200,y:20},{nodeId:'after',x:390,y:20}],viewport:{x:0,y:0,zoom:1}}};
 let value={document,assignments:[]};
 value=graph.graphConnect(value,{nodeId:'source',portId:'value'},{nodeId:'before',portId:'in'});
 value=graph.graphConnect(value,{nodeId:'before',portId:'out'},{nodeId:'middle',portId:'in'});
 value=graph.graphConnect(value,{nodeId:'middle',portId:'out'},{nodeId:'after',portId:'in'});
 value=graph.graphConnect(value,{nodeId:'after',portId:'out'},{nodeId:'target',portId:'value'});
 return value;
};

test('typed identity can be replaced by compatible scale while preserving its position and route bindings',()=>{
 const value=makeDraft(),before=plain(value),choices=chain.chainTransformOptions(value,'middle');
 assert.ok(choices.some(choice=>choice.value==='scale'));
 const next=chain.replaceChainTransform(value,'middle','scale');
 const old=before.document.processingNodes.find(node=>node.id==='middle'),updated=next.document.processingNodes.find(node=>node.id==='middle');
 assert.equal(updated.id,'middle');assert.equal(updated.operation.kind,'scale');assert.equal(updated.name,'Scale and offset');
 assert.equal(updated.inputs[0].id,old.inputs[0].id);assert.equal(updated.outputs[0].id,old.outputs[0].id);
 assert.deepEqual(plain(updated.inputs[0].contract),old.inputs[0].contract);assert.deepEqual(plain(updated.outputs[0].contract),old.outputs[0].contract);
 assert.deepEqual(plain(next.document.bindings),before.document.bindings);
 assert.deepEqual(plain(next.document.editor.positions),before.document.editor.positions);
 assert.equal(old.operation.kind,'identity');
});

test('every preset remains selectable while incompatible replacement fails without changing the draft',()=>{
 const value=makeDraft(),before=plain(value);
 assert.ok(chain.chainTransformOptions(value,'middle').some(choice=>choice.value==='compare'));
 assert.equal(chain.chainTransformOptions(value,'middle').length,processing.PROCESSING_PRESETS.length);
 assert.throws(()=>chain.replaceChainTransform(value,'middle','compare'),/preserve.*route contracts/);
 assert.deepEqual(plain(value),before);
});

test('multi-input and branched transforms remain selectable but cannot be replaced in place',()=>{
 const value=makeDraft(),node=value.document.processingNodes.find(item=>item.id==='middle');
 node.inputs.push({...plain(node.inputs[0]),id:'second'});
 assert.equal(chain.chainTransformOptions(value,'middle').length,processing.PROCESSING_PRESETS.length);
 assert.throws(()=>chain.replaceChainTransform(value,'middle','identity'),/Branched or incomplete/);
 const branch=makeDraft();
 branch.document.bindings.push({id:'route-branch',from:{nodeId:'middle',portId:'out'},to:{nodeId:'target',portId:'value'}});
 assert.equal(chain.chainTransformOptions(branch,'middle').length,processing.PROCESSING_PRESETS.length);
 assert.throws(()=>chain.replaceChainTransform(branch,'middle','identity'),/Branched or incomplete/);
});

test('custom transform names survive compatible replacement',()=>{
 const value=makeDraft();value.document.processingNodes.find(node=>node.id==='middle').name='Altitude calibration';
 const next=chain.replaceChainTransform(value,'middle','scale');
 assert.equal(next.document.processingNodes.find(node=>node.id==='middle').name,'Altitude calibration');
});

test('a connected two-input OR can become AND without changing either route or port identity',()=>{
 const booleanContract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
 const booleanPort=(id,direction)=>({id,name:id,direction,contract:plain(booleanContract)});
 const or=processing.processingTemplate('any','logic',booleanContract,booleanContract);
 or.name='Custom condition';or.inputs[0].name='Left signal';or.inputs[1].name='Right signal';or.outputs[0].name='Condition result';
 const document={schemaVersion:2,id:'33333333-3333-4333-8333-333333333333',name:'Two input chain',revision:1,eligibility:{gameIds:[]},resources:[
  {id:'left',name:'Left',kind:'device',required:false,ports:[booleanPort('left-value','output')]},
  {id:'right',name:'Right',kind:'device',required:false,ports:[booleanPort('right-value','output')]},
  {id:'target',name:'Target',kind:'device',required:false,ports:[booleanPort('result','input')]},
 ],bindings:[],processingNodes:[or],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[{nodeId:'logic',x:220,y:55}],viewport:{x:0,y:0,zoom:1}}};
 let value={document,assignments:[]};
 value=graph.graphConnect(value,{nodeId:'left',portId:'left-value'},{nodeId:'logic',portId:or.inputs[0].id});
 value=graph.graphConnect(value,{nodeId:'right',portId:'right-value'},{nodeId:'logic',portId:or.inputs[1].id});
 value=graph.graphConnect(value,{nodeId:'logic',portId:or.outputs[0].id},{nodeId:'target',portId:'result'});
 const before=plain(value),old=before.document.processingNodes[0],next=chain.replaceChainTransform(value,'logic','all'),updated=next.document.processingNodes[0];
 assert.equal(updated.operation.kind,'all');assert.equal(updated.name,'Custom condition');
 assert.deepEqual(plain(updated.inputs.map(({id,name,contract})=>({id,name,contract}))),old.inputs.map(({id,name,contract})=>({id,name,contract})));
 assert.deepEqual(plain(updated.outputs),old.outputs);
 assert.deepEqual(plain(next.document.bindings),before.document.bindings);
 assert.deepEqual(plain(next.document.editor.positions),before.document.editor.positions);
 assert.deepEqual(plain(value),before);
});

test('a connected three-input OR becomes AND without dropping its third route',()=>{
 const booleanContract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
 const booleanPort=(id,direction)=>({id,name:id,direction,contract:plain(booleanContract)});
 let node=processing.processingTemplate('any','logic-three',booleanContract,booleanContract);
 node=chain.addChainInput(node);node.inputs[2].name='Third signal';
 const resources=['left','middle','right'].map(id=>({id,name:id,kind:'device',required:false,ports:[booleanPort(`${id}-value`,'output')]}));
 resources.push({id:'target',name:'Target',kind:'device',required:false,ports:[booleanPort('result','input')]});
 let value={document:{schemaVersion:2,id:'44444444-4444-4444-8444-444444444444',name:'Three input OR',revision:1,eligibility:{gameIds:[]},resources,bindings:[],processingNodes:[node],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[{nodeId:node.id,x:210,y:35}],viewport:{x:0,y:0,zoom:1}}},assignments:[]};
 for(const [index,id] of ['left','middle','right'].entries())value=graph.graphConnect(value,{nodeId:id,portId:`${id}-value`},{nodeId:node.id,portId:node.inputs[index].id});
 value=graph.graphConnect(value,{nodeId:node.id,portId:'out'},{nodeId:'target',portId:'result'});
 const before=plain(value),next=chain.replaceChainTransform(value,node.id,'all'),updated=next.document.processingNodes[0];
 assert.equal(updated.operation.kind,'all');assert.equal(updated.inputs.length,3);
 assert.deepEqual(plain(updated.inputs.map(({id,name})=>({id,name}))),before.document.processingNodes[0].inputs.map(({id,name})=>({id,name})));
 assert.deepEqual(plain(next.document.bindings),before.document.bindings);assert.deepEqual(plain(next.document.editor.positions),before.document.editor.positions);
 assert.deepEqual(plain(value),before);
});

test('a compatible three-input priority replacement preserves its ordered inputs and names',()=>{
 const booleanContract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
 const booleanPort=(id,direction)=>({id,name:id,direction,contract:plain(booleanContract)});
 let node=processing.processingTemplate('priority','priority-three',booleanContract,booleanContract);
 node=chain.addChainInput(node);node.inputs[2].name='Third signal';node.operation.order=['input-3','second','first'];
 const resources=['left','middle','right'].map(id=>({id,name:id,kind:'device',required:false,ports:[booleanPort(`${id}-value`,'output')]}));
 resources.push({id:'target',name:'Target',kind:'device',required:false,ports:[booleanPort('result','input')]});
 let value={document:{schemaVersion:2,id:'55555555-5555-4555-8555-555555555555',name:'Three input priority',revision:1,eligibility:{gameIds:[]},resources,bindings:[],processingNodes:[node],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[{nodeId:node.id,x:210,y:35}],viewport:{x:0,y:0,zoom:1}}},assignments:[]};
 for(const [index,id] of ['left','middle','right'].entries())value=graph.graphConnect(value,{nodeId:id,portId:`${id}-value`},{nodeId:node.id,portId:node.inputs[index].id});
 value=graph.graphConnect(value,{nodeId:node.id,portId:'out'},{nodeId:'target',portId:'result'});
 const before=plain(value),old=before.document.processingNodes[0],next=chain.replaceChainTransform(value,node.id,'priority'),updated=next.document.processingNodes[0];
 assert.equal(updated.inputs.length,3);assert.deepEqual(plain(updated.operation.order),old.operation.order);
 assert.deepEqual(plain(updated.inputs.map(({id,name})=>({id,name}))),old.inputs.map(({id,name})=>({id,name})));
 assert.deepEqual(plain(next.document.bindings),before.document.bindings);assert.deepEqual(plain(value),before);
});

test('variadic OR, AND and priority append valid typed inputs without mutating their source node',()=>{
 const booleanContract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
 for(const preset of ['any','all','priority']){
  let node=processing.processingTemplate(preset,`logic-${preset}`,booleanContract,booleanContract);
  const original=node,before=plain(node);
  for(const count of [3,4]){
   node=chain.addChainInput(node);
   assert.equal(node.id,before.id);assert.equal(node.name,before.name);assert.equal(node.operation.kind,preset);
   assert.equal(node.inputs.length,count);assert.equal(processing.processingNodeError(node),'');
   assert.deepEqual(plain(node.inputs.at(-1).contract),plain(booleanContract));
   assert.equal(node.inputs.at(-1).direction,'input');
   if(preset==='priority')assert.deepEqual(node.operation.order,node.inputs.map(input=>input.id));
  }
  assert.deepEqual(plain(original),before);
 }
});

test('variadic input IDs avoid collisions and unsupported nodes or the eight-input limit are rejected',()=>{
 const booleanContract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
 const priority=processing.processingTemplate('priority','priority',booleanContract,booleanContract);
 priority.inputs[1].id='input-3';priority.operation.order[1]='input-3';
 const expanded=chain.addChainInput(priority);
 assert.equal(expanded.inputs[2].id,'input-4');assert.equal(expanded.inputs[2].name,'Input 4');
 assert.deepEqual(plain(expanded.operation.order),['first','input-3','input-4']);
 assert.deepEqual(plain(priority.inputs.map(input=>input.id)),['first','input-3']);
 const gate=processing.processingTemplate('gate','gate',booleanContract,booleanContract);
 assert.throws(()=>chain.addChainInput(gate),/only supported for Boolean OR, Boolean AND, and Explicit input priority/);
 let full=processing.processingTemplate('any','full',booleanContract,booleanContract);
 while(full.inputs.length<8)full=chain.addChainInput(full);
 assert.equal(processing.processingNodeError(full),'');
 assert.throws(()=>chain.addChainInput(full),/maximum of 8 inputs/);
});

test('removing a variadic input reduces OR, AND, and priority nodes to two without mutating the source',()=>{
 const booleanContract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
 for(const preset of ['any','all','priority']){
  let node=processing.processingTemplate(preset,`logic-${preset}`,booleanContract,booleanContract);
  node=chain.addChainInput(node);
  const before=plain(node),removedId=node.inputs[2].id;
  const next=chain.removeChainInput(node,removedId);
  assert.equal(next.inputs.length,2);assert.deepEqual(plain(next.inputs.map(({id,name})=>({id,name}))),before.inputs.slice(0,2).map(({id,name})=>({id,name})));
  assert.equal(processing.processingNodeError(next),'');
  if(preset==='priority')assert.deepEqual(plain(next.operation.order),before.operation.order.filter(id=>id!==removedId));
  assert.deepEqual(plain(node),before);
 }
});

test('removal preserves later input identities and names and filters only the removed priority key',()=>{
 const booleanContract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
 let node=processing.processingTemplate('priority','priority-four',booleanContract,booleanContract);
 node=chain.addChainInput(chain.addChainInput(node));
 node.inputs.forEach((input,index)=>input.name=['Pilot','Copilot','Ground crew','Autopilot'][index]);
 node.operation.order=['input-4','second','input-3','first'];
 const before=plain(node),next=chain.removeChainInput(node,'second');
 assert.deepEqual(plain(next.inputs.map(({id,name})=>({id,name}))),before.inputs.filter(input=>input.id!=='second').map(({id,name})=>({id,name})));
 assert.deepEqual(plain(next.operation.order),['input-4','input-3','first']);
 assert.equal(processing.processingNodeError(next),'');assert.deepEqual(plain(node),before);
});

test('removal rejects two-input nodes, fixed-arity nodes, and missing input ports',()=>{
 const booleanContract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
 const minimum=processing.processingTemplate('any','minimum',booleanContract,booleanContract);
 assert.throws(()=>chain.removeChainInput(minimum,minimum.inputs[0].id),/at least 2 inputs/);
 const variadic=chain.addChainInput(minimum);
 assert.throws(()=>chain.removeChainInput(variadic,'missing'),/does not exist/);
 const gate=processing.processingTemplate('gate','gate',booleanContract,booleanContract);
 assert.throws(()=>chain.removeChainInput(gate,gate.inputs[0].id),/only supported for Boolean OR, Boolean AND, and Explicit input priority/);
});

test('editing one row instances a shared transform and leaves the other row on its original node',()=>{
 const sourceContract={delivery:'state',schema:{type:'number',min:0,max:100,unit:null},freshnessMs:1000,fallback:null};
 const document={schemaVersion:2,id:'22222222-2222-4222-8222-222222222222',name:'Shared rows',revision:1,eligibility:{gameIds:[]},resources:[
  {id:'source',name:'Source',kind:'device',required:false,ports:[{id:'value',name:'Value',direction:'output',contract:plain(sourceContract)}]},
 ],bindings:[],processingNodes:[processing.processingTemplate('compare','shared-compare',sourceContract)],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[{nodeId:'shared-compare',x:200,y:40}],viewport:{x:0,y:0,zoom:1}}};
 let value={document,assignments:[]};
 for(const id of ['left','right'])value=graph.graphSetVirtual(value,{id,name:id,required:false,layout:{buttons:['button'],axes:[],hats:[]}});
 value=graph.graphConnect(value,{nodeId:'source',portId:'value'},{nodeId:'shared-compare',portId:'in'});
 value=graph.graphConnect(value,{nodeId:'shared-compare',portId:'out'},{nodeId:'left',portId:'button'});
 value=graph.graphConnect(value,{nodeId:'shared-compare',portId:'out'},{nodeId:'right',portId:'button'});
 const lanes=()=>[...new Map(modules.get('./profile-signal-overview').signalOverview(value.document).flatMap(bundle=>bundle.lanes).map(lane=>[lane.id,lane])).values()];
 const leftLane=lanes().find(lane=>lane.destination.nodeId==='left'),rightLane=lanes().find(lane=>lane.destination.nodeId==='right');
 assert.ok(leftLane&&rightLane);
 const result=chain.instanceRowTransforms(value,leftLane.id),copyId=result.nodeIds['shared-compare'];
 assert.ok(copyId);assert.notEqual(copyId,'shared-compare');
 const sharedInputId=value.document.bindings.find(binding=>binding.to.nodeId==='shared-compare').id;
 assert.ok(result.bindingIds[sharedInputId]);
 assert.equal(result.draft.document.bindings.find(binding=>binding.id===rightLane.id).from.nodeId,'shared-compare');
 assert.equal(result.draft.document.bindings.find(binding=>binding.id===leftLane.id).from.nodeId,copyId);
 const original=result.draft.document.processingNodes.find(node=>node.id==='shared-compare');
 const isolated=result.draft.document.processingNodes.find(node=>node.id===copyId);
 const originalThreshold=original.operation.threshold;isolated.operation.threshold=73;
 assert.equal(original.operation.threshold,originalThreshold);
 assert.equal(isolated.operation.threshold,73);
 assert.deepEqual(plain(value.document.processingNodes.find(node=>node.id==='shared-compare').operation.threshold),originalThreshold);
 assert.deepEqual(plain(result.draft.document.editor.positions.find(item=>item.nodeId===copyId)),{nodeId:copyId,x:200,y:40});
 assert.equal(result.draft.document.bindings.length,value.document.bindings.length+1);
});

test('row instancing returns an unchanged clone when transforms are not shared and rejects stale lanes',()=>{
 const value=makeDraft();
 const lane=modules.get('./profile-signal-overview').signalOverview(value.document).flatMap(bundle=>bundle.lanes)[0];
 const result=chain.instanceRowTransforms(value,lane.id);
 assert.deepEqual(plain(result.nodeIds),{});assert.deepEqual(plain(result.bindingIds),{});assert.notEqual(result.draft,value);
 assert.throws(()=>chain.instanceRowTransforms(value,'missing-row'),/current signal row/);
});
