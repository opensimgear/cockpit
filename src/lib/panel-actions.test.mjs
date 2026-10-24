import assert from 'node:assert/strict';
import test from 'node:test';
import { newDocument,publishDocument } from '../../packages/companion-designer/src/index.ts';
import { synchronizePanel,removePanelAssignments } from './panel-actions.ts';
async function fixture(){const first=await publishDocument(newDocument('Panel'),[]);const doc=structuredClone(first.document);doc.ui.title='Updated panel';return {first,next:await publishDocument(doc,[])};}
function host(instances=[],publications=[]){
 const calls=[],state={host:{snapshotToken:'native-snapshot:4',instances:structuredClone(instances),publications,clients:['a','b','c'].map(companionId=>({companionId}))}};
 return {calls,state,send:async(command,payload={})=>{calls.push({command,payload});if(command==='companion_publish_begin')return {uploadId:'upload'};if(command==='companion_publish_commit')return {...structuredClone(state),selectionCommitted:true};return structuredClone(state);}};
}
test('Save carries exact observed authority and all deduplicated selections in a single native commit',async()=>{
 const {first,next}=await fixture(),h=host([{instanceId:'one',companionId:'a',publicationHash:first.publicationHash}]);
 await synchronizePanel(next,[first.publicationHash],h.send,['b','c','c']);
 assert.deepEqual(h.calls.find(c=>c.command==='companion_publish_commit').payload,{uploadId:'upload',selection:{expectedHostToken:'native-snapshot:4',companionIds:['b','c']}});
 assert.equal(h.calls.filter(c=>c.command==='companion_status').length,1);assert.equal(h.calls.filter(c=>c.command==='companion_assign'||c.command==='companion_unassign').length,0);
});
test('omitted selection tells native to preserve current panel membership and profile ownership',async()=>{
 const {first,next}=await fixture(),h=host([{instanceId:'profile',companionId:'a',publicationHash:first.publicationHash,profileOwned:true},{instanceId:'direct',companionId:'b',publicationHash:first.publicationHash}]);
 await synchronizePanel(next,[first.publicationHash],h.send);
 assert.deepEqual(h.calls.find(c=>c.command==='companion_publish_commit').payload.selection,{expectedHostToken:'native-snapshot:4'});
 assert.ok(!h.calls.some(c=>c.command==='companion_assign'||c.command==='companion_unassign'));
 const empty=host();await synchronizePanel(next,[],empty.send,[]);assert.deepEqual(empty.calls.find(c=>c.command==='companion_publish_commit').payload.selection.companionIds,[]);
});
test('a missing native acknowledgement or failed profile transition is surfaced without follow-up assignment writes',async()=>{
 const {next}=await fixture();
 for(const response of [{selectionCommitted:false},{selectionCommitted:true,panelSaveApplication:{state:'failed',error:'Release touch first'}}]){
  const h=host(),send=async(command,payload)=>command==='companion_publish_commit'?{...await h.send(command,payload),...response}:h.send(command,payload);
  await assert.rejects(synchronizePanel(next,[],send),/not acknowledged|Release touch/);assert.ok(!h.calls.some(c=>c.command==='companion_assign'||c.command==='companion_unassign'));
 }
});
test('unpaired, ambiguous, incompatible or missing authority fail before upload',async()=>{
 const {first,next}=await fixture();
 const cases=[{h:host(),selected:['missing'],pattern:/no longer paired/},{h:host([{instanceId:'one',companionId:'b',publicationHash:'old-1'},{instanceId:'two',companionId:'b',publicationHash:'old-2'}]),selected:['b'],pattern:/multiple/}];
 const missing=host();delete missing.state.host.snapshotToken;cases.push({h:missing,pattern:/Refresh/});
 const incompatible=host([{instanceId:'one',companionId:'a',publicationHash:first.publicationHash}]);incompatible.state.host.clients[0].kind='esp32';incompatible.state.host.clients[0].screen={width:1280,height:800};const bad=structuredClone(next);bad.document.presentation.viewports[0].backgroundAssetId='image';cases.push({h:incompatible,panel:bad,pattern:/backgrounds/});
 for(const {h,selected,panel,pattern} of cases){await assert.rejects(synchronizePanel(panel??next,[first.publicationHash],h.send,selected),pattern);assert.deepEqual(h.calls.map(c=>c.command),['companion_status']);}
 await assert.rejects(synchronizePanel(next,[],async()=>{throw Error('offline');}),/offline/);
});
test('a stale or failed commit aborts its upload without sequential mutations',async()=>{
 const {next}=await fixture(),h=host();const send=async(command,payload)=>{const result=await h.send(command,payload);if(command==='companion_publish_commit')throw Error('Companion assignments changed. Refresh before saving.');return result;};
 await assert.rejects(synchronizePanel(next,[],send,['a']),/Refresh/);assert.equal(h.calls.at(-1).command,'companion_publish_abort');assert.ok(!h.calls.some(c=>c.command==='companion_assign'||c.command==='companion_unassign'));
});
test('explicit removal touches only matching immutable panel assignments',async()=>{
 const h=host([{instanceId:'one',companionId:'a',publicationHash:'remove'},{instanceId:'two',companionId:'b',publicationHash:'keep'}]);await removePanelAssignments(['remove'],h.send);assert.deepEqual(h.calls.filter(c=>c.command==='companion_unassign').map(c=>c.payload),[{instanceId:'one'}]);
});
