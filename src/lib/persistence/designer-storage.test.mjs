import test from 'node:test';
import assert from 'node:assert/strict';
import {sqliteDesignerStorage,desktopDesignerStorage} from './designer-storage.ts';

function database(initial=null) {
  let value=initial,revision=initial?1:0,fail;
  const invoke=async(command,{request})=>{
    assert.equal(command,'persistence_workspace');
    if(fail)throw new Error(fail);
    if(request.operation==='read')return {revision,libraryRevision:revision,value:structuredClone(value)};
    assert.equal(request.operation,'write');
    if(request.expectedRevision!==revision||request.expectedLibraryRevision!==revision)throw new Error('Persistence revision conflict');
    value=structuredClone(request.value);return {revision:++revision,libraryRevision:revision};
  };
  return {invoke,get value(){return value;},set fail(error){fail=error;}};
}

test('desktop detection is explicit and native storage starts without browser reads',async()=>{
  const previous=globalThis.__TAURI_INTERNALS__;
  try {
    delete globalThis.__TAURI_INTERNALS__;
    assert.equal(desktopDesignerStorage(),undefined);
    const db=database();globalThis.__TAURI_INTERNALS__={invoke:db.invoke};
    const store=desktopDesignerStorage();assert.ok(store);
    assert.equal(await store.read(),undefined);
    const value={drafts:[{id:'panel'}]};await store.write(value);
    assert.deepEqual(await store.read(),value);
  } finally {
    if(previous)globalThis.__TAURI_INTERNALS__=previous;else delete globalThis.__TAURI_INTERNALS__;
  }
});

test('separate windows cannot acknowledge overwriting a newer workspace',async()=>{
  const db=database({drafts:[]}),first=sqliteDesignerStorage(db.invoke),stale=sqliteDesignerStorage(db.invoke);
  await first.read();await stale.read();await first.write({drafts:['new']});
  await assert.rejects(stale.write({drafts:['stale']}),/revision conflict/);
  assert.deepEqual(db.value,{drafts:['new']});
  assert.deepEqual(await stale.read(),{drafts:['new']});
  await stale.write({drafts:['refreshed']});assert.deepEqual(db.value,{drafts:['refreshed']});
});

test('failed native commits keep the last acknowledged revision and expose their errors',async()=>{
  const db=database(),store=sqliteDesignerStorage(db.invoke);
  await store.read();db.fail='SQLite busy';await assert.rejects(store.write({drafts:['failed']}),/SQLite busy/);
  assert.equal(db.value,null);db.fail=undefined;
  await store.write({drafts:['committed']});assert.deepEqual(db.value,{drafts:['committed']});
  db.fail='permission denied';await assert.rejects(store.read(),/permission denied/);
});

test('invalid or uncertain acknowledgements require a reload',async()=>{
  for(const result of [null,[],{revision:0},{revision:-1,value:null},{revision:1.5,value:null},{revision:Number.MAX_SAFE_INTEGER+1,value:null}]) {
    await assert.rejects(sqliteDesignerStorage(async()=>result).read(),/Invalid designer/);
  }
  for(const result of [null,[]])await assert.rejects(sqliteDesignerStorage(async()=>result).write({}),/Invalid designer/);
  const store=sqliteDesignerStorage(async()=>({revision:4,libraryRevision:4}));
  await assert.rejects(store.write({drafts:[]}),/acknowledgement changed/);
});

test('library conflicts and malformed replies preserve both acknowledged revisions',async()=>{
  let fail=false,writeCount=0;
  const store=sqliteDesignerStorage(async(_,{request})=>{
    if(request.operation==='read')return {revision:7,libraryRevision:12,value:{drafts:[]}};
    assert.equal(request.expectedRevision,7);assert.equal(request.expectedLibraryRevision,12);
    writeCount++;if(!fail)return {revision:8,libraryRevision:-1};
    return {revision:8,libraryRevision:14,value:{drafts:['saved']}};
  });
  await store.read();await assert.rejects(store.write({}),/Invalid designer storage revision/);
  fail=true;assert.deepEqual(await store.write({}),{drafts:['saved']});assert.equal(writeCount,2);
  for(const invalid of [false,1,'value',{revision:1,libraryRevision:1},[],{value:null,revision:0,libraryRevision:-1}])
    await assert.rejects(sqliteDesignerStorage(async()=>invalid).read(),/Invalid designer/);
  for(const invalid of [false,1,'value',[],{revision:1,libraryRevision:NaN}])
    await assert.rejects(sqliteDesignerStorage(async()=>invalid).write({}),/Invalid designer/);
});
