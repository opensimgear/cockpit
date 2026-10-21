import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-transform-drafts']);
const {createProcessingDraft:create,graphDropProcessing:drop,isProcessingDraft:isDraft,resolveProcessingDraft:resolve,updateProcessingDraft:update,graphUpdateProcessing:configure}=modules.get('./profile-transform-drafts');
const {PROCESSING_PRESETS,processingNodeError}=modules.get('./profile-processing');
const {profileDocumentError}=modules.get('./profiles');
const {graphConnect,graphRemoveNode,graphSetProcessing}=modules.get('./profile-graph');
const plain=value=>JSON.parse(JSON.stringify(value));
const blank=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/blank.json',import.meta.url),'utf8'));
const state=(min=0,max=100,unit='rpm',type='number')=>({delivery:'state',schema:{type,min,max,unit},freshnessMs:270,fallback:null});
const graph=()=>({document:plain(blank),assignments:[]});

test('every palette transformation can be dropped and persisted before connection without pretending its contract is resolved',()=>{
  for(const [preset]of PROCESSING_PRESETS){
    const node=create(preset,`transform-${preset}`),value=graphSetProcessing(graph(),node);
    assert.equal(processingNodeError(node),'',preset);assert.equal(profileDocumentError(value.document),'',preset);
    assert.equal(isDraft(value.document,node.id),preset!=='constant',preset);
    assert.deepEqual(plain(value.document.processingNodes[0]),plain(node));
    if(['scale','clamp','range-map','compare','format'].includes(preset))assert.equal(node.inputs[0].contract.schema.type,'number');
    assert.equal(graphRemoveNode(value,node.id).document.processingNodes.length,0);
  }
});

test('shared native draft fixture matches every generated canvas preset and configured Multiply parameter',async()=>{
  let value=graph();value.document.name='Canvas transformation drafts';for(const [preset]of PROCESSING_PRESETS)value=drop(value,preset,preset==='scale'?{factor:10}:{});
  const fixture=JSON.parse(await readFile(new URL('../../tests/fixtures/profile-authoring/canvas-drafts.json',import.meta.url),'utf8'));
  assert.deepEqual(plain(value.document),fixture);
});

test('Multiply preserves configured factor before wiring and resolves exact source units, delivery, freshness and transformed bounds',()=>{
  const first=drop(graph(),'scale',{factor:10}),id=first.document.processingNodes[0].id;
  assert.equal(first.document.processingNodes[0].operation.factor,10);assert.equal(isDraft(first.document,id),true);
  const resolved=resolve(first,id,state(-20,9000));
  const node=resolved.document.processingNodes[0];assert.equal(node.draft,undefined);assert.equal(isDraft(resolved.document,id),false);
  assert.deepEqual(plain(node.inputs[0].contract),state(-20,9000));
  assert.deepEqual(plain(node.outputs[0].contract),state(-200,90000));
  assert.equal(node.operation.factor,10);assert.equal(first.document.processingNodes[0].draft.preset,'scale');
  assert.throws(()=>resolve(resolved,id,state()),/Only a transformation awaiting/);
  const event={delivery:'event',schema:{type:'integer',min:0,max:20,unit:null}},eventNode=resolve(first,id,event).document.processingNodes[0];
  assert.equal(eventNode.outputs[0].contract.delivery,'event');assert.equal(eventNode.outputs[0].contract.schema.type,'integer');assert.equal(eventNode.outputs[0].contract.schema.max,200);
});

test('configuration survives draft replacement and derives fractional or negative scale ranges safely',()=>{
  let value=drop(graph(),'identity'),id=value.document.processingNodes[0].id;
  value=update(value,id,'scale',{factor:-.5,offset:10});
  value=resolve(value,id,state(-20,100,null,'integer'));
  assert.deepEqual(plain(value.document.processingNodes[0].outputs[0].contract.schema),{type:'number',min:-40,max:20,unit:null});
  assert.equal(processingNodeError(value.document.processingNodes[0]),'');
  for(const params of [{factor:NaN},{factor:Infinity},{factor:1e20},{factor:1,unknown:2}])assert.throws(()=>create('scale','bad',params));
  assert.throws(()=>resolve(drop(graph(),'scale'),'transform-1',{delivery:'state',schema:{type:'boolean'},freshnessMs:50,fallback:null}),/integer or number/);
  const partial=update(drop(graph(),'scale',{factor:10}),'transform-1','scale',{offset:5});assert.equal(partial.document.processingNodes[0].operation.factor,10);assert.equal(partial.document.processingNodes[0].operation.offset,5);
});

