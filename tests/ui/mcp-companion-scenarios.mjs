import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
export async function companionScenarios({page,reader,connect,endpoint,ok,call,mutation,evidence}){
 const credential=await page.invokeMcp('mcp_credential_create',{name:'Live companion acceptance',scope:'write'});
 let client,uploadId,retained=[];
 const publication=JSON.parse(await readFile(new URL('../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json',import.meta.url),'utf8'));
 const bytes=Buffer.from(JSON.stringify(publication));
 try{
  client=await connect(endpoint,credential.token);
  const before=await ok(client,'companion_inventory');retained=before.host.publications.filter(p=>p.uiId==='toggle-native-fixture').map(p=>p.publicationHash);
  assert.ok((await client.listTools()).tools.some(t=>t.name==='companion_publish_commit'));
  assert.ok(!(await reader.listTools()).tools.some(t=>t.name==='companion_publish_commit'));
  await assert.rejects(()=>reader.callTool({name:'companion_pair_offer',arguments:mutation({kind:'browser'})}));
  const offer=await ok(client,'companion_pair_offer',mutation({kind:'browser'}));assert.ok(offer.pairing.browserUrl.includes('#pair='));
  await ok(client,'companion_pair_cancel',mutation({}));
  const begin=await ok(client,'companion_publish_begin',mutation({size:bytes.length}));uploadId=begin.uploadId;
  const wrong=await call(client,'companion_publish_chunk',mutation({uploadId,offset:1,base64:bytes.subarray(0,10).toString('base64')}));assert.equal(wrong.ok,false);
  for(let offset=0;offset<bytes.length;offset+=8192)await ok(client,'companion_publish_chunk',mutation({uploadId,offset,base64:bytes.subarray(offset,offset+8192).toString('base64')}));
  const state=await ok(client,'companion_inventory');const args=mutation({uploadId,saveAsLatest:true,selection:{expectedHostToken:state.host.snapshotToken,companionIds:[]}});
  const committed=await ok(client,'companion_publish_commit',args);uploadId=undefined;assert.equal(committed.selectionCommitted,true);assert.equal(committed.panelSaved.publicationHash,publication.publicationHash);
  assert.deepEqual(await ok(client,'companion_publish_commit',args),committed);
  const native=(await page.native({command:'companion_inventory'})).companions;assert.deepEqual(native.host.instances,committed.host.instances);assert.equal(native.host.snapshotToken,committed.host.snapshotToken);
  const stale=await call(client,'companion_prune',mutation({expectedHostToken:state.host.snapshotToken,hashes:[],panelIds:['toggle-native-fixture']}));assert.equal(stale.error.code,'revision_conflict');
  evidence.companionControls={readerScope:true,oneTimePairing:true,ownedChunkUpload:true,exactOffsets:true,guardedBulkSave:true,operationReplay:true,nativeUiAgrees:true,stalePruneRejected:true};
 }finally{
  try{
   if(uploadId)await call(client,'companion_publish_abort',mutation({uploadId})).catch(()=>{});
   if(client){const state=await ok(client,'companion_inventory');await ok(client,'companion_prune',mutation({expectedHostToken:state.host.snapshotToken,hashes:retained,panelIds:['toggle-native-fixture']}));}
  }finally{await client?.close();await page.invokeMcp('mcp_credential_revoke',{id:credential.credential.id});}
 }
}
