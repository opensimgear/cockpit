import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { loadModules } from './test-module-loader.mjs';
const modules=await loadModules(['./profiles','./profile-editor']);
const {DEFAULT_PROFILE_ID,exportProfileDocument}=modules.get('./profiles');
const {ProfileEditorStore,ProfileNavigationGuard,seedProfile,blankProfile,copyProfile,addProfileResources,profileAutoLayout,profileEditorDirty,profileCategory}=modules.get('./profile-editor');
const plain=value=>JSON.parse(JSON.stringify(value));
const uid=n=>`11111111-1111-4111-8111-${String(n).padStart(12,'0')}`;
const fixture=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/mixed.json',import.meta.url),'utf8').catch(()=>readFile(new URL('../../../tests/fixtures/profiles/mixed.json',import.meta.url),'utf8')));
const summary=document=>({id:document.id,name:document.name,revision:document.revision,isDefault:document.id===DEFAULT_PROFILE_ID,semanticHash:`sha256:${'3'.repeat(64)}`});
const savedResult=document=>({document,assignments:[],session:session(),profiles:[summary(document)]});
const session=()=>({selectedProfileId:DEFAULT_PROFILE_ID,selectionMode:'manual',state:'stopped',appliedSnapshot:{profileId:DEFAULT_PROFILE_ID,revision:2,semanticHash:`sha256:${'1'.repeat(64)}`},fault:null});
const resource=(type,n)=>({id:`native:${type}:${n}`,name:`${type} ${n}`,resourceType:type,availability:type==='game-integration'?'unavailable':'online',generation:'1',capabilityRevision:`sha256:${'2'.repeat(64)}`,portCount:type==='game-integration'?20000:20,cachedPortCount:type==='game-integration'?20000:20,connections:[{scope:'fixture',key:String(n),route:{id:`route${n}`,transport:type==='osg-module'?(n%2?'usb':'can'):type==='game-integration'?'integration':'hid',providerGeneration:'boot1',attributes:{gateway:'gateway-1',owner:'fixture'},baselineReady:false,writable:false,priority:1}}],attributes:{gameId:type==='game-integration'?'dcs':'',context:'waiting-profile'},diagnostics:type==='game-integration'?['Waiting for the matching game']:[]});
const inventory=()=>['host-controller','osg-module','companion','game-integration'].flatMap(type=>[resource(type,1),resource(type,2)]);
const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};};
function harness(){
  const docs=new Map([[DEFAULT_PROFILE_ID,{...blankProfile(uid(1),'Default'),id:DEFAULT_PROFILE_ID}]]);let serial=10;const calls=[];
  const api={async list(){calls.push('list');return{profiles:[...docs.values()].map(document=>({id:document.id,name:document.name,revision:document.revision,isDefault:document.id===DEFAULT_PROFILE_ID,semanticHash:'hash'})),session:session(),readOnly:false};},async getAssignments(id){calls.push(['get',id]);return{document:plain(docs.get(id)),assignments:[],session:session()};},async editorSave(document,expectedRevision,assignments){calls.push(['save',plain(document),expectedRevision,plain(assignments)]);const saved={...plain(document),revision:expectedRevision===null?1:expectedRevision+1};docs.set(saved.id,saved);return{document:saved,assignments:plain(assignments),session:session(),profiles:[...docs.values()].map(summary)};}};
  const provider={async listAll(){calls.push('inventory');return{resources:inventory(),inventoryDiagnostics:[]};}};
  return{api,provider,calls,docs,store:new ProfileEditorStore(api,provider,()=>uid(serial++))};
}

