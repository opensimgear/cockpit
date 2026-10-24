import assert from 'node:assert/strict';
import test from 'node:test';
import { sourceIdentity } from '../../generic-widget-editor.ts';
import { addSignal, addPort, addOutputBinding, addVisualDomain, recognizesSelection, renameSignal, removeSignal, renamePort, removePort, associatePort, changeSignalType, syncSignal } from './authoring.ts';

function fixture(){return {
  id:'test',title:'Test',size:{width:100,height:100},parameters:[],
  behavior:{version:1,signals:[{id:'intent',source:'local',type:'boolean',initial:false},{id:'feedback',source:'feedback',type:'boolean',initial:false},{id:'quality',source:'local',type:'enum',values:['fresh','unknown'],initial:'fresh'}],events:[{id:'activate',updates:['intent']}],regions:[{id:'mode',initial:'off',states:['off','on'],transitions:[{id:'change',from:'off',to:'on',event:'activate',guard:{ref:'intent'},actions:[{type:'set',signal:'intent',value:{literal:true}},{type:'emit',port:'request',value:{ref:'intent'}}]}]}],outputs:[{id:'request',type:'boolean',neutral:false}]},
  inputs:[{id:'request',label:'Request',signal:'intent',type:'boolean'}],outputs:[{id:'observed',label:'Observed',signal:'feedback',type:'boolean'}],visualDomains:[{signal:'intent',values:[false,true]}],hitRegions:[],scene:[{id:'face',kind:'rectangle',x:0,y:0,width:20,height:20,visible:{ref:'intent'}}],
  motions:[{id:'motion',from:{intent:false},to:{intent:true},durationMs:100,fps:10,tracks:[]}],
  design:{nodes:[{id:'derived',label:'Derived',domain:{type:'boolean'},expression:{ref:'intent'}}],owners:[],appearances:[{id:'on',regions:{mode:'on'},layers:{face:{visible:{ref:'intent'}}}}],effects:[]}
};}

test('signal rename rewrites declarations and semantic references without changing literal data',()=>{
  const source=fixture();source.behavior.signals[1].qualitySignal='intent';source.scene[0].fill={literal:'intent'};
  source.design.owners=[{region:'mode',signal:'intent',port:'request',values:{off:false,on:true}}];
  const {source:next,selected}=renameSignal(source,'intent','requested');
  assert.equal(selected,'requested');assert.equal(source.behavior.signals[0].id,'intent');
  assert.equal(next.behavior.signals[1].qualitySignal,'requested');
  assert.deepEqual(next.behavior.events[0].updates,['requested']);
  assert.equal(next.behavior.regions[0].transitions[0].guard.ref,'requested');
  assert.equal(next.behavior.regions[0].transitions[0].actions[0].signal,'requested');
  assert.equal(next.inputs[0].signal,'requested');assert.equal(next.visualDomains[0].signal,'requested');
  assert.equal(next.scene[0].visible.ref,'requested');assert.equal(next.scene[0].fill.literal,'intent');
  assert.equal(next.design.nodes[0].expression.ref,'requested');assert.equal(next.design.owners[0].signal,'requested');
  assert.equal(next.design.appearances[0].layers.face.visible.ref,'requested');
  assert.deepEqual(next.motions[0].from,{requested:false});assert.deepEqual(next.motions[0].to,{requested:true});
  assert.throws(()=>renameSignal(source,'intent','quality'),/unique/);
  assert.throws(()=>renameSignal(source,'intent','bad id'),/unique/);
});

test('deletion protects references and removes unused declarations only',()=>{
  const source=fixture();assert.throws(()=>removeSignal(source,'intent'),/Disconnect/);
  assert.throws(()=>removeSignal(source,'feedback'),/Disconnect/);
  const next=addSignal(source,'local');const deleted=removeSignal(next.source,next.selected);
  assert.equal(deleted.selected,'');assert.deepEqual(deleted.source,source);
  const updates=fixture();updates.inputs=[];updates.visualDomains=[];updates.scene=[];updates.motions=[];updates.design=undefined;updates.behavior.regions=[];
  assert.throws(()=>removeSignal(updates,'intent'),/Disconnect/);
});

test('repairing a duplicate signal ID keeps existing references on the remaining declaration',()=>{
  const source=fixture();source.behavior.signals.push({...source.behavior.signals[0]});
  const next=renameSignal(source,'intent','unique');
  assert.equal(next.source.behavior.signals[0].id,'unique');assert.equal(next.source.behavior.signals.at(-1).id,'intent');
  assert.equal(next.source.inputs[0].signal,'intent');assert.equal(next.source.scene[0].visible.ref,'intent');
});

test('endpoint rename follows emit actions and owner publishing; delete protects writers',()=>{
  const source=fixture();source.design.owners=[{region:'mode',signal:'intent',port:'request',values:{off:false,on:true}}];
  const next=renamePort(source,'inputs','request','publish');
  assert.equal(next.selected,'output:publish');assert.equal(next.source.inputs[0].id,'publish');
  assert.equal(next.source.behavior.outputs[0].id,'publish');assert.equal(next.source.design.owners[0].port,'publish');
  assert.equal(next.source.behavior.regions[0].transitions[0].actions[1].port,'publish');
  assert.throws(()=>removePort(source,'inputs','request'),/Disconnect/);
  assert.equal(removePort(source,'outputs','observed').source.outputs.length,0);
  const standalone=addOutputBinding(source);assert.equal(recognizesSelection(standalone.source,standalone.selected),true);
  assert.equal(removePort(standalone.source,'inputs',standalone.selected.slice(7)).source.behavior.outputs.length,1);
});

