import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profiles','./profile-inventory','./profile-graph','./profile-graph-history','./profile-editor']);
const {ProfileGraphHistory,ProfileEditorUndo}=modules.get('./profile-graph-history');
const {ProfileEditorStore}=modules.get('./profile-editor');
const plain=value=>JSON.parse(JSON.stringify(value));
const initial=()=>({document:{schemaVersion:2,id:'11111111-1111-4111-8111-111111111111',name:'Graph',revision:1,eligibility:{gameIds:[]},resources:[],bindings:[],processingNodes:[],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}},assignments:[]});

test('groups, collapsed categories and deleting their last member remain undoable across saved revisions',()=>{
 const value=initial();value.document.resources=['a','b'].map(id=>({id,name:id,kind:'device',required:false,ports:[]}));
 const history=new ProfileGraphHistory(value),grouped=plain(history.current);grouped.document.editor.groups=[{id:'controls',name:'Controls',members:['a','b']}];history.commit(grouped,'Group controls');
 const collapsed=plain(history.current);collapsed.document.editor.collapsedPortGroups=[{nodeId:'a',group:'outputs'},{nodeId:'b',group:'actions'}];history.commit(collapsed,'Collapse categories');history.rebaseRevision(2);
 assert.equal(history.undo().document.editor.collapsedPortGroups,undefined);assert.equal(history.current.document.revision,2);assert.equal(history.undo().document.editor.groups,undefined);assert.equal(history.redo().document.editor.groups[0].name,'Controls');history.redo();
 const {graphRemoveNode}=modules.get('./profile-graph'),removed=graphRemoveNode(history.current,'a');history.commit(removed,'Remove first member');assert.deepEqual(plain(history.current.document.editor.groups[0].members),['b']);assert.deepEqual(plain(history.current.document.editor.collapsedPortGroups),[{nodeId:'b',group:'actions'}]);
 history.commit(graphRemoveNode(history.current,'b'),'Remove last member');assert.equal(history.current.document.editor.groups,undefined);assert.equal(history.current.document.editor.collapsedPortGroups,undefined);history.undo();assert.equal(history.current.document.editor.groups.length,1);assert.equal(history.current.document.resources.length,1);history.undo();assert.deepEqual(plain(history.current.document.editor),collapsed.document.editor);history.redo();assert.equal(history.current.document.resources.length,1);
});