test('blank seeding includes every resource category and closed game, with no selected ports, routes or effects',()=>{
  const input=inventory();const seeded=seedProfile(uid(1),input);
  assert.equal(seeded.document.resources.length,8);assert.equal(seeded.assignments.length,8);
  assert.deepEqual(plain(seedProfile(uid(1),input.slice().reverse())),plain(seeded));
  assert.deepEqual(new Set(seeded.document.resources.map(r=>r.kind)),new Set(['device','companion','game']));
  for(const field of ['bindings','processingNodes','virtualOutputs','visibility','overrides','companionAssignments'])assert.equal(seeded.document[field].length,0);
  assert.ok(seeded.document.resources.every(r=>r.ports.length===0));assert.ok(seeded.assignments.every(a=>a.ports.length===0));
  assert.equal(seeded.document.editor.positions.length,8);assert.equal(new Set(seeded.document.editor.positions.map(p=>`${p.x}/${p.y}`)).size,8);
  const exported=exportProfileDocument(seeded.document);for(const r of input)assert.ok(!exported.includes(r.id));assert.ok(!exported.includes('gateway-1'));assert.ok(!exported.includes('providerGeneration'));
  assert.equal(input[6].portCount,20000);assert.equal(profileCategory(input[0]),'Physical controller');assert.equal(profileCategory(input[2]),'OSG peripheral');
});
test('resource capacity is explicit and never silently truncates first-N resources or provider ports',()=>{
  const many=Array.from({length:65},(_,n)=>resource('companion',n));assert.throws(()=>seedProfile(uid(1),many),/65 resources.*64.*Nothing was truncated/);
  const seeded=seedProfile(uid(1),many.slice(0,64));assert.equal(seeded.document.resources.length,64);
  assert.throws(()=>addProfileResources(seeded.document,seeded.assignments,[resource('companion',100)]),/No resources were omitted or added/);
  assert.throws(()=>seedProfile(uid(1),[many[0],many[0]]),/duplicate/);
  assert.throws(()=>blankProfile(DEFAULT_PROFILE_ID),/UUID/);assert.throws(()=>blankProfile('bad'),/UUID/);assert.throws(()=>blankProfile(uid(1),''));
});
test('new discovery adds only explicitly requested resources without erasing saved/offline blocks or placement',()=>{
  const first=seedProfile(uid(1),inventory().slice(0,2));first.document.editor.positions[0]={nodeId:first.document.resources[0].id,x:-400,y:725};
  const second=addProfileResources(first.document,first.assignments,[inventory()[0],resource('osg-module',99)]);
  assert.equal(second.document.resources.length,3);assert.deepEqual(plain(second.document.editor.positions[0]),plain(first.document.editor.positions[0]));
  assert.equal(second.document.resources[0].name,first.document.resources[0].name);assert.equal(first.document.resources.length,2);
  assert.ok(second.document.editor.positions[2].y>725+232);assert.equal(second.document.bindings.length,0);
});
test('copies preserve the entire portable graph and layout with independent identity but no inferred local assignments',()=>{
  const source=plain(fixture);source.name='A'.repeat(80);const copy=copyProfile(source,uid(1));
  assert.equal(copy.revision,1);assert.notEqual(copy.id,source.id);assert.equal([...copy.name].length,80);assert.deepEqual(plain(copy.bindings),source.bindings);assert.deepEqual(plain(copy.editor),source.editor);
  copy.resources[0].name='Changed';assert.notEqual(copy.resources[0].name,source.resources[0].name);
  assert.throws(()=>copyProfile(source,source.id),/independent/);assert.throws(()=>copyProfile(source,'BAD'),/UUID/);assert.deepEqual(plain(profileAutoLayout([])),[]);
});
test('Default-only list and exact editor opening never select, apply or change captured runtime state',async()=>{
  const h=harness();await h.store.refresh();assert.equal(h.store.state.view,'list');assert.equal(h.store.state.profiles[0].id,DEFAULT_PROFILE_ID);
  const snapshots=[];const stop=h.store.subscribe(state=>snapshots.push(state));await h.store.open(DEFAULT_PROFILE_ID);
  assert.equal(h.store.state.editor.document.id,DEFAULT_PROFILE_ID);assert.equal(h.store.state.session.state,'stopped');assert.equal(h.store.state.session.appliedSnapshot.revision,2);
  assert.ok(Object.isFrozen(h.store.state.session.appliedSnapshot));assert.equal(h.store.back(),true);stop();assert.ok(snapshots.length>1);
  assert.deepEqual(h.calls,['list',['get',DEFAULT_PROFILE_ID]]);h.store.dispose();
});
test('Add/Cancel is entirely local; Save creates exactly one atomic graph+assignment entry',async()=>{
  const h=harness();await h.store.refresh();await h.store.refreshInventory();const before=plain(h.calls);h.store.beginBlank();assert.equal(h.store.state.editor.baseRevision,null);assert.equal(profileEditorDirty(h.store.state.editor),true);
  assert.deepEqual(h.calls,before);assert.equal(h.store.back(),false);assert.equal(h.store.state.view,'editor');assert.equal(h.store.back(true),true);assert.equal(h.docs.size,1);assert.deepEqual(h.calls,before);
  h.store.beginBlank();const document=plain(h.store.state.editor.document);document.name='My setup';h.store.update(document);await h.store.save();
  assert.equal(h.docs.size,2);assert.equal(h.store.state.editor.baseRevision,1);assert.equal(profileEditorDirty(h.store.state.editor),false);assert.equal(h.store.state.profiles.length,2);
  const save=h.calls.find(call=>Array.isArray(call)&&call[0]==='save');assert.equal(save[2],null);assert.equal(save[3].length,8);assert.equal(save[1].bindings.length,0);assert.equal(h.store.state.session.selectedProfileId,DEFAULT_PROFILE_ID);assert.equal(h.store.back(),true);
});
test('copy creation, layout edits, unsaved Back and saved reopening preserve graph references without reseeding',async()=>{
  const h=harness();h.docs.set(fixture.id,plain(fixture));await h.store.refresh();await h.store.beginCopy(fixture.id);
  assert.deepEqual(plain(h.store.state.editor.assignments),[]);assert.deepEqual(plain(h.store.state.editor.document.bindings),fixture.bindings);assert.equal(h.docs.size,2);
  const document=plain(h.store.state.editor.document);document.editor.viewport={x:100,y:-20,zoom:0.5};h.store.update(document);assert.equal(h.store.back(),false);await h.store.save();const id=document.id;h.store.back();await h.store.open(id);
  assert.deepEqual(plain(h.store.state.editor.document.editor.viewport),{x:100,y:-20,zoom:0.5});assert.deepEqual(plain(h.store.state.editor.document.bindings),fixture.bindings);assert.equal(h.calls.filter(call=>call==='inventory').length,0);
});
test('save completion preserves newer edits and advances their optimistic base; a switched editor is untouched',async()=>{
  const h=harness();await h.store.open(DEFAULT_PROFILE_ID);const pending=deferred();h.api.editorSave=()=>pending.promise;
  const first=h.store.save();const newer=plain(h.store.state.editor.document);newer.editor.viewport.x=40;h.store.update(newer);
  pending.resolve(savedResult({...plain(h.docs.get(DEFAULT_PROFILE_ID)),revision:2}));await first;
  assert.equal(h.store.state.editor.document.editor.viewport.x,40);assert.equal(h.store.state.editor.document.revision,2);assert.equal(h.store.state.editor.baseRevision,2);assert.equal(profileEditorDirty(h.store.state.editor),true);
  const pending2=deferred();h.api.editorSave=()=>pending2.promise;const second=h.store.save();h.store.back(true);await h.store.beginCopy(DEFAULT_PROFILE_ID);const copyId=h.store.state.editor.document.id;
  pending2.resolve(savedResult({...plain(h.docs.get(DEFAULT_PROFILE_ID)),revision:3}));await second;assert.equal(h.store.state.editor.document.id,copyId);assert.equal(h.store.state.editor.baseRevision,null);
});

