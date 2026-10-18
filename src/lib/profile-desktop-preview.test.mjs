import assert from'node:assert/strict';import test from'node:test';import{loadModules}from'./test-module-loader.mjs';
const modules=await loadModules(['./profile-desktop-preview']),{desktopPreviewFrames,desktopPreviewSources,desktopPreviewResult,previewSourceKey,previewNeutral}=modules.get('./profile-desktop-preview');
const plain=value=>JSON.parse(JSON.stringify(value)),state=(schema={type:'boolean'},fallback=false)=>({delivery:'state',schema,freshnessMs:1000,fallback});
const source=(nodeId,portId,contract=state())=>({endpoint:{nodeId,portId},contract,inventoryResourceId:null,observedPortId:null,token:null,route:null,readiness:'blocked'});
const preview={compilationId:'11111111-1111-4111-8111-111111111111',profileId:'22222222-2222-4222-8222-222222222222',profileRevision:7,programHash:`sha256:${'1'.repeat(64)}`,sectionTotals:{sources:0}};
test('synthetic recording constructs complete per-source neutral baselines and one ordered press/release with explicit monotonic ticks',()=>{
 const sources=[source('panel','one'),source('panel','hat',state({type:'integer',min:-1,max:7,unit:null},-1)),source('encoder','tap',{delivery:'event',schema:{type:'boolean'}})];
 const selected=previewSourceKey(sources[0].endpoint),request=desktopPreviewFrames(sources,selected,true);assert.deepEqual(plain(request.frames.map(frame=>frame.capturedUs)),['0','0','1000','1000000']);assert.deepEqual(plain(request.frames.map(frame=>frame.sequence)),['1','1','2','3']);
 const baseline=request.frames.find(frame=>frame.source==='panel'&&frame.reason==='baseline');assert.equal(baseline.changes.length,2);assert.equal(baseline.changes.find(change=>change.portId==='hat').sample.value.value,-1);assert.equal(request.frames.at(-1).changes[0].sample.value.value,false);assert.equal(request.advances.length,150);assert.equal(request.advances[0],'8000');assert.equal(request.advances.at(-1),'1200000');assert.equal(request.command,undefined);
 assert.equal(previewNeutral(source('axis','x',state({type:'integer',min:-32768,max:32767,unit:null},null))),0);
});
test('out-of-contract, duplicate or non-scalar synthetic inputs cannot cross recording transport',()=>{
 const sources=[source('panel','one')],selected=previewSourceKey(sources[0].endpoint);for(const value of [NaN,Infinity,'true',null,1])assert.throws(()=>desktopPreviewFrames(sources,selected,value),/exact contract/);assert.throws(()=>desktopPreviewFrames([...sources,...sources],selected,true),/unique/);assert.throws(()=>desktopPreviewFrames(sources,'unknown',true),/Choose/);
 assert.throws(()=>desktopPreviewFrames([source('panel','action',{delivery:'invocation',parameters:[],semantics:'one-shot'})],'',true),/scalar/);assert.deepEqual(plain(desktopPreviewFrames([],'',true).frames),[]);
});
test('captured source paging advances by actual count, reads only metadata, and rejects a changed capture',async()=>{
 const sources=Array.from({length:70},(_,index)=>source('panel',`port-${index}`)),offsets=[];const compiler={inspect:async(_preview,section,offset,limit)=>{assert.equal(section,'sources');assert.equal(limit,64);offsets.push(offset);return{total:70,items:sources.slice(offset,offset+limit)};}};
 assert.equal((await desktopPreviewSources(compiler,{...preview,sectionTotals:{sources:70}})).length,70);assert.deepEqual(offsets,[0,64]);await assert.rejects(desktopPreviewSources({inspect:async()=>({total:2,items:[]})},preview),/changed/);
});
test('recorded output must match the exact compiled program and can never claim native submission',()=>{
 const result={...preview,mode:'recording',nativeSubmission:false,operations:[{sequence:'1',operationId:'1',actionId:'route',atUs:'1000',events:[{kind:'key',usage:79,down:true,repeat:false}],collision:false,held:[]}],decisions:[{atUs:'1000',actionId:'route',kind:'hold',detail:'synthetic hold'}],trace:{schemaVersion:1},fault:null,droppedDecisions:'0',limitations:'Synthetic only'};
 const recorded=desktopPreviewResult(result,preview);result.operations[0].collision=true;assert.equal(recorded.operations[0].collision,false);
 for(const changed of [{nativeSubmission:true},{compilationId:'foreign'},{programHash:`sha256:${'2'.repeat(64)}`},{operations:[{...result.operations[0],sequence:1}]},{fault:false},{droppedDecisions:'01'}])assert.throws(()=>desktopPreviewResult({...result,...changed},preview),/does not match/);
});
