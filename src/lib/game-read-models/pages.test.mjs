import test from 'node:test';
import assert from 'node:assert/strict';
import {GamePages,MAX_GAME_BLOB_BYTES,capturePageReference,assetReference,blobs,deferred,until} from './test-fixture.mjs';

test('generic trees validate recursive leaf/branch data, preserve identities and leave artwork opaque',async()=>{
  const f=blobs(),name=f.add('Café 🎮','text'),image=f.add('data:image/jpeg;base64,/9j/','asset');
  const record=f.add({type:'leaf',entries:[['id','first'],['name',name],['image',image]]},'object');
  const left=f.add({type:'leaf',items:[record]},'array'),right=f.add({type:'leaf',items:[{id:'second'}]},'array');
  const root=f.add({type:'branch',chunks:[left,right]},'array'),pages=new GamePages(f.get),signal=new AbortController().signal;
  const first=await pages.resolve({rows:root},signal),second=await pages.resolve({rows:root},signal);
  assert.equal(first.rows,second.rows);assert.equal(first.rows[0].name,'Café 🎮');assert.equal(first.rows[0].image,`game-asset:${image.$gamePage.id.slice(7)}:${image.$gamePage.bytes}`);
  assert.equal(f.calls.some(call=>call.id===image.$gamePage.id),false);assert.equal(Object.isFrozen(first.rows[0]),true);
  assert.equal(await pages.resolveAsset(image.$gamePage,signal),'data:image/jpeg;base64,/9j/');
  assert.equal(assetReference(first.rows[0].image).id,image.$gamePage.id);
  const objects=f.add({type:'branch',chunks:[record,f.add({type:'leaf',entries:[['another',1]]},'object')]},'object');
  assert.equal((await pages.resolve(objects,signal)).another,1);
});

test('UTF-8 multi-chunk content verifies bytes and digest before it can be used',async()=>{
  const f=blobs(),ref=f.add('🎮café'.repeat(9000),'text'),pages=new GamePages(f.get);
  assert.equal(await pages.resolve(ref,new AbortController().signal),'🎮café'.repeat(9000));assert.ok(f.calls.length>4);
  for(const change of [page=>({...page,id:`sha256:${'0'.repeat(64)}`}),page=>({...page,total:page.total+1}),page=>({...page,nextOffset:0}),page=>({...page,chunk:page.chunk+'x'}),page=>({...page,extra:true})]){
    const invalid=new GamePages(async(...args)=>change(await f.get(...args)));await assert.rejects(invalid.resolve(ref,new AbortController().signal));
  }
  const wrong=f.add('good','text');f.values.set(wrong.$gamePage.id,new TextEncoder().encode('evil'));
  await assert.rejects(new GamePages(f.get).resolve(wrong,new AbortController().signal));
});

test('last cancellation aborts shared loads; cancelled consumers cannot poison surviving consumers',async()=>{
  const f=blobs(),ref=f.add('shared','text'),gate=deferred();let requests=0;
  const pages=new GamePages(async(...args)=>{requests++;await gate.promise;return f.get(...args);}),a=new AbortController(),b=new AbortController();
  const one=pages.resolve(ref,a.signal),two=pages.resolve(ref,b.signal);await until(()=>requests===1);a.abort();await assert.rejects(one);gate.resolve();assert.equal(await two,'shared');assert.equal(requests,1);
  const next=f.add('cancel','text'),hold=deferred();let called;
  const cancelledPages=new GamePages(async(...args)=>{called=args[2];await hold.promise;return f.get(...args);}),controller=new AbortController();
  const pending=cancelledPages.resolve(next,controller.signal);await until(()=>called);controller.abort();await assert.rejects(pending);assert.equal(called.aborted,true);hold.resolve();
  await assert.rejects(pages.resolve(ref,a.signal));
});

test('bounded concurrent native loads and a full cache reuse content without source polling',async()=>{
  const f=blobs(),gate=deferred();let active=0,maximum=0,started=0;
  const pages=new GamePages(async(...args)=>{active++;started++;maximum=Math.max(maximum,active);await gate.promise;try{return await f.get(...args);}finally{active--;}});
  const jobs=Array.from({length:12},(_,index)=>pages.resolve(f.add(`source-${index}`,'text'),new AbortController().signal));
  await until(()=>started===8);assert.equal(maximum,8);gate.resolve();await Promise.all(jobs);assert.equal(maximum,8);
});

test('malformed manifests, JSON trees, duplicate keys, kind changes and non-JPEG assets fail explicitly',async()=>{
  for(const value of [{pages:{}},{$gamePage:{id:'bad',bytes:4,kind:'text'}},{$gamePage:{id:`sha256:${'1'.repeat(64)}`,bytes:MAX_GAME_BLOB_BYTES+1,kind:'text'}},{$gamePage:{id:`sha256:${'1'.repeat(64)}`,bytes:4,kind:'other'}},{$gamePage:{id:`sha256:${'1'.repeat(64)}`,bytes:4,kind:'text',extra:true}}])assert.throws(()=>capturePageReference(value));
  for(const reference of ['game-asset:bad:10',`game-asset:${'1'.repeat(64)}:01`])assert.throws(()=>assetReference(reference));
  const f=blobs(),pages=new GamePages(f.get),signal=new AbortController().signal;
  for(const [value,kind]of [['not JSON','array'],[{type:'wrong',items:[]},'array'],[{type:'leaf',entries:[['x',1],['x',2]]},'object'],[{type:'branch',chunks:[]},'array'],[{type:'leaf',entries:[['x']]},'object']])await assert.rejects(pages.resolve(f.add(value,kind),signal));
  const image=f.add('data:image/svg+xml;base64,AAAA','asset');await assert.rejects(pages.resolveAsset(image.$gamePage,signal));
  const text=f.add('cached','text');await pages.resolve(text,signal);await assert.rejects(pages.resolve({$gamePage:{...text.$gamePage,bytes:7}},signal));
  let nested=1;for(let index=0;index<66;index++)nested={child:nested};await assert.rejects(pages.resolve(nested,signal));
  await assert.rejects(pages.resolve({value:Infinity},signal));
});

test('paged collections accept current large catalogues and discovery reviews beyond old transport caps',async()=>{
  const f=blobs(),chunks=[];
  for(let part=0;part<16;part++)chunks.push(f.add({type:'leaf',items:Array.from({length:256},(_,index)=>({id:`entry-${part*256+index}`,message:'x'.repeat(1000)}))},'array'));
  const root=f.add({type:'branch',chunks},'array'),value=await new GamePages(f.get).resolve(root,new AbortController().signal);
  assert.equal(value.length,4096);assert.equal(value[4095].id,'entry-4095');assert.ok(f.calls.length>400);
});