test('active save accepts applied and failed runtime outcomes as a clean durable save with the native message',async()=>{
  for(const application of [{state:'applied',error:null},{state:'failed',error:'Profile saved, but its running changes could not be applied: Required controller is offline. Reconnect it, then save again.'}]){
    const h=harness();await h.store.open(DEFAULT_PROFILE_ID);
    const document=plain(h.store.state.editor.document);document.editor.viewport.x=25;h.store.update(document);
    const save=h.api.editorSave;
    h.api.editorSave=async(...args)=>{
      const result=await save(...args);result.saveApplication=application;
      result.session={...session(),state:'running',appliedSnapshot:{...session().appliedSnapshot,revision:application.state==='applied'?2:1}};
      return result;
    };
    await h.store.save();
    assert.equal(h.store.state.error,'');assert.equal(h.store.state.busy,false);
    assert.equal(h.store.state.editor.baseRevision,2);assert.equal(h.store.state.editor.document.revision,2);
    assert.equal(profileEditorDirty(h.store.state.editor),false);assert.equal(h.docs.get(DEFAULT_PROFILE_ID).editor.viewport.x,25);
    assert.equal(h.store.state.session.state,'running');assert.equal(h.store.state.session.appliedSnapshot.revision,application.state==='applied'?2:1);
    assert.equal(h.store.state.saveMessage,application.state==='applied'?'Profile saved and active changes applied.':application.error.replace('its running changes','its active changes'));
    assert.equal(h.store.state.saveNotificationKind,application.state==='applied'?'info':'warning');
    assert.deepEqual(h.calls.map(call=>Array.isArray(call)?call[0]:call),['get','save']);
  }
});

