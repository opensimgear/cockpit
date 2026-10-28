import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const {createCanvasCamera}=(await loadModules(['./profile-canvas-camera'])).get('./profile-canvas-camera');

function harness(extra={}){
  let view={x:0,y:0,zoom:1},time=0,id=0;
  const pending=new Map(),history=new Map(),writes=[],activity=[],cancelled=[];
  const camera=createCanvasCamera(()=>view,next=>{view={...next};writes.push({...next});},{
    request:callback=>{pending.set(++id,callback);history.set(id,callback);return id;},
    cancel:key=>{cancelled.push(key);pending.delete(key);},now:()=>time,
    activity:value=>activity.push(value),...extra,
  });
  return{camera,writes,activity,pending,history,cancelled,get view(){return view;},set view(value){view=value;},
    step(next){time=next;const callbacks=[...pending.values()];pending.clear();for(const callback of callbacks)callback(time);}};
}

test('eases all rendered coordinates together and finishes at the exact target',async()=>{
  const h=harness(),target={x:120,y:-240,zoom:3},done=h.camera.move(target);
  assert.deepEqual({...h.camera.destination},target);assert.equal(h.pending.size,1);
  h.step(0);assert.deepEqual(h.view,{x:0,y:0,zoom:1});
  h.step(60);assert.deepEqual(h.view,{x:7.5,y:-15,zoom:1.125});
  h.step(120);assert.deepEqual(h.view,{x:60,y:-120,zoom:2});
  h.step(180);assert.deepEqual(h.view,{x:112.5,y:-225,zoom:2.875});
  h.step(400);assert.equal(await done,true);assert.deepEqual(h.view,target);
  assert.equal(h.camera.destination,null);assert.equal(h.pending.size,0);assert.deepEqual(h.activity,[true,false]);
});

test('rapid navigation starts from the current rendered view and fences old callbacks',async()=>{
  const h=harness(),first=h.camera.move({x:100,y:100,zoom:2});h.step(120);
  const stale=[...h.pending.keys()][0],second=h.camera.move({x:150,y:-50,zoom:3});
  assert.equal(await first,false);assert.deepEqual(h.view,{x:50,y:50,zoom:1.5});
  h.history.get(stale)(240);assert.deepEqual(h.view,{x:50,y:50,zoom:1.5});
  h.step(240);assert.deepEqual(h.view,{x:100,y:0,zoom:2.25});
  h.step(360);assert.equal(await second,true);assert.deepEqual(h.view,{x:150,y:-50,zoom:3});
  assert.deepEqual(h.cancelled,[stale]);
});

test('cancel retains rendered coordinates, resolves false, and permits a new move',async()=>{
  const h=harness(),done=h.camera.move({x:100,y:0,zoom:2});h.step(60);
  const rendered={...h.view},stale=[...h.pending.keys()][0];
  h.camera.cancel();h.camera.cancel();assert.equal(await done,false);
  h.history.get(stale)(500);assert.deepEqual(h.view,rendered);assert.equal(h.camera.destination,null);
  assert.equal(h.pending.size,0);assert.deepEqual(h.activity,[true,false]);
  const next=h.camera.move({x:0,y:0,zoom:1});h.step(500);assert.equal(await next,true);
});

test('noops, explicit snaps, reduced motion, and zero duration schedule no frames',async()=>{
  for(const extra of [{},{reducedMotion:()=>true},{duration:0}]){
    const h=harness(extra);assert.equal(await h.camera.move({...h.view}),true);assert.equal(h.writes.length,0);
    assert.equal(await h.camera.move({x:30,y:20,zoom:2},false),true);assert.deepEqual(h.view,{x:30,y:20,zoom:2});
    if(extra.reducedMotion||extra.duration===0)assert.equal(await h.camera.move({x:10,y:0,zoom:1}),true);
    assert.equal(h.pending.size,0);assert.deepEqual(h.activity,[]);
  }
  const h=harness(),prior=h.camera.move({x:100,y:20,zoom:3});
  assert.equal(await h.camera.move({x:25,y:25,zoom:1},false),true);assert.equal(await prior,false);
  assert.deepEqual(h.view,{x:25,y:25,zoom:1});assert.equal(h.pending.size,0);
});

test('destroy fences late callbacks and all future moves',async()=>{
  const h=harness(),done=h.camera.move({x:20,y:30,zoom:2}),stale=[...h.pending.keys()][0];
  h.step(60);const rendered={...h.view};h.camera.destroy();h.camera.destroy();
  assert.equal(await done,false);h.history.get(stale)(300);assert.deepEqual(h.view,rendered);
  assert.equal(await h.camera.move({x:9,y:9,zoom:9}),false);assert.equal(h.pending.size,0);
});

test('invalid input fails before replacing valid navigation',async()=>{
  const h=harness(),done=h.camera.move({x:10,y:20,zoom:2});
  for(const view of [{x:NaN,y:0,zoom:1},{x:0,y:Infinity,zoom:1},{x:0,y:0,zoom:0},{x:0,y:0,zoom:-1}])
    assert.throws(()=>h.camera.move(view),/finite/);
  assert.deepEqual({...h.camera.destination},{x:10,y:20,zoom:2});h.step(240);assert.equal(await done,true);
  h.view={x:Infinity,y:0,zoom:1};assert.throws(()=>h.camera.move({x:0,y:0,zoom:1}),/finite/);
  for(const duration of [-1,NaN,Infinity])assert.throws(()=>harness({duration}),/duration/);
});

test('captures inputs and isolates destination snapshots from callers',async()=>{
  const h=harness(),target={x:100,y:200,zoom:3},original=h.view,done=h.camera.move(target);
  target.x=999;original.y=999;h.camera.destination.zoom=999;
  h.step(120);assert.deepEqual(h.view,{x:50,y:100,zoom:2});
  h.view.x=999;h.step(240);assert.equal(await done,true);assert.deepEqual(h.view,{x:100,y:200,zoom:3});
});

test('optional activity callback and cancellation from a writer are supported',async()=>{
  let camera,view={x:0,y:0,zoom:1},callback;
  camera=createCanvasCamera(()=>view,next=>{view=next;camera.cancel();},{request:next=>{callback=next;return 1;},cancel:()=>{},now:()=>0});
  const done=camera.move({x:5,y:5,zoom:2});callback(100);assert.equal(await done,false);
  assert.ok(view.x>0&&view.x<5);
});
