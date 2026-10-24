import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from '../../test-module-loader.mjs';
const modules=await loadModules(['./widget-graph/states/authoring']);
const a=modules.get('./widget-graph/states/authoring');
const plain=value=>JSON.parse(JSON.stringify(value));
function fixture(){return{id:'widget',size:{width:100,height:80},hitRegions:[{id:'hit-1',x:0,y:0,width:30,height:30,events:{press:'press',release:'release',activate:'press',cancel:'release'}}],behavior:{version:1,signals:[{id:'owned',type:'boolean',source:'local',initial:false},{id:'local',type:'boolean',source:'local',initial:true},{id:'external',type:'boolean',source:'input',initial:false}],events:[{id:'press'},{id:'release'}],outputs:[{id:'owned-port',type:'boolean',neutral:false},{id:'out',type:'boolean',neutral:true}],timers:[{id:'timer-1',event:'press',delayMs:20},{id:'timer-2',event:'release',delayMs:30}],regions:[{id:'machine',initial:'off',states:['off','on'],transitions:[{id:'press-transition',from:'off',to:'on',event:'press',actions:[{type:'start-timer',timer:'timer-1'},{type:'set',signal:'local',value:{literal:false}}]},{id:'release-transition',from:'on',event:'release',actions:[{type:'cancel-timer',timer:'timer-1'},{type:'start-timer',timer:'timer-2'}]}]}]},design:{owners:[{region:'machine',signal:'owned',port:'owned-port',values:{off:false,on:true}}],nodes:[],appearances:[],effects:[{region:'machine',transition:'press-transition'},{region:'machine',transition:'release-transition'},{region:'other',transition:'press-transition'}]}};}

