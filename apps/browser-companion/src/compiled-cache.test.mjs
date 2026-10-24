import assert from 'node:assert/strict';
import test from 'node:test';
import { compileWidget } from '../../../packages/widget-compiler/src/compiler.ts';
import { createCompiledPanel } from '../../../packages/widget-runtime/src/panel.ts';
import { canonicalWidget } from '../../../packages/widget-runtime/src/artifact.ts';
import { createCompiledAssetCache, browserCompiledCacheStore } from './compiled-cache.ts';

export async function fixture(color='#48ae7e') {
  const source={id:'fixture-cache',title:'Lamp',size:{width:16,height:16},parameters:[],behavior:{version:1,signals:[{id:'value',source:'feedback',type:'boolean',initial:false}],events:[{id:'feedback',updates:['value']}],regions:[]},visualDomains:[{signal:'value',values:[false,true]}],inputs:[],outputs:[{id:'status',label:'Status',signal:'value',type:'boolean'}],hitRegions:[],scene:[{id:'square',kind:'rectangle',x:0,y:0,width:16,height:16,fill:{op:'if',condition:{ref:'value'},then:{literal:color},else:{literal:'#142330'}}}],motions:[],sounds:[]};
  const bundle=await compileWidget(source,{motion:false,audio:false});
  return createCompiledPanel({id:'fixture-panel',title:'Lamp panel',viewports:[{pageId:'page',title:'Page',width:320,height:320}]},[{id:'lamp',pageId:'page',label:'Lamp',bounds:{x:0,y:0,width:.05,height:.05},bundle}]);
}
const publication=await fixture();
export const delivery=pub=>({canonicalDocument:canonicalWidget(pub.document),assets:structuredClone(pub.assets),publicationHash:pub.publicationHash,capabilityFingerprint:pub.capabilityFingerprint});
function storage(){let value;return {read:async()=>value,write:async next=>{value=next;},get value(){return value;},set value(next){value=next;}};}

test('cold and warm delivery fully verify canonical artifacts and asset closure',async()=>{
  const store=storage(),cache=createCompiledAssetCache(store,{clock:()=>1});
  assert.deepEqual(await cache.haveAssets(),[]);
  assert.deepEqual(await cache.verifyDelivery(delivery(publication)),publication);
  await cache.save(publication);
  assert.deepEqual(await cache.haveAssets(),publication.document.ui.assets.map(a=>a.sha256));
  const reload=createCompiledAssetCache(store);
  assert.deepEqual(await reload.verifyDelivery({...delivery(publication),assets:[]}),publication);
  assert.ok(!store.value.includes('credential'));assert.ok(!store.value.includes('sessionId'));assert.ok(!store.value.includes('behavior'));
  const withoutClaim=delivery(publication);delete withoutClaim.capabilityFingerprint;
  assert.deepEqual(await reload.verifyDelivery(withoutClaim),publication);
});
test('missing, corrupt, mismatched or noncanonical deliveries reject before activation',async()=>{
  const cache=createCompiledAssetCache(storage()),full=delivery(publication);
  await assert.rejects(cache.verifyDelivery({...full,assets:[]}),/uncached/);
  await assert.rejects(cache.verifyDelivery({...full,canonicalDocument:' '+full.canonicalDocument}),/canonical/);
  await assert.rejects(cache.verifyDelivery({...full,document:{}}),/canonical/);
  await assert.rejects(cache.verifyDelivery({...full,publicationHash:'sha256:'+'0'.repeat(64)}),/content hash/);
  await assert.rejects(cache.verifyDelivery({...full,capabilityFingerprint:'sha256:'+'0'.repeat(64)}),/fingerprint/);
  const corrupt=structuredClone(full);corrupt.assets[0].base64='AAAA';await assert.rejects(cache.verifyDelivery(corrupt),/digest|size/);
  await assert.rejects(cache.verifyDelivery({...full,assets:[...full.assets,full.assets[0]]}),/duplicate/);
  await assert.rejects(cache.verifyDelivery({...full,assets:[{assetId:'foreign',base64:'AAAA'}]}),/Unknown/);
  await assert.rejects(cache.verifyDelivery({...full,assets:'invalid'}),/assets/);
  await assert.rejects(cache.verifyDelivery({...full,canonicalDocument:'x'.repeat(1024*1024+1)}),/budget/);
  const legacy={...publication.document,schemaVersion:3};await assert.rejects(cache.verifyDelivery({...full,canonicalDocument:canonicalWidget(legacy)}),/version/);
});
test('untrusted persistent contents are checked before advertising cache hits',async()=>{
  const store=storage(),cache=createCompiledAssetCache(store);await cache.save(publication);
  const valid=JSON.parse(store.value);
  for(const corrupt of [false,'no JSON',{version:7,assets:[]},{...valid,token:'private'},{...valid,assets:[...valid.assets,valid.assets[0]]},{...valid,assets:valid.assets.map(row=>({...row,usedAt:-1}))},{...valid,assets:valid.assets.map(row=>({...row,base64:'AAAA'}))},{...valid,assets:valid.assets.map(row=>({...row,descriptor:{...row.descriptor,mime:'text/html'}}))}]){
    store.value=typeof corrupt==='string'?corrupt:JSON.stringify(corrupt);
    await assert.rejects(createCompiledAssetCache(store).haveAssets());
  }
  store.value='x'.repeat(100);await assert.rejects(createCompiledAssetCache(store,{maxBytes:10}).haveAssets(),/budget/);
  await createCompiledAssetCache(store).clear();assert.deepEqual(await createCompiledAssetCache(store).haveAssets(),[]);
});
test('public cache is bounded, atomically persists and never advertises a failed save',async()=>{
  const store=storage(),cache=createCompiledAssetCache(store,{maxAssets:1,clock:()=>1});
  await cache.save(publication);const first=(await cache.haveAssets())[0];
  const second=await fixture('#ed7033');await cache.save(second);assert.equal((await cache.haveAssets()).length,1);assert.notEqual((await cache.haveAssets())[0],first);
  const tiny=createCompiledAssetCache(storage(),{maxBytes:100});await tiny.save(publication);assert.deepEqual(await tiny.haveAssets(),[]);
  const failure=createCompiledAssetCache({read:async()=>undefined,write:async()=>{throw Error('Quota');}});
  await assert.rejects(failure.save(publication),/Quota/);assert.deepEqual(await failure.haveAssets(),[]);
  await assert.rejects(createCompiledAssetCache(storage(),{clock:()=>NaN}).save(publication),/clock/);
});
test('browser cache storage reports absent, blocked and failed databases',async()=>{
  await assert.rejects(browserCompiledCacheStore(undefined).read(),/unavailable/);
  for(const event of ['onblocked','onerror']){
    const factory={open(){const request={};queueMicrotask(()=>request[event]());return request;}};
    await assert.rejects(browserCompiledCacheStore(factory).read(),/blocked|Cannot open/);
  }
  let saved,closed=false;
  const factory={open(){const db={objectStoreNames:{contains:()=>false},createObjectStore(){},close(){closed=true;},transaction(){const tx={objectStore:()=>({get(){const request={result:saved};queueMicrotask(()=>{request.onsuccess();tx.oncomplete();});return request;},put(value){saved=value;const request={result:undefined};queueMicrotask(()=>{request.onsuccess();tx.oncomplete();});return request;}})};return tx;}};const request={result:db};queueMicrotask(()=>{request.onupgradeneeded();request.onsuccess();db.onversionchange();});return request;}};
  const store=browserCompiledCacheStore(factory);await store.write('public');assert.equal(await store.read(),'public');assert.equal(closed,true);
});
