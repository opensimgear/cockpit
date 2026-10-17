import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profiles','./profile-inventory','./profile-graph','./profile-graph-history']);
const {ProfileGraphHistory}=modules.get('./profile-graph-history');
const plain=value=>JSON.parse(JSON.stringify(value));
const initial=()=>({document:{schemaVersion:2,id:'11111111-1111-4111-8111-111111111111',name:'Graph',revision:1,eligibility:{gameIds:[]},resources:[],bindings:[],processingNodes:[],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}},assignments:[]});

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