test('newer edits rebase and remain dirty after either successful or failed save-time application',async()=>{
  for(const application of [{state:'applied',error:null},{state:'failed',error:'Profile saved, but its running changes could not be applied: Reconnect the required controller, then save again.'}]){
    const h=harness();await h.store.open(DEFAULT_PROFILE_ID);
    const submitted=plain(h.store.state.editor.document);submitted.editor.viewport.x=10;h.store.update(submitted);
    const pending=deferred();h.api.editorSave=()=>pending.promise;const saving=h.store.save();
    const newer=plain(h.store.state.editor.document);newer.editor.viewport.x=90;h.store.update(newer);
    pending.resolve({...savedResult({...submitted,revision:2}),saveApplication:application});await saving;
    assert.equal(h.store.state.editor.document.editor.viewport.x,90);assert.equal(h.store.state.editor.baseDocument.editor.viewport.x,10);
    assert.equal(h.store.state.editor.document.revision,2);assert.equal(h.store.state.editor.baseRevision,2);
    assert.equal(profileEditorDirty(h.store.state.editor),true);assert.equal(h.store.state.error,'');
    assert.equal(h.store.state.saveMessage,application.state==='applied'?'Profile saved and active changes applied.':application.error.replace('its running changes','its active changes'));
    assert.equal(h.store.state.saveNotificationKind,application.state==='applied'?'info':'warning');
  }
});