test('Clamp is explicit, type preserving and computes saturated output contracts before downstream validation',()=>{
  const value=resolve(drop(graph(),'clamp',{min:0,max:10}),'transform-1',state(-100,100,'g'));
  const node=value.document.processingNodes[0];assert.equal(processingNodeError(node),'');
  assert.deepEqual(plain(node.outputs[0].contract.schema),{type:'number',min:0,max:10,unit:'g'});
  assert.deepEqual(plain(node.operation),{kind:'clamp',min:0,max:10});
  assert.throws(()=>create('clamp','bad',{min:20,max:10}),/minimum/);
  assert.throws(()=>resolve(drop(graph(),'clamp',{min:.5,max:10}),'transform-1',state(-100,100,null,'integer')));
});

test('pending outputs cannot connect and resolved parameter edits reject downstream widening atomically',()=>{
  let value=drop(graph(),'scale',{factor:10}),id='transform-1';
  value.document.resources.push({id:'sink',name:'Sink',kind:'companion',required:false,ports:[{id:'in',name:'Input',direction:'input',contract:state(0,1000)}]});
  assert.throws(()=>graphConnect(value,{nodeId:id,portId:'out'},{nodeId:'sink',portId:'in'}),/needs an input/);
  value=resolve(value,id,state(0,100));value=graphConnect(value,{nodeId:id,portId:'out'},{nodeId:'sink',portId:'in'});
  const edited=plain(value.document.processingNodes[0]);edited.operation.factor=11;
  assert.throws(()=>configure(value,edited),/ranges differ/);assert.equal(value.document.processingNodes[0].operation.factor,10);
  edited.operation.factor=5;const reduced=configure(value,edited);assert.equal(reduced.document.processingNodes[0].outputs[0].contract.schema.max,500);assert.equal(reduced.document.bindings[0].id,value.document.bindings[0].id);
});

test('source-dependent defaults resolve from the selected contract and explicit pending parameters persist',()=>{
  const input=state(100,1000,'m');
  for(const preset of ['compare','hysteresis','range-map','curve']){
    const node=resolve(drop(graph(),preset),'transform-1',input).document.processingNodes[0];
    assert.equal(processingNodeError(node),'',preset);assert.equal(node.inputs[0].contract.schema.min,100,preset);
  }
  const mapping=resolve(drop(graph(),'range-map',{outputMin:0,outputMax:100}),'transform-1',state(0,1000,null));
  assert.equal(mapping.document.processingNodes[0].operation.outputMax,100);assert.equal(mapping.document.processingNodes[0].outputs[0].contract.schema.max,100);
  const conversion=resolve(drop(graph(),'unit-convert',{to:'ft'}),'transform-1',state(0,1000,'m'));
  assert.equal(conversion.document.processingNodes[0].operation.to,'ft');
  const before=drop(graph(),'gate');assert.throws(()=>resolve(before,'transform-1',{delivery:'state',schema:{type:'boolean'},freshnessMs:100,fallback:null},'gate'),/value input first/);
});

test('draft metadata is strict, bounded, not accepted on constants and survives independent document copies',()=>{
  const value=drop(graph(),'scale',{factor:10});
  for(const draft of [null,{preset:'unknown'},{preset:'scale',authority:true},{preset:'scale',configured:['missing']},{preset:'scale',configured:['kind']},{preset:'scale',configured:['factor','factor']}]){const doc=plain(value.document);doc.processingNodes[0].draft=draft;assert.ok(profileDocumentError(doc),JSON.stringify(draft));}
  const copy=plain(value.document);copy.processingNodes[0].operation.factor=20;assert.equal(value.document.processingNodes[0].operation.factor,10);
});