test('palette creates independent sources, unique IDs, full surface hit and action-only stay transition',()=>{
 const source=fixture(),before=plain(source);
 assert.equal(a.addEvent(source).selected,'event:event-1');
 assert.equal(a.addEvent(a.addEvent(source).source).selected,'event:event-2');
 const timer=a.addTimer(source);assert.equal(timer.selected,'timer:timer-3');assert.equal(timer.source.behavior.timers.at(-1).event,'press');
 const hit=a.addHitRegion(source);assert.equal(hit.selected,'hit:hit-2');assert.deepEqual(plain(hit.source.hitRegions.at(-1)),{id:'hit-2',x:0,y:0,width:100,height:80,events:{}});
 const result=a.addTransition(source,'machine');assert.equal(result.transitionId,'transition-1');assert.equal(result.selected,'off');const transition=result.source.behavior.regions[0].transitions.at(-1);assert.equal(transition.to,undefined);assert.equal(transition.priority,1);
 const duplicate=a.addTransition(result.source,'machine');assert.equal(duplicate.transitionId,'transition-2');assert.equal(duplicate.source.behavior.regions[0].transitions.at(-1).priority,2);
 assert.deepEqual(source,before);assert.throws(()=>a.addTransition(source,'missing'),/Select a state machine/);
 for(const key of ['event:x','timer:x','hit:x'])assert.equal(a.recognizesSelection(key),true);assert.equal(a.recognizesSelection('off'),false);
});
test('palette establishes an event when adding timers or transitions to an empty source',()=>{
 const source=fixture();source.behavior.events=[];source.behavior.regions[0].transitions=[];delete source.behavior.timers;
 const timer=a.addTimer(source);assert.equal(timer.source.behavior.events[0].id,'event-1');assert.equal(timer.source.behavior.timers[0].id,'timer-1');
 const transition=a.addTransition(source,'machine');assert.equal(transition.source.behavior.regions[0].transitions[0].event,'event-1');assert.equal(transition.source.behavior.regions[0].transitions[0].priority,0);
});
test('event rename updates every timer, transition and hit assignment without touching unrelated references',()=>{
 const source=fixture(),next=a.renameEvent(source,'press','activate');
 assert.equal(next.behavior.events[0].id,'activate');assert.equal(next.behavior.timers[0].event,'activate');assert.equal(next.behavior.timers[1].event,'release');assert.equal(next.behavior.regions[0].transitions[0].event,'activate');assert.equal(next.hitRegions[0].events.activate,'activate');assert.equal(next.hitRegions[0].events.press,'activate');assert.equal(next.hitRegions[0].events.release,'release');assert.equal(source.behavior.events[0].id,'press');
 assert.throws(()=>a.renameEvent(source,'press','release'),/already in use/);assert.throws(()=>a.renameEvent(source,'press','1bad'),/starting with a letter/);assert.doesNotThrow(()=>a.assertId('press',['press'],'press'));
});
test('timer rename preserves start/cancel actions and deletion removes only affected actions',()=>{
 const source=fixture(),next=a.renameTimer(source,'timer-1','wait');assert.equal(next.behavior.timers[0].id,'wait');assert.equal(next.behavior.regions[0].transitions[0].actions[0].timer,'wait');assert.equal(next.behavior.regions[0].transitions[1].actions[0].timer,'wait');
 const removed=a.removeTimer(next,'wait');assert.deepEqual(plain(removed.behavior.regions[0].transitions[0].actions),[{type:'set',signal:'local',value:{literal:false}}]);assert.equal(removed.behavior.regions[0].transitions[1].actions[0].timer,'timer-2');assert.equal(removed.behavior.timers.length,1);
 assert.throws(()=>a.renameTimer(source,'timer-1','timer-2'),/already in use/);
 const empty=fixture();delete empty.behavior.timers;delete empty.behavior.regions[0].transitions[0].actions;assert.doesNotThrow(()=>a.removeTimer(empty,'missing'));
});
test('event deletion removes dependents and matching design effects while retaining unrelated behavior',()=>{
 const source=fixture(),next=a.removeEvent(source,'press');assert.deepEqual(plain(next.behavior.events),[{id:'release'}]);assert.deepEqual(plain(next.behavior.timers),[{id:'timer-2',event:'release',delayMs:30}]);assert.equal(next.behavior.regions[0].transitions.length,1);assert.deepEqual(plain(next.behavior.regions[0].transitions[0].actions),[{type:'start-timer',timer:'timer-2'}]);assert.deepEqual(plain(next.hitRegions[0].events),{release:'release',cancel:'release'});assert.deepEqual(plain(next.design.effects),[{region:'machine',transition:'release-transition'},{region:'other',transition:'press-transition'}]);assert.equal(source.behavior.events.length,2);
 const noDesign=fixture();delete noDesign.design;delete noDesign.behavior.timers;assert.doesNotThrow(()=>a.removeEvent(noDesign,'press'));assert.doesNotThrow(()=>a.removeTransition(noDesign,'machine','press-transition'));
});
test('actions exclude state owners and external signals and use the selected domain defaults',()=>{
 const source=fixture();assert.deepEqual(plain(a.writableSignals(source)).map(s=>s.id),['local']);assert.deepEqual(plain(a.writableOutputs(source)).map(s=>s.id),['out']);
 assert.deepEqual(plain(a.createAction(source,'set')),{type:'set',signal:'local',value:{literal:true}});assert.deepEqual(plain(a.createAction(source,'emit')),{type:'emit',port:'out',value:{literal:true}});assert.deepEqual(plain(a.createAction(source,'cue')),{type:'cue',cue:'cue'});source.sounds=[{cue:'click'}];assert.equal(a.createAction(source,'cue').cue,'click');
 for(const type of ['start-timer','cancel-timer'])assert.deepEqual(plain(a.createAction(source,type)),{type,timer:'timer-1'});
 assert.equal(a.createAction(source,'start-timer',[{type:'cancel-timer',timer:'timer-1'}]).timer,'timer-2');
 assert.throws(()=>a.createAction(source,'set',[{type:'set',signal:'local',value:{literal:true}}]),/unused local signal/);assert.throws(()=>a.createAction(source,'emit',[{type:'emit',port:'out',value:{literal:true}}]),/unused published output/);
 source.behavior.signals=[];source.behavior.outputs=[];source.behavior.timers=[];assert.throws(()=>a.createAction(source,'set'),/local signal/);assert.throws(()=>a.createAction(source,'emit'),/published output/);assert.throws(()=>a.createAction(source,'start-timer'),/Add a timer/);
 delete source.behavior.outputs;delete source.design;assert.deepEqual(plain(a.writableOutputs(source)),[]);source.behavior.signals=[{id:'free',source:'local'}];source.behavior.outputs=[{id:'free-port'}];assert.equal(a.writableSignals(source)[0].id,'free');assert.equal(a.writableOutputs(source)[0].id,'free-port');
});

test('event connections allocate priority against the final dragged event',()=>{
 const source=fixture();source.behavior.regions[0].transitions.push({id:'global-release',from:'*',event:'release',priority:7});
 const result=a.addTransition(source,'machine','release');const transition=result.source.behavior.regions[0].transitions.at(-1);
 assert.equal(transition.event,'release');assert.equal(transition.priority,8);
 assert.throws(()=>a.addTransition(source,'machine','missing'),/existing event/);
});

test('timer and event removal preserve serializable absent optional fields',()=>{
 const source=fixture();delete source.behavior.timers;delete source.behavior.regions[0].transitions[0].actions;
 const serializable=value=>JSON.stringify(value,(_key,item)=>{assert.notEqual(item,undefined);return item;});
 serializable(a.removeTimer(source,'missing'));serializable(a.removeEvent(source,'release'));
});