test('saving an inactive empty profile needs no inventory or separate Apply request',async()=>{
  const h=harness();h.store.beginBlank([]);
  const save=h.api.editorSave;h.api.editorSave=async(...args)=>({...await save(...args),saveApplication:{state:'inactive',error:null}});
  await h.store.save();
  assert.equal(h.store.state.inventoryLoaded,false);assert.equal(h.store.state.editor.document.resources.length,0);
  assert.equal(profileEditorDirty(h.store.state.editor),false);assert.equal(h.store.state.saveMessage,'Profile saved.');assert.equal(h.store.state.saveNotificationKind,'info');assert.equal(h.store.state.error,'');
  assert.equal(h.store.state.session.selectedProfileId,DEFAULT_PROFILE_ID);assert.equal(h.store.state.session.state,'stopped');
  assert.equal(h.store.state.session.appliedSnapshot.revision,2);
  assert.deepEqual(h.calls.map(call=>Array.isArray(call)?call[0]:call),['save']);
});
test('stale, malformed and failed responses preserve local edits and expose actionable errors',async()=>{
  const h=harness();await h.store.open(DEFAULT_PROFILE_ID);let document=plain(h.store.state.editor.document);document.name='';h.store.update(document);await h.store.save();assert.ok(h.store.state.error);assert.equal(h.calls.filter(c=>Array.isArray(c)&&c[0]==='save').length,0);
  document=plain(h.store.state.editor.document);document.name='Default';h.store.update(document);h.api.editorSave=async()=>{throw new Error('Revision changed. Refresh saved profile.');};await h.store.save();assert.match(h.store.state.error,/Revision changed/);assert.equal(h.store.state.editor.document.id,DEFAULT_PROFILE_ID);
  h.api.editorSave=async()=>({document:{...document,id:uid(99)},assignments:[]});await h.store.save();assert.match(h.store.state.error,/this profile/);
  h.api.getAssignments=async()=>({document:{...document,id:uid(99)},assignments:[]});await h.store.open(DEFAULT_PROFILE_ID);assert.match(h.store.state.error,/returned together/);
  assert.throws(()=>h.store.update({...document,id:uid(10)}),/identity/);assert.throws(()=>h.store.update({...document,revision:100}),/revision/);assert.throws(()=>h.store.update({...document,resources:Array(65).fill({})}));
});
test('late open/list/inventory results cannot overwrite a newer editor or disposed workspace',async()=>{
  const h=harness();const a=deferred(),b=deferred();let calls=0;h.api.getAssignments=()=>++calls===1?a.promise:b.promise;const first=h.store.open(uid(1)),second=h.store.open(uid(2));
  b.resolve({document:blankProfile(uid(2)),assignments:[]});await second;a.resolve({document:blankProfile(uid(1)),assignments:[]});await first;assert.equal(h.store.state.editor.document.id,uid(2));
  const later=deferred();h.provider.listAll=()=>later.promise;const read=h.store.refreshInventory();h.store.dispose();later.resolve({resources:inventory()});await read;assert.equal(h.store.state.inventoryLoaded,false);
});

test('inventory refresh retires removed companions without editing profiles or invalidating unchanged preflight',async()=>{
  const h=harness();let snapshot={session:'native-one',revision:'1',resources:inventory()};
  h.provider.listAll=async()=>plain(snapshot);
  await h.store.refreshInventory();h.store.beginBlank();
  const editor=plain(h.store.state.editor),first=h.store.inventoryRevision;
  await h.store.refreshInventory();assert.equal(h.store.inventoryRevision,first,'Identical polling must keep captured preflight current.');
  const removed=snapshot.resources.find(resource=>resource.resourceType==='companion').id;
  snapshot={...snapshot,revision:'2',resources:snapshot.resources.filter(resource=>resource.id!==removed)};
  await h.store.refreshInventory();assert.equal(h.store.inventoryRevision,first+1);assert.ok(!h.store.state.inventory.some(resource=>resource.id===removed));
  assert.deepEqual(plain(h.store.state.editor),editor,'Removed inventory must not erase saved graph blocks, routes or local assignments.');
  snapshot={...snapshot,session:'native-two'};await h.store.refreshInventory();
  assert.equal(h.store.inventoryRevision,first+2,'A native restart must invalidate previously captured authority even with identical resources.');
});

