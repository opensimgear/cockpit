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

test('incompatible output contracts are omitted and replacement fails without changing the draft',()=>{
 const value=makeDraft(),before=plain(value);
 assert.ok(!chain.chainTransformOptions(value,'middle').some(choice=>choice.value==='compare'));
 assert.throws(()=>chain.replaceChainTransform(value,'middle','compare'),/preserve.*route contracts/);
 assert.deepEqual(plain(value),before);
});

test('multi-input and branched transforms are not offered or replaced',()=>{
 const value=makeDraft(),node=value.document.processingNodes.find(item=>item.id==='middle');
 node.inputs.push({...plain(node.inputs[0]),id:'second'});
 assert.equal(chain.chainTransformOptions(value,'middle').length,0);
 assert.throws(()=>chain.replaceChainTransform(value,'middle','identity'),/Branched or multi-input/);
 const branch=makeDraft();
 branch.document.bindings.push({id:'route-branch',from:{nodeId:'middle',portId:'out'},to:{nodeId:'target',portId:'value'}});
 assert.equal(chain.chainTransformOptions(branch,'middle').length,0);
 assert.throws(()=>chain.replaceChainTransform(branch,'middle','identity'),/Branched or multi-input/);
});

test('custom transform names survive compatible replacement',()=>{
 const value=makeDraft();value.document.processingNodes.find(node=>node.id==='middle').name='Altitude calibration';
 const next=chain.replaceChainTransform(value,'middle','scale');
 assert.equal(next.document.processingNodes.find(node=>node.id==='middle').name,'Altitude calibration');
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