test('history undoes and redoes independent semantic and layout changes with readable labels',()=>{
  const history=new ProfileGraphHistory(initial());assert.equal(history.canUndo,false);assert.equal(history.canRedo,false);assert.equal(history.undoLabel,'');assert.equal(history.redoLabel,'');
  const renamed=plain(history.current);renamed.document.name='Renamed';history.commit(renamed,'Rename profile');const moved=plain(history.current);moved.document.editor.viewport={x:40,y:-10,zoom:.5};history.commit(moved,'Move canvas');
  assert.equal(history.undoLabel,'Move canvas');assert.equal(history.undo().document.name,'Renamed');assert.equal(history.current.document.editor.viewport.x,0);assert.equal(history.redoLabel,'Move canvas');assert.equal(history.undo().document.name,'Graph');assert.equal(history.undo().document.name,'Graph');assert.equal(history.redo().document.name,'Renamed');assert.equal(history.redo().document.editor.viewport.x,40);assert.equal(history.redo().document.editor.viewport.x,40);
  renamed.document.name='Caller mutation';assert.equal(history.current.document.name,'Renamed');assert.ok(Object.isFrozen(history.current.document.editor.viewport));assert.throws(()=>history.current.document.name='Mutable',TypeError);
});
test('undo followed by an edit discards only the abandoned future and no-op updates create no entries',()=>{
  const history=new ProfileGraphHistory(initial());const next=plain(history.current);next.document.name='First';history.commit(next,'First');next.document.name='Second';history.commit(next,'Second');history.undo();
  const branch=plain(history.current);branch.document.name='Branch';history.commit(branch,'Branch');assert.equal(history.canRedo,false);assert.equal(history.undo().document.name,'First');history.commit(history.current,'No operation');assert.equal(history.canRedo,true);assert.throws(()=>history.commit({...branch,document:{...branch.document,name:'Third'}},' '),/Describe/);
});
test('bounded entry and byte budgets drop oldest history without changing the current graph',()=>{
  const history=new ProfileGraphHistory(initial(),3);for(let i=0;i<8;i++){const next=plain(history.current);next.document.name=`Name ${i}`;history.commit(next,'Rename');}assert.equal(history.current.document.name,'Name 7');history.undo();history.undo();assert.equal(history.canUndo,false);assert.equal(history.current.document.name,'Name 5');
  const bytes=new ProfileGraphHistory(initial(),64,1024);for(let i=0;i<5;i++){const next=plain(bytes.current);next.document.name=`Name ${i}`;next.document.resources=[{id:'resource-one',name:'A'.repeat(80),kind:'device',required:false,ports:[]},{id:'resource-two',name:'B'.repeat(80),kind:'device',required:false,ports:[]}];bytes.commit(next,'Rename');}assert.equal(bytes.current.document.name,'Name 4');assert.equal(bytes.canUndo,false);
  assert.throws(()=>new ProfileGraphHistory(initial(),1),/bounded/);assert.throws(()=>new ProfileGraphHistory(initial(),64,Infinity),/bounded/);
});
test('Save revision rebases every undo/redo snapshot without changing graph semantics or native authority',()=>{
  const history=new ProfileGraphHistory(initial());const next=plain(history.current);next.document.name='Edited';history.commit(next,'Rename');history.rebaseRevision(3);assert.equal(history.current.document.revision,3);assert.equal(history.undo().document.revision,3);assert.equal(history.current.document.name,'Graph');assert.equal(history.redo().document.revision,3);assert.equal(history.current.document.name,'Edited');assert.throws(()=>history.rebaseRevision(2),/newer saved revision/);assert.throws(()=>history.rebaseRevision(NaN),/newer saved revision/);
});
test('editor identity/revision replacement requires reset and cannot enter another profile history',()=>{
  const history=new ProfileGraphHistory(initial());const other=initial();other.document.id='22222222-2222-4222-8222-222222222222';assert.throws(()=>history.commit(other,'Other profile'),/identity/);const newer=initial();newer.document.revision=2;assert.throws(()=>history.commit(newer,'New revision'),/revision/);history.reset(other);assert.equal(history.current.document.id,other.document.id);assert.equal(history.canUndo,false);
});
function editorHarness(){const document=initial().document,hash=`sha256:${'1'.repeat(64)}`,session={selectedProfileId:document.id,selectionMode:'manual',state:'stopped',appliedSnapshot:{profileId:document.id,revision:1,semanticHash:hash},fault:null},calls=[];let persisted=plain(document);
  const result=doc=>({document:plain(doc),assignments:[],profiles:[{id:doc.id,name:doc.name,revision:doc.revision,isDefault:false,semanticHash:hash}],session});
  const api={async list(){return result(persisted);},async getAssignments(){return result(persisted);},async editorSave(document,revision){calls.push('save');persisted={...plain(document),revision:revision+1};return result(persisted);}};
  const store=new ProfileEditorStore(api,{async listAll(){return{resources:[]};}}),undo=new ProfileEditorUndo(store);return{store,undo,api,calls,result,id:document.id};
}
test('actual editor history covers graph/layout changes, Save revision rebasing and captured runtime isolation',async()=>{
  const h=editorHarness();assert.equal(h.undo.status.canUndo,false);assert.equal(h.undo.step('undo'),false);assert.throws(()=>h.undo.edit(initial().document),/Open a profile/);await h.store.open(h.id);const applied=h.store.state.session.appliedSnapshot;assert.equal(h.undo.status.canUndo,false);
  const renamed=plain(h.store.state.editor.document);renamed.name='Rally';h.undo.edit(renamed,undefined,'Rename profile');const moved=plain(h.store.state.editor.document);moved.editor.viewport={x:120,y:-40,zoom:.7};h.undo.edit(moved,[],'Move canvas');assert.equal(h.undo.status.undoLabel,'Move canvas');
  await h.store.save();assert.equal(h.store.state.editor.baseRevision,2);assert.equal(h.undo.status.canUndo,true);assert.equal(h.undo.step('undo'),true);assert.equal(h.store.state.editor.document.revision,2);assert.equal(h.store.state.editor.document.editor.viewport.x,0);assert.equal(h.undo.step('undo'),true);assert.equal(h.store.state.editor.document.name,'Graph');assert.equal(h.undo.step('redo'),true);assert.equal(h.store.state.editor.document.name,'Rally');assert.equal(h.store.state.session.appliedSnapshot.revision,applied.revision);assert.deepEqual(h.calls,['save']);
  h.store.back(true);assert.equal(h.undo.status.canUndo,false);await h.store.open(h.id);assert.equal(h.undo.status.canRedo,false);assert.equal(h.store.state.editor.document.name,'Rally');
});
test('inflight Save history keeps newer edits and cannot undo into another editor epoch',async()=>{
  const h=editorHarness();await h.store.open(h.id);const before=plain(h.store.state.editor.document);before.name='Submitted';h.undo.edit(before,[],'Rename');let resolve;h.api.editorSave=()=>new Promise(done=>resolve=done);const saving=h.store.save();assert.equal(h.undo.step('undo'),false);
  const newer=plain(h.store.state.editor.document);newer.editor.viewport.x=99;h.undo.edit(newer,[],'Newer canvas edit');resolve(h.result({...before,revision:2}));await saving;assert.equal(h.store.state.editor.document.editor.viewport.x,99);assert.equal(h.undo.status.undoLabel,'Newer canvas edit');h.undo.step('undo');assert.equal(h.store.state.editor.document.editor.viewport.x,0);assert.equal(h.store.state.editor.document.revision,2);assert.equal(h.store.state.editor.document.name,'Submitted');
  await h.store.open(h.id);assert.equal(h.undo.status.canUndo,false);const state=h.store.state;const readonly={get state(){return{...state,readOnly:true};},editorEpoch:h.store.editorEpoch,update(){throw new Error('must not update');}};assert.equal(new ProfileEditorUndo(readonly).step('undo'),false);
});
test('selected provider declarations and their local expectations undo/redo together without persistence',async()=>{
  const h=editorHarness();await h.store.open(h.id);const document=plain(h.store.state.editor.document);document.resources=[{id:'source',name:'Controller',kind:'device',required:false,ports:[]}];h.undo.edit(document,[{resourceId:'source',sourceId:'native:controller',ports:[]}],'Add resource');
  const {graphSelectProviderPort}=modules.get('./profile-graph'),observed={descriptor:{id:'button:7',name:'Button 7',direction:'output',contract:{delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null}},contractHash:`sha256:${'1'.repeat(64)}`,routeIds:['hid'],current:true};const selected=graphSelectProviderPort({document:h.store.state.editor.document,assignments:h.store.state.editor.assignments},'source','native:controller',observed);h.undo.edit(selected.document,selected.assignments,'Select exact port');
  h.undo.step('undo');assert.equal(h.store.state.editor.document.resources[0].ports.length,0);assert.equal(h.store.state.editor.assignments[0].ports.length,0);h.undo.step('redo');assert.equal(h.store.state.editor.document.resources[0].ports.length,1);assert.equal(h.store.state.editor.assignments[0].ports[0].observedPortId,'button:7');assert.deepEqual(h.calls,[]);
});
