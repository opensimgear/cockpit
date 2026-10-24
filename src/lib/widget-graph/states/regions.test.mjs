import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from '../../test-module-loader.mjs';
const modules=await loadModules(['./widget-graph/states/regions']);
const {renameRegion,removeRegion,renameState}=modules.get('./widget-graph/states/regions');
const plain=value=>JSON.parse(JSON.stringify(value));
function fixture(){return{behavior:{regions:[{id:'machine',initial:'off',states:['off','on'],transitions:[{id:'go',from:'off',to:'on',event:'go',guard:{op:'eq',left:{state:'machine'},right:{literal:'off'}}},{id:'any',from:'*',event:'go',actions:[]}]},{id:'other',initial:'off',states:['off'],transitions:[]}]},scene:[{id:'text',text:{state:'machine'}}],design:{owners:[{region:'machine',signal:'owned',values:{off:false,on:true}},{region:'other',signal:'other',values:{off:false}}],nodes:[{id:'state-name',expression:{state:'machine'}}],appearances:[{id:'a',regions:{machine:'off',other:'off'},layers:{text:{text:{state:'machine'}}}},{id:'b',regions:{other:'off'},layers:{}}],effects:[{region:'machine',transition:'go'},{region:'other',transition:'go'}]},motions:[{id:'m1',fromRegions:{machine:'off'},toRegions:{machine:'on'},from:{},to:{}},{id:'m2',fromRegions:{other:'off'},toRegions:{other:'off'},from:{},to:{}},{id:'m3',from:{owned:false},toRegions:{machine:'off'},to:{owned:true}}]};}

test('region rename updates identities, selectors and nested state expressions without changing IDs or source',()=>{
 const source=fixture(),before=plain(source),next=renameRegion(source,'machine','primary');
 assert.deepEqual(source,before);assert.equal(next.behavior.regions[0].id,'primary');assert.equal(next.behavior.regions[0].initial,'off');assert.equal(next.behavior.regions[0].transitions[0].id,'go');assert.equal(next.behavior.regions[0].transitions[0].guard.left.state,'primary');assert.equal(next.scene[0].text.state,'primary');assert.equal(next.design.nodes[0].expression.state,'primary');assert.equal(next.design.appearances[0].layers.text.text.state,'primary');assert.deepEqual(plain(next.design.appearances[0].regions),{primary:'off',other:'off'});assert.equal(next.design.owners[0].region,'primary');assert.equal(next.design.owners[1].region,'other');assert.equal(next.design.effects[0].region,'primary');assert.equal(next.design.effects[1].region,'other');assert.deepEqual(plain(next.motions[0].fromRegions),{primary:'off'});assert.deepEqual(plain(next.motions[0].toRegions),{primary:'on'});assert.deepEqual(plain(next.motions[1].fromRegions),{other:'off'});assert.deepEqual(plain(next.motions[2].toRegions),{primary:'off'});
 assert.deepEqual(plain(renameRegion(source,'machine','machine')),before);
});
test('state rename updates initial state, destinations, origins, ownership and selectors only within its region',()=>{
 const source=fixture(),next=renameState(source,'machine','off','ready');assert.equal(next.behavior.regions[0].initial,'ready');assert.deepEqual(plain(next.behavior.regions[0].states),['ready','on']);assert.equal(next.behavior.regions[0].transitions[0].from,'ready');assert.equal(next.behavior.regions[0].transitions[0].to,'on');assert.equal(next.behavior.regions[0].transitions[1].from,'*');assert.deepEqual(plain(next.design.owners[0].values),{ready:false,on:true});assert.deepEqual(plain(next.design.owners[1].values),{off:false});assert.equal(next.design.appearances[0].regions.machine,'ready');assert.equal(next.design.appearances[0].regions.other,'off');assert.equal(next.motions[0].fromRegions.machine,'ready');assert.equal(next.motions[2].toRegions.machine,'ready');assert.equal(next.behavior.regions[0].transitions[0].guard.left.state,'machine');assert.equal(next.behavior.regions[0].transitions[0].guard.right.literal,'ready');assert.equal(source.behavior.regions[0].initial,'off');
 const destination=renameState(source,'machine','on','active');assert.equal(destination.behavior.regions[0].transitions[0].to,'active');assert.equal(destination.motions[0].toRegions.machine,'active');assert.equal(destination.behavior.regions[0].initial,'off');assert.deepEqual(plain(renameState(source,'machine','off','off')),plain(source));
});
test('region deletion removes associated metadata and motions but refuses remaining external expressions',()=>{
 const source=fixture(),before=plain(source);assert.throws(()=>removeRegion(source,'machine'),/still used by an expression/);assert.deepEqual(source,before);
 source.scene=[];source.design.nodes=[];const next=removeRegion(source,'machine');assert.deepEqual(plain(next.behavior.regions).map(r=>r.id),['other']);assert.deepEqual(plain(next.design.owners).map(o=>o.region),['other']);assert.deepEqual(plain(next.design.appearances).map(a=>a.id),['b']);assert.deepEqual(plain(next.design.effects).map(e=>e.region),['other']);assert.deepEqual(plain(next.motions).map(m=>m.id),['m2']);
 source.behavior.regions[1].transitions=[{id:'dependent',guard:{op:'not',value:{state:'machine'}}}];assert.throws(()=>removeRegion(source,'machine'),/external state references/);
});
test('invalid names, duplicates and missing identities produce meaningful errors',()=>{
 const source=fixture();for(const name of ['','1bad','bad name','x'.repeat(65)])assert.throws(()=>renameRegion(source,'machine',name),/starting with a letter/);assert.throws(()=>renameRegion(source,'machine','other'),/already in use/);assert.throws(()=>renameState(source,'machine','off','on'),/already in use/);assert.throws(()=>renameState(source,'machine','off','!'),/starting with a letter/);assert.throws(()=>renameState(source,'machine','missing','ready'),/does not exist/);for(const fn of [()=>renameRegion(source,'missing','ready'),()=>removeRegion(source,'missing'),()=>renameState(source,'missing','off','ready')])assert.throws(fn,/State machine.*does not exist/);
});
test('optional authoring metadata is not required and unusual data is not mistaken for an expression',()=>{
 const source=fixture();delete source.design;delete source.motions;source.scene=[null,undefined,3,{state:'machine',label:'unrelated'},[{state:4}]];assert.equal(renameRegion(source,'machine','primary').scene[3].state,'machine');assert.equal(renameState(source,'machine','off','ready').behavior.regions[0].initial,'ready');assert.equal(removeRegion(source,'machine').behavior.regions.length,1);
});
test('state equality and inequality literals follow their region, including conditional values, without changing unrelated text',()=>{
 const source=fixture();source.scene=[{text:{literal:'off'}},{text:{op:'if',condition:{op:'ne',left:{literal:'off'},right:{state:'machine'}},then:{literal:'off'},else:{literal:'on'}}},{text:{op:'eq',left:{op:'if',condition:{literal:true},then:{state:'machine'},else:{literal:'off'}},right:{op:'if',condition:{literal:false},then:{literal:'off'},else:{literal:'on'}}}},{text:{op:'eq',left:{state:'other'},right:{literal:'off'}}}];
 const next=renameState(source,'machine','off','ready');assert.equal(next.scene[0].text.literal,'off');assert.equal(next.scene[1].text.condition.left.literal,'ready');assert.equal(next.scene[1].text.then.literal,'off');assert.equal(next.scene[2].text.left.else.literal,'ready');assert.equal(next.scene[2].text.right.then.literal,'ready');assert.equal(next.scene[2].text.right.else.literal,'on');assert.equal(next.scene[3].text.right.literal,'off');
});

