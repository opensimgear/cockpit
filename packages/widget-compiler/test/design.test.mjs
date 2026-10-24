import test from 'node:test';
import assert from 'node:assert/strict';
import {createToggleSource,createLinearGaugeFixture,lowerWidgetDesign,validateWidgetSource,appearanceScene,patchAppearance,compileWidget,canonicalWidget,widgetDigest,designLayers,designValues,emptyDesign,removeDesignLayer,duplicateDesignLayer} from '../src/index.ts';
import {behaviorToggleSource} from './behavior-toggle.mjs';
function capsuleFixture(){const source=createToggleSource();source.design.nodes=[{id:'requested-value',label:'Requested',domain:{type:'boolean'},expression:{op:'eq',left:{state:'requested-state'},right:{literal:'on'}}},{id:'confirmed-value',label:'Confirmed',domain:{type:'boolean'},expression:{ref:'confirmed'}}];return source;}
import {createBehaviorState,reduceBehavior,evaluateExpression} from '../../widget-runtime/src/behavior.ts';

test('capsule design keeps IDs, geometry, public ports and requested/feedback separation',()=>{
  const old=behaviorToggleSource(),source=capsuleFixture(),lowered=lowerWidgetDesign(source);
  assert.equal(validateWidgetSource(source).ok,true,JSON.stringify(validateWidgetSource(source)));
  assert.equal('formatVersion'in source,false);assert.equal('revision'in source,false);assert.deepEqual(source.inputs,old.inputs);assert.deepEqual(source.outputs,old.outputs);
  let state=createBehaviorState(lowered.behavior);
  const result=reduceBehavior(lowered.behavior,state,{type:'event',event:'activate',nowMs:1});
  assert.equal(result.error,undefined);assert.equal(result.state.values.requested,true);assert.equal(result.state.regions['requested-state'],'on');assert.deepEqual(result.effects.filter(e=>e.type==='emit'),[{type:'emit',port:'toggle',value:true}]);
  const feedback=reduceBehavior(lowered.behavior,result.state,{type:'event',event:'feedback',nowMs:2,feedback:{confirmed:{status:'fresh',value:false,provenance:'game-confirmed'}}});
  assert.equal(feedback.state.values.requested,true);assert.equal(feedback.state.values.confirmed,false);
  for(const requested of [false,true]){
    const values={...state.values,requested},regions={...state.regions,'requested-state':requested?'on':'off'};
    for(const original of designLayers(old.scene)){
      const changed=designLayers(lowered.scene).find(n=>n.id===original.id);
      for(const key of ['x','y','width','height','opacity'])if(key in original){const resolve=v=>typeof v==='object'?evaluateExpression(v,{values,regions}):v;assert.equal(resolve(changed[key]),resolve(original[key]),original.id+'.'+key);}
    }
  }
});
test('appearance editing preserves base, inherited layers and advanced bindings',()=>{
  const source=capsuleFixture(),base=structuredClone(source.scene);
  patchAppearance(source,'on','thumb',{x:170,transform:{translateY:3}});
  assert.deepEqual(source.scene,base);assert.equal(designLayers(appearanceScene(source,'on')).find(n=>n.id==='thumb').x,170);
  assert.equal(designLayers(appearanceScene(source,'off')).find(n=>n.id==='thumb').x,31);
  assert.equal(validateWidgetSource(source).ok,true);
  const broken=structuredClone(source);broken.design.appearances[1].layers.missing={x:1};assert.match(validateWidgetSource(broken).error,/missing layer/);
});
test('typed graph lowering rejects cycles, invalid ranges and duplicate writers',()=>{
  const source=capsuleFixture();
  source.design.nodes.push({id:'inverse',label:'Not',domain:{type:'boolean'},expression:{op:'not',value:{ref:'requested-value'}}});
  assert.equal(validateWidgetSource(source).ok,true);
  const cycle=structuredClone(source);cycle.design.nodes[0].expression={ref:'inverse'};assert.match(validateWidgetSource(cycle).error,/cycle/);
  const typed=structuredClone(source);typed.design.nodes[0].domain={type:'number',min:0,max:100};assert.equal(validateWidgetSource(typed).ok,false);
  const writer=structuredClone(source);writer.behavior.regions[0].transitions[0].actions.push({type:'set',signal:'requested',value:{literal:true}});assert.match(validateWidgetSource(writer).error,/second writer/);
  const stale=structuredClone(source);stale.design.appearances[0].regions['requested-state']='bad';assert.match(validateWidgetSource(stale).error,/Unknown appearance/);
});
test('independent region conflicts require an explicit combination override',()=>{
  const source=capsuleFixture();
  source.design.effects=[];source.design.appearances.push({id:'pressed',regions:{interaction:'ready'},layers:{thumb:{x:77}}});
  assert.match(validateWidgetSource(source).error,/combination override/);
  source.design.appearances.push({id:'both',regions:{interaction:'ready','requested-state':'on'},layers:{thumb:{x:88}}});
  assert.equal(validateWidgetSource(source).ok,true,JSON.stringify(validateWidgetSource(source)));
  source.design.appearances.find(a=>a.id==='on').layers.thumb.y=45;
  const thumb=designLayers(appearanceScene(source,'both')).find(n=>n.id==='thumb');
  assert.equal(thumb.x,88);assert.equal(thumb.y,45,'Combined editing context inherits the simpler appearance');
});
test('automatic motion endpoints come from appearances and artifacts retain canonical authoring identity',async()=>{
  const source=capsuleFixture();source.motions=[];source.design.effects=[];
  source.design.effects.push({region:'requested-state',transition:'turn-on',durationMs:160,fps:30,properties:[{node:'thumb',property:'x'}],easing:'ease-in-out',interruption:'reverse'});
  const lowered=lowerWidgetDesign(source);assert.deepEqual(lowered.motions[0].tracks[0].keyframes,[{at:0,value:31},{at:1,value:157}]);
  const bundle=await compileWidget(source,{width:24,height:16,motion:false,audio:false});
  assert.equal(bundle.artifact.sourceHash,await widgetDigest(new TextEncoder().encode(canonicalWidget(source))));
  assert.equal(bundle.artifact.motions.length,0);assert.ok(bundle.artifact.fallbacks.some(f=>f.feature==='motion'));assert.equal('design'in bundle.artifact,false);
});
test('numeric graph maps bounded RPM to a sampled presentation without a second runtime',async()=>{
  const source=createLinearGaugeFixture();source.design=emptyDesign();
  source.design.nodes=[{id:'normalized',label:'Normalize',domain:{type:'number',min:0,max:1},expression:{op:'div',left:{ref:'rpm'},right:{literal:8000}}},{id:'travel',label:'Map range',domain:{type:'number',min:0,max:240},expression:{op:'mul',left:{ref:'normalized'},right:{literal:240}}}];
  assert.equal(validateWidgetSource(source).ok,true);
  const probe=designValues(source,{values:{rpm:4000},regions:{}});assert.equal(probe.normalized,.5);assert.equal(probe.travel,120);
  source.design.nodes[1].expression={op:'div',left:{ref:'rpm'},right:{ref:'rpm'}};assert.match(validateWidgetSource(source).error,/zero/);
  source.design.nodes[1].expression={literal:241};assert.equal(validateWidgetSource(source).ok,false,'Declared output bounds are enforced');
});
test('global layer duplication and deletion preserve all state and motion references atomically',()=>{
  const source=capsuleFixture();let next=0;
  const id=duplicateDesignLayer(source,'body',()=>`copy-${++next}`);assert.ok(id);
  assert.equal(validateWidgetSource(source).ok,true,JSON.stringify(validateWidgetSource(source)));
  const duplicate=designLayers(source.scene).find(n=>n.id===id),copiedThumb=duplicate.children.find(n=>n.kind==='ellipse'&&n.width===50);
  assert.ok(copiedThumb);assert.ok(source.design.appearances[1].layers[copiedThumb.id]);
  assert.ok(source.design.effects[0].properties.some(p=>p.node===copiedThumb.id));
  removeDesignLayer(source,id);assert.equal(validateWidgetSource(source).ok,true);
  assert.equal(source.design.appearances[1].layers[copiedThumb.id],undefined);
  assert.equal(source.design.effects[0].properties.some(p=>p.node===copiedThumb.id),false);
  removeDesignLayer(source,'missing');assert.equal(duplicateDesignLayer(source,'missing',()=>''),undefined);
});
test('automatic motion rejects ambiguous live endpoints and duplicate tracks',()=>{
  const source=capsuleFixture();
  source.motions=[{id:'custom',from:{},to:{},fromRegions:{'requested-state':'off'},toRegions:{'requested-state':'on'},durationMs:160,fps:30,tracks:[{node:'thumb',property:'x',keyframes:[{at:0,value:31},{at:1,value:157}]}]}];
  assert.match(validateWidgetSource(source).error,/tracks conflict/);source.motions=[];
  source.design.effects[0].properties.push(source.design.effects[0].properties[0]);assert.match(validateWidgetSource(source).error,/Duplicate automatic/);
  source.design.effects[0].properties.pop();source.design.appearances[1].layers.thumb.x={op:'if',condition:{ref:'available'},then:{literal:150},else:{literal:140}};
  assert.match(validateWidgetSource(source).error,/live signal/);
  source.design.effects=[];assert.equal(validateWidgetSource(source).ok,true,'Static presentations continue to support dynamic values');
});
test('plain schema, graph namespaces, state mapping and source bounds fail closed',()=>{
  const cases=[
    s=>s.design.nodes[0].id='requested',s=>s.design.nodes[0].extra=true,s=>s.design.owners[0].values.off=true,
    s=>s.design.owners[0].values.on=4,s=>s.design.owners[0].port='missing',s=>s.design.appearances.push(s.design.appearances[1]),
    s=>s.design.appearances[0].regions={},s=>s.design.effects[0].properties[0].property='fill',s=>s.design.effects[0].fps=0,
    s=>s.design.effects[0].easing='bounce',s=>s.design.effects[0].transition='missing',s=>s.design.nodes[0].expression={ref:'missing'},
    s=>s.design.nodes=Array.from({length:65},(_,i)=>({id:'node-'+i,label:'Value',domain:{type:'boolean'},expression:{literal:false}})),
  ];for(const change of cases){const s=capsuleFixture();change(s);assert.equal(validateWidgetSource(s).ok,false);}
  const invalid=createToggleSource();invalid.revision=1;assert.equal(validateWidgetSource(invalid).ok,false);invalid.formatVersion=3;assert.equal(validateWidgetSource(invalid).ok,false);
});
