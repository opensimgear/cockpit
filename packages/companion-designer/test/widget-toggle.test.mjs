import test from 'node:test';
import assert from 'node:assert/strict';
import { createWidgetCatalog, includeBuiltinWidgets, validateWidgetDefinition, resolveWidgetLayers, resolveWidgetState, widgetSemantics, newDocument, createDesignerHistory, applyDesignerCommand, viewportRegions, publishDocument, exportBundle, importBundle } from '../src/index.ts';
import { createCapsuleToggle } from '../src/widget-toggle.ts';

for(const vertical of [false,true])test(`${vertical?'vertical':'horizontal'} capsule preserves title, circular thumb, OFF/ON geometry and confirmed feedback`,()=>{
  const definition=createCapsuleToggle(vertical);
  assert.equal(validateWidgetDefinition(definition).ok,true);
  const off=resolveWidgetLayers(definition,{label:'GEAR'},'normal'),on=resolveWidgetLayers(definition,{},'active');
  assert.equal(off.find(l=>l.id==='title').text,'GEAR');
  assert.equal(off.find(l=>l.id==='off-thumb').visible,true);assert.equal(off.find(l=>l.id==='on-thumb').visible,false);
  assert.equal(on.find(l=>l.id==='off-thumb').visible,false);assert.equal(on.find(l=>l.id==='on-thumb').visible,true);
  const a=off.find(l=>l.id==='off-thumb').bounds,b=on.find(l=>l.id==='on-thumb').bounds;
  assert.equal(a.width*definition.size.width,a.height*definition.size.height);
  assert.ok(vertical?a.y>b.y:a.x<b.x);
  for(const state of ['pressed','warning','stale','unknown','disabled','disconnected']){
    const layers=resolveWidgetLayers(definition,{},state);
    assert.equal(layers.find(l=>l.id==='off-thumb').visible,true,state);
    assert.equal(layers.find(l=>l.id==='on-thumb').visible,false,state);
    assert.ok(layers.find(l=>l.id==='off-thumb').opacity<off.find(l=>l.id==='off-thumb').opacity,state);
  }
  const sample={status:'fresh',value:true,provenance:'game-confirmed',sourceKind:'simulator',sourceAgeMs:0,receivedAtMs:100};
  const context={connected:true,enabled:true,pressed:false,now:100,samples:{active:sample}};
  assert.equal(resolveWidgetState(definition,context),'active');
  assert.equal(resolveWidgetState(definition,{...context,samples:{active:{...sample,value:false}}}),'normal');
  assert.notEqual(resolveWidgetState(definition,{...context,samples:{active:{...sample,provenance:'requested',sourceKind:'host'}}}),'active');
  assert.equal(resolveWidgetState(definition,{...context,now:1100}),'stale');
  assert.deepEqual(widgetSemantics(definition,{label:'Renamed',accent:'#ff0000'}),widgetSemantics(definition));
});

test('older saved libraries gain both variants without replacing custom or pinned definitions',()=>{
  const saved=createWidgetCatalog().slice(0,12);saved[1].title='My retained toggle';const before=JSON.stringify(saved);
  const updated=includeBuiltinWidgets(saved);
  assert.equal(updated.length,14);assert.equal(updated[1].title,'My retained toggle');assert.equal(JSON.stringify(saved),before);
  assert.equal(includeBuiltinWidgets(updated).length,14);
  const custom=structuredClone(updated.at(-1));custom.version=4;custom.title='Custom vertical';
  assert.deepEqual(includeBuiltinWidgets([custom]).filter(d=>d.id===custom.id),[custom]);
});

test('saved capsule libraries expose the fixed revision while retaining pinned artwork',()=>{
  const saved=[createCapsuleToggle(false),createCapsuleToggle(true)];
  for(const definition of saved)definition.version=1;
  const before=structuredClone(saved),updated=includeBuiltinWidgets(saved);
  for(const definition of saved){
    assert.deepEqual(updated.find(d=>d.id===definition.id&&d.version===1),definition);
    assert.ok(updated.some(d=>d.id===definition.id&&d.version===2));
  }
  assert.deepEqual(saved,before);
  assert.deepEqual(includeBuiltinWidgets(updated),updated);
});

test('both capsule variants publish and round-trip as editable portable widget definitions',async()=>{
  let history=createDesignerHistory(newDocument('Capsule toggles'));
  for(const vertical of [false,true])history=applyDesignerCommand(history,{type:'add-component',definition:createCapsuleToggle(vertical),properties:{label:vertical?'PWR':'GSAFE'}});
  const publication=await publishDocument(history.present,[]);
  const restored=await importBundle(await exportBundle(publication.document,[]));
  assert.deepEqual(restored.document.presentation.definitions,publication.document.presentation.definitions);
  assert.ok(restored.document.presentation.widgets.filter(w=>w.kind==='component').every(w=>w.config.definitionId.startsWith('osg-capsule-toggle-')));
});

test('new capsule instances retain their authored proportions and fit the selected screen',()=>{
  for(const vertical of [false,true])for(const size of [[800,480],[320,320]]){
    let history=createDesignerHistory(newDocument());const pageId=history.present.ui.pages[0].id;
    history=applyDesignerCommand(history,{type:'set-viewport',pageId,width:size[0],height:size[1]});
    if(size[0]===320)history=applyDesignerCommand(history,{type:'update-viewport',pageId,id:viewportRegions(history.present,pageId)[0].id,patch:{bounds:{x:.1,y:.1,width:.2,height:.2}}});
    const definition=createCapsuleToggle(vertical);
    history=applyDesignerCommand(history,{type:'add-component',definition});
    const b=history.present.presentation.widgets.at(-1).bounds;
    assert.ok(Math.abs(b.width*size[0]/(b.height*size[1])-definition.size.width/definition.size.height)<1e-9);
    assert.ok(b.x>=0&&b.y>=0&&b.x+b.width<=1&&b.y+b.height<=1);
  }
});
