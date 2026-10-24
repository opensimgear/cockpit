import test from 'node:test';
import assert from 'node:assert/strict';
import {nativeCacheStore,publicationCacheStore} from './persistence/cache-storage.ts';

function host(){
  let revision=0,payload=null,error=null;
  return {
    setError(value){error=value;},
    async invoke(command,{request}){
      assert.equal(command,'companion_cache_request');
      if(request.op==='read')return {revision,payload,error};
      assert.equal(request.op,'write');
      if(request.expectedRevision!==revision)throw new Error('revision conflict');
      payload=request.payload;revision++;error=null;return {revision,payload:null,error:null};
    },
  };
}
test('native acknowledged snapshots retain revisions and stale windows cannot restore cleared content',async()=>{
  const owner=host(),first=nativeCacheStore(owner.invoke),stale=nativeCacheStore(owner.invoke);
  await assert.rejects(first.write('unread'),/Read/);assert.equal(await first.read(),null);assert.equal(await stale.read(),null);
  await first.write('saved');await assert.rejects(stale.write('old'),/conflict/);
  assert.equal(await stale.read(),'saved');await first.write('cleared');await assert.rejects(stale.write('resurrect'),/conflict/);
  assert.equal(await nativeCacheStore(owner.invoke).read(),'cleared');
});
test('corruption remains an error and explicit clear uses its observed revision',async()=>{
  const owner=host();owner.setError('Invalid offline cache');const store=nativeCacheStore(owner.invoke);
  await assert.rejects(store.read(),/Invalid offline/);await store.write('{"entries":[],"selectedKey":""}');assert.equal(await store.read(),'{"entries":[],"selectedKey":""}');
});
test('native errors never instantiate browser storage and preview explicitly chooses its browser adapter',async()=>{
  let opened=0;const browser=()=>{opened++;return {read:async()=>null,write:async()=>{}};};
  const store=publicationCacheStore({native:true,invoke:async()=>{throw Error('SQLite unavailable');},browser});
  await assert.rejects(store.read(),/SQLite/);assert.equal(opened,0);
  await publicationCacheStore({native:false,invoke:async()=>{throw Error('native');},browser}).read();assert.equal(opened,1);
  const preview=publicationCacheStore({invoke:async()=>{throw Error('native');},browser});await preview.read();assert.equal(opened,2);
});
test('malformed acknowledgements and failed writes cannot advance an earlier revision',async()=>{
  for(const snapshot of [{revision:-1},{revision:1.5},{revision:0,payload:1,error:null}]){
    await assert.rejects(nativeCacheStore(async()=>snapshot).read(),/Invalid native/);
  }
  let reply={revision:5,payload:null,error:null},requests=[];
  const store=nativeCacheStore(async(_,{request})=>{requests.push(request);return reply;});await store.read();
  await assert.rejects(store.write('first'),/acknowledged/);reply={revision:6,payload:null,error:'Disk full'};await assert.rejects(store.write('second'),/Disk full/);
  reply={revision:6,payload:null,error:null};await store.write('third');assert.deepEqual(requests.slice(1).map(request=>request.expectedRevision),[5,5,5]);
});
