import assert from 'node:assert/strict';
import test from 'node:test';
import { newDocument,publishDocument } from '../../packages/companion-designer/src/index.ts';
import { synchronizePanel,removePanelAssignments } from './panel-actions.ts';
async function fixture(){
 const first=await publishDocument(newDocument('Panel'),[]);
 const doc=structuredClone(first.document);doc.ui.title='Updated panel';
 const next=await publishDocument(doc,[]);
 return {first,next};
}
function host(instances,publications=[]){
 const calls=[];const state={host:{instances:structuredClone(instances),publications,clients:['a','b','c'].map(companionId=>({companionId}))}};
 return {calls,state,send:async(command,payload={})=>{
  calls.push({command,payload});
  if(command==='companion_publish_begin')return {uploadId:'upload'};
  if(command==='companion_unassign')state.host.instances=state.host.instances.filter(i=>i.instanceId!==payload.instanceId);
  if(command==='companion_assign'){state.host.instances=state.host.instances.filter(i=>i.instanceId!==payload.instanceId);state.host.instances.push({...payload});}
  return structuredClone(state);
 }};
}
test('saving updates all assigned companions while leaving unrelated panels alone',async()=>{
 const {first,next}=await fixture();
 const h=host([{instanceId:'one',companionId:'a',publicationHash:first.publicationHash},{instanceId:'two',companionId:'b',publicationHash:first.publicationHash},{instanceId:'other',companionId:'c',publicationHash:'other'}]);
 await synchronizePanel(next,[first.publicationHash],h.send);
 assert.equal(h.calls.filter(c=>c.command==='companion_assign').length,2);
 assert.equal(h.state.host.instances.find(i=>i.instanceId==='other').publicationHash,'other');
 assert.equal(h.state.host.instances.find(i=>i.instanceId==='one').publicationHash,next.publicationHash);
});
test('selection removes assignments, reuses a companion controller and assigns a new companion',async()=>{
 const {first,next}=await fixture();
 const h=host([{instanceId:'one',companionId:'a',publicationHash:first.publicationHash},{instanceId:'two',companionId:'b',publicationHash:'other'}]);
 await synchronizePanel(next,[first.publicationHash],h.send,['b','c','c']);
 assert.ok(!h.state.host.instances.some(i=>i.companionId==='a'));
 assert.equal(h.state.host.instances.find(i=>i.instanceId==='two').publicationHash,next.publicationHash);
 assert.equal(h.state.host.instances.filter(i=>i.companionId==='c').length,1);
});
test('current content is not reassigned, and host panel identity locates older assignments',async()=>{
 const {first,next}=await fixture();
 const h=host([{instanceId:'one',companionId:'a',publicationHash:next.publicationHash},{instanceId:'two',companionId:'b',publicationHash:first.publicationHash}],[{uiId:next.document.ui.id,publicationHash:first.publicationHash}]);
 await synchronizePanel(next,[],h.send);
 assert.equal(h.calls.filter(c=>c.command==='companion_assign').length,1);
});
test('empty selection unassigns the panel and removal only touches matching panels',async()=>{
 const {first,next}=await fixture();
 const h=host([{instanceId:'one',companionId:'a',publicationHash:first.publicationHash},{instanceId:'other',companionId:'b',publicationHash:'other'}]);
 await synchronizePanel(next,[first.publicationHash],h.send,[]);
 assert.deepEqual(h.state.host.instances.map(i=>i.instanceId),['other']);
 await removePanelAssignments(['other'],h.send);assert.equal(h.state.host.instances.length,0);
});
test('upload and assignment ambiguity failures are surfaced without unrelated mutations',async()=>{
 const {next}=await fixture();
 await assert.rejects(synchronizePanel(next,[],async()=>{throw new Error('offline')}),/offline/);
 const h=host([{instanceId:'one',companionId:'b',publicationHash:'old-1'},{instanceId:'two',companionId:'b',publicationHash:'old-2'}]);
 await assert.rejects(synchronizePanel(next,[],h.send,['b']),/multiple/);
 assert.equal(h.calls.filter(c=>c.command==='companion_assign'||c.command==='companion_unassign').length,0);
});

test('an unpaired selection fails preflight before removing existing assignments',async()=>{
 const {first,next}=await fixture();
 const h=host([{instanceId:'one',companionId:'a',publicationHash:first.publicationHash}]);
 await assert.rejects(synchronizePanel(next,[first.publicationHash],h.send,['missing']),/no longer paired/);
 assert.equal(h.calls.filter(c=>c.command==='companion_unassign').length,0);
});

test('incompatible ESP updates fail before uploads or assignment changes',async()=>{
 const {first,next}=await fixture();
 const h=host([{instanceId:'one',companionId:'a',publicationHash:first.publicationHash}]);
 h.state.host.clients[0].kind='esp32';h.state.host.clients[0].screen={width:1280,height:800};
 const bad=structuredClone(next);bad.document.presentation.viewports[0].backgroundAssetId='image';
 await assert.rejects(synchronizePanel(bad,[first.publicationHash],h.send),/backgrounds/);
 assert.deepEqual(h.calls.map(c=>c.command),['companion_status']);
 assert.equal(h.state.host.instances[0].publicationHash,first.publicationHash);
});