test('palette additions select their declarations and create missing profile signals',()=>{
  const source=fixture();source.behavior.signals=[];source.inputs=[];source.outputs=[];source.behavior.outputs=[];source.visualDomains=[];
  for(const kind of ['local','input','feedback']){const added=addSignal(source,kind);assert.equal(added.source.behavior.signals[0].source,kind);assert.equal(recognizesSelection(added.source,added.selected),true);}
  for(const direction of ['inputs','outputs']){const added=addPort(source,direction);assert.equal(added.source[direction].length,1);assert.equal(recognizesSelection(added.source,added.selected),true);}
  const sampled=addVisualDomain(source);assert.equal(recognizesSelection(sampled.source,sampled.selected),true);
  assert.equal(recognizesSelection(source,'missing'),false);
  const extra=addVisualDomain(fixture());assert.equal(extra.selected,'domain:feedback');
});

test('association keeps output type and neutral aligned and preserves state owner exclusivity',()=>{
  const source=fixture();const numeric=addSignal(source,'local');numeric.source.behavior.signals.at(-1).initial=false;
  const changed=associatePort(numeric.source,'inputs','request',numeric.selected);
  assert.equal(changed.inputs[0].signal,numeric.selected);assert.equal(changed.behavior.outputs[0].neutral,false);
  assert.throws(()=>associatePort(source,'inputs','request','feedback'),/local/);
  assert.throws(()=>associatePort(source,'outputs','observed','intent'),/feedback/);
  source.design.owners=[{region:'mode',signal:'intent',port:'request',values:{off:false,on:true}}];
  assert.throws(()=>associatePort(source,'inputs','request','quality'),/state owner/);
  assert.throws(()=>changeSignalType(source,'intent','number'),/ownership/);
});

test('type changes and shared signal edits keep profile domain declarations aligned',()=>{
  const number=changeSignalType(fixture(),'intent','number');
  assert.deepEqual(number.inputs[0],{id:'request',label:'Request',signal:'intent',type:'number',min:0,max:100});
  assert.deepEqual(number.behavior.outputs[0],{id:'request',type:'number',min:0,max:100,neutral:0});
  assert.equal(number.visualDomains[0].samples,11);
  const enumeration=changeSignalType(number,'intent','enum');
  assert.equal(Object.hasOwn(enumeration.inputs[0],'min'),false);assert.deepEqual(enumeration.inputs[0].values,['off','on']);
  const boolean=changeSignalType(enumeration,'intent','boolean');assert.equal(Object.hasOwn(boolean.inputs[0],'values'),false);
  boolean.behavior.signals[0].initial=true;syncSignal(boolean,'intent');assert.equal(boolean.behavior.outputs[0].neutral,true);
});

test('authoring mutations preserve absent optional fields and strict history serialization',()=>{
  const source=fixture();source.behavior.events.push({id:'no-updates'});delete source.behavior.outputs;delete source.design;delete source.motions;
  const before=sourceIdentity(source);
  const renamed=renameSignal(source,'intent','requested').source;
  assert.equal(Object.hasOwn(renamed.behavior.events[1],'updates'),false);assert.doesNotThrow(()=>sourceIdentity(renamed));
  for(const type of ['boolean','number','enum']){
    const changed=changeSignalType(source,'feedback',type);
    const feedback=changed.behavior.signals.find(s=>s.id==='feedback');
    for(const key of ['freshnessMs','qualitySignal','requireGameConfirmed'])assert.equal(Object.hasOwn(feedback,key),false);
    assert.doesNotThrow(()=>sourceIdentity(changed));
  }
  const configured=structuredClone(source);Object.assign(configured.behavior.signals[1],{freshnessMs:500,qualitySignal:'quality',requireGameConfirmed:false});
  const changed=changeSignalType(configured,'feedback','number');assert.equal(changed.behavior.signals[1].freshnessMs,500);assert.equal(changed.behavior.signals[1].qualitySignal,'quality');assert.equal(changed.behavior.signals[1].requireGameConfirmed,false);assert.doesNotThrow(()=>sourceIdentity(changed));
  const empty=structuredClone(source);empty.behavior.regions=[];
  assert.equal(Object.hasOwn(removePort(empty,'inputs','request').source.behavior,'outputs'),false);
  const results=[addSignal(source,'feedback').source,addPort(source,'inputs').source,addPort(source,'outputs').source,addOutputBinding(source).source,addVisualDomain(source).source,renamePort(source,'outputs','observed','received').source,removePort(source,'outputs','observed').source,associatePort(source,'inputs','request','intent')];
  const extra=addSignal(source,'local');results.push(removeSignal(extra.source,extra.selected).source);syncSignal(empty,'intent');results.push(empty);
  for(const result of results)assert.doesNotThrow(()=>sourceIdentity(result));
  assert.equal(sourceIdentity(source),before);
});