test('inventory availability changes invalidate preflight once and failed polls preserve last-known resources',async()=>{
  const h=harness();await h.store.refreshInventory();const before=h.store.inventoryRevision,known=plain(h.store.state.inventory),read=h.provider.listAll;
  h.provider.listAll=async()=>{throw Error('Companion provider unavailable');};
  await h.store.refreshInventory();assert.equal(h.store.inventoryRevision,before+1);assert.equal(h.store.state.inventoryLoaded,false);assert.deepEqual(plain(h.store.state.inventory),known);
  await h.store.refreshInventory();assert.equal(h.store.inventoryRevision,before+1,'Repeated identical failures must not invent new inventory changes.');
  h.provider.listAll=read;await h.store.refreshInventory();assert.equal(h.store.inventoryRevision,before+2);assert.equal(h.store.state.inventoryLoaded,true);assert.equal(h.store.state.inventoryError,'');
});
test('missing, mismatched or malformed committed summaries never advance the editor base or fabricate hashes',async()=>{
  for(const mutate of [result=>delete result.profiles,result=>result.profiles=[],result=>result.profiles.push({...result.profiles[0]}),result=>result.profiles[0].name='Wrong',result=>result.profiles[0].revision=99,result=>result.profiles[0].isDefault=false,result=>result.profiles[0].semanticHash='',result=>result.profiles[0].id=uid(99)]){
    const h=harness();await h.store.refresh();await h.store.open(DEFAULT_PROFILE_ID);const current=plain(h.store.state.editor.document);current.editor.viewport.x=75;h.store.update(current);
    h.api.editorSave=async()=>{const result=savedResult({...current,revision:2});mutate(result);return result;};await h.store.save();
    assert.match(h.store.state.error,/exact committed profile summary/);assert.equal(h.store.state.editor.baseRevision,1);assert.equal(h.store.state.editor.document.editor.viewport.x,75);assert.equal(profileEditorDirty(h.store.state.editor),true);assert.equal(h.store.state.profiles[0].semanticHash,'hash');
  }
  const h=harness();await h.store.open(DEFAULT_PROFILE_ID);await h.store.save();assert.equal(h.store.state.profiles[0].semanticHash,summary(h.store.state.editor.document).semanticHash);
});
test('read-only, capacity and inventory failures do not produce partial drafts or writes',async()=>{
  const h=harness();h.store.beginBlank([]);assert.equal(h.store.state.editor.document.resources.length,0);assert.equal(h.store.state.inventoryLoaded,false);assert.deepEqual(h.calls,[]);h.store.back(true);assert.throws(()=>h.store.beginBlank(inventory()),/Refresh resource inventory/);await h.store.refreshInventory();h.api.list=async()=>({profiles:[],readOnly:true,diagnostics:['Corrupt library']});await h.store.refresh();assert.throws(()=>h.store.beginBlank(),/read-only/);assert.throws(()=>h.store.update(blankProfile(uid(1))),/read-only/);
  const other=harness();other.api.list=async()=>({profiles:Array.from({length:32},(_,n)=>({id:uid(n),name:'P',revision:1,isDefault:false,semanticHash:'hash'}))});await other.store.refresh();await other.store.refreshInventory();assert.throws(()=>other.store.beginBlank(),/library is full/);await assert.rejects(()=>other.store.beginCopy(uid(1)),/library is full/);
  other.provider.listAll=async()=>{throw new Error('Inventory changed; refresh');};await other.store.refreshInventory();assert.match(other.store.state.inventoryError,/refresh/);assert.equal(other.store.state.inventory.length,8);assert.equal(other.store.state.editor,null);
  const bad=harness();bad.api.list=async()=>({});await bad.store.refresh();assert.match(bad.store.state.error,/missing/);await bad.store.save();assert.throws(()=>bad.store.addResources([]),/Open a profile/);
});
test('failed inventory refresh retains known blocks but cannot silently seed a new draft from stale cached inventory',async()=>{
  const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const original=plain(h.store.state.editor);h.store.back(true);
  h.provider.listAll=async()=>{throw new Error('Inventory revision changed while paging; refresh');};await h.store.refreshInventory();
  assert.equal(h.store.state.inventoryLoaded,false);assert.equal(h.store.state.inventory.length,8);assert.throws(()=>h.store.beginBlank(),/Refresh resource inventory/);assert.throws(()=>h.store.beginBlank(h.store.state.inventory),/Refresh resource inventory/);assert.equal(h.store.state.editor,null);assert.equal(h.docs.size,1);
  h.provider.listAll=async()=>({resources:[...inventory(),resource('companion',3)]});await h.store.refreshInventory();h.store.beginBlank();assert.equal(h.store.state.editor.document.resources.length,9);assert.equal(original.document.resources.length,8);
});
test('sidebar guard keeps a canceled draft mounted and only discards after the explicit navigation choice',async()=>{
  const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const guard=new ProfileNavigationGuard(h.store);const original=h.store.state.editor;let first=0,second=0;
  assert.equal(guard.request(()=>first++),'confirm');assert.equal(first,0);assert.equal(h.store.state.editor,original);guard.keepEditing();assert.equal(h.store.state.editor,original);
  assert.equal(guard.request(()=>second++),'confirm');guard.discard();assert.equal(first,0);assert.equal(second,1);assert.equal(h.store.state.view,'list');assert.equal(h.docs.size,1);assert.equal(h.store.state.session.selectedProfileId,DEFAULT_PROFILE_ID);assert.equal(h.calls.filter(c=>Array.isArray(c)&&c[0]==='save').length,0);
});
test('clean sidebar navigation and guarded save preserve runtime selection and continue exactly once',async()=>{
  const h=harness();await h.store.refresh();const guard=new ProfileNavigationGuard(h.store);let navigated=0;assert.equal(guard.request(()=>navigated++),'left');assert.equal(navigated,1);
  await h.store.open(DEFAULT_PROFILE_ID);assert.equal(guard.request(()=>navigated++),'left');assert.equal(navigated,2);assert.equal(h.store.state.editor,null);
  await h.store.refreshInventory();h.store.beginBlank();assert.equal(guard.request(()=>navigated++),'confirm');assert.equal(await guard.save(),true);assert.equal(navigated,3);assert.equal(h.docs.size,2);assert.equal(h.store.state.session.state,'stopped');assert.equal(h.store.state.session.appliedSnapshot.revision,2);
  assert.equal(await guard.save(),false);assert.equal(navigated,3);
});
test('failed guarded saves stay in the editor and show the native error before retrying the same continuation',async()=>{
  const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const guard=new ProfileNavigationGuard(h.store);let navigated=0;guard.request(()=>navigated++);const save=h.api.editorSave;
  h.api.editorSave=async()=>{throw new Error('Storage is unavailable; preserve this draft.');};assert.equal(await guard.save(),false);assert.equal(navigated,0);assert.match(h.store.state.error,/preserve this draft/);assert.equal(h.store.state.view,'editor');
  h.api.editorSave=save;assert.equal(await guard.save(),true);assert.equal(navigated,1);assert.equal(h.store.state.view,'list');
});
test('newer edits during guarded save stay dirty and a busy navigation cannot replace its pending destination',async()=>{
  const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const guard=new ProfileNavigationGuard(h.store);let original=0,replacement=0;guard.request(()=>original++);
  const save=h.api.editorSave,pending=deferred();h.api.editorSave=()=>pending.promise;const saving=guard.save();assert.equal(guard.request(()=>replacement++),'busy');assert.equal(await guard.save(),false);
  const current=h.store.state.editor;assert.equal(guard.discard(),false);assert.equal(h.store.state.editor,current);assert.equal(h.store.state.busy,true);assert.equal(original,0);
  const newer=plain(h.store.state.editor.document);newer.name='Newer edits';h.store.update(newer);pending.resolve({...savedResult({...newer,name:'New profile',revision:1}),assignments:plain(h.store.state.editor.assignments)});assert.equal(await saving,false);assert.equal(original,0);assert.equal(replacement,0);assert.equal(h.store.state.editor.document.name,'Newer edits');assert.equal(h.store.state.editor.baseRevision,1);
  h.api.editorSave=save;assert.equal(await guard.save(),true);assert.equal(original,1);assert.equal(replacement,0);
});
test('canceling a pending navigation or replacing its editor during Save cannot close the retained or replacement editor',async()=>{
  for(const keep of [true,false]){
    const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const guard=new ProfileNavigationGuard(h.store);let navigated=0;guard.request(()=>navigated++);
    const submitted=plain(h.store.state.editor.document),assignments=plain(h.store.state.editor.assignments),pending=deferred();h.api.editorSave=()=>pending.promise;const saving=guard.save();
    if(keep)guard.keepEditing();else{h.store.back(true);await h.store.open(DEFAULT_PROFILE_ID);}
    const retainedId=h.store.state.editor.document.id;pending.resolve({...savedResult(submitted),assignments});assert.equal(await saving,false);assert.equal(navigated,0);assert.equal(h.store.state.view,'editor');assert.equal(h.store.state.editor.document.id,retainedId);
    assert.equal(guard.awaitingChoice,false);
  }
});