test('state rename rejects enum aliases and indirect consumers without mutating the source',()=>{
 const source=fixture();source.design.nodes=[{id:'state-alias',label:'State alias',domain:{type:'enum',values:['off','on']},expression:{state:'machine'}},{id:'alias-chain',label:'Alias chain',domain:{type:'enum',values:['off','on']},expression:{ref:'state-alias'}}];
 source.behavior.regions[0].transitions[0].guard={op:'eq',left:{ref:'alias-chain'},right:{literal:'off'}};
 const before=plain(source);
 assert.throws(()=>renameState(source,'machine','off','ready'),/Update or remove.*state-alias.*alias-chain.*references before renaming/);
 assert.deepEqual(source,before);assert.deepEqual(plain(renameState(source,'machine','off','off')),before);
 source.design.nodes[0].expression={op:'if',condition:{literal:true},then:{literal:'off'},else:{state:'machine'}};
 assert.throws(()=>renameState(source,'machine','off','ready'),/state value aliases/);
});

test('other-region aliases, boolean comparisons and cyclic graph drafts do not block unrelated state rename',()=>{
 const source=fixture();source.design.nodes=[{id:'other-alias',domain:{type:'enum',values:['off']},expression:{state:'other'}},{id:'is-off',domain:{type:'boolean'},expression:{op:'eq',left:{state:'machine'},right:{literal:'off'}}},{id:'cycle-a',domain:{type:'enum',values:['off']},expression:{ref:'cycle-b'}},{id:'cycle-b',domain:{type:'enum',values:['off']},expression:{ref:'cycle-a'}}];
 const next=renameState(source,'machine','off','ready');assert.equal(next.behavior.regions[0].initial,'ready');assert.equal(next.design.nodes[0].expression.state,'other');assert.equal(next.design.nodes[1].expression.right.literal,'ready');
});

test('removing a region preserves absence of optional motions',()=>{
 const source={behavior:{regions:[{id:'machine',initial:'ready',states:['ready'],transitions:[]}]}};
 const next=removeRegion(source,'machine');assert.equal(Object.hasOwn(next,'motions'),false);
});
