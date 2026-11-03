import test from 'node:test';
import assert from 'node:assert/strict';
import {imageModule} from './test-fixture.mjs';

const {createGameImageAction,gameImageSource,resolveGameImage}=imageModule;
class FakeImage {
  src='';dataset={};removed=[];events=[];
  removeAttribute(name){this.removed.push(name);if(name==='src')this.src='';}
  dispatchEvent(event){this.events.push(event.type);return true;}
}
function observerFactory(){
  const instances=[];
  class FakeObserver {
    constructor(callback,options){this.callback=callback;this.options=options;this.observed=[];this.unobserved=[];this.disconnected=0;instances.push(this);}
    observe(node){this.observed.push(node);}
    unobserve(node){this.unobserved.push(node);}
    disconnect(){this.disconnected++;}
    trigger(...nodes){this.callback(nodes.map(target=>({target,isIntersecting:true})),this);}
  }
  return {instances,Observer:FakeObserver};
}
const ready=value=>({status:'ready',value});

test('game images wait for visibility and resolve an opaque asset only once visible',()=>{
  const observers=observerFactory(),calls=[],image=new FakeImage(),ref=`game-asset:${'a'.repeat(64)}:20`;
  const action=createGameImageAction((reference,listener)=>{calls.push({reference,listener});return()=>calls.push({stopped:reference});},()=>observers.Observer);
  const mounted=action(image,ref);
  assert.equal(calls.length,0);assert.equal(image.src,'');assert.equal(image.dataset.gameImageState,'loading');
  assert.equal(observers.instances.length,1);assert.equal(observers.instances[0].options.rootMargin,'200px');
  observers.instances[0].trigger(image);observers.instances[0].trigger(image);assert.equal(calls.length,1);assert.equal(calls[0].reference,ref);
  calls[0].listener(ready('data:image/jpeg;base64,encoded'));
  assert.equal(image.src,'data:image/jpeg;base64,encoded');assert.equal(image.dataset.gameImageState,'ready');
  mounted.destroy();
});

test('asset failures leave the image source unset and emit an error event',()=>{
  const observers=observerFactory(),image=new FakeImage(),ref=`game-asset:${'6'.repeat(64)}:6`;
  const action=createGameImageAction((_reference,listener)=>{queueMicrotask(()=>listener({status:'error',error:'expired'}));return()=>{};},()=>observers.Observer);
  const mounted=action(image,ref);observers.instances[0].trigger(image);
  return new Promise(resolve=>queueMicrotask(()=>{assert.equal(image.src,'');assert.equal(image.dataset.gameImageState,'error');assert.deepEqual(image.events,['error']);mounted.destroy();resolve();}));
});

test('one intersection observer serves several mounted game images',()=>{
  const observers=observerFactory(),requests=[],first=new FakeImage(),second=new FakeImage();
  const action=createGameImageAction((reference,listener)=>{requests.push({reference,listener});return()=>{};},()=>observers.Observer);
  const one=action(first,`game-asset:${'1'.repeat(64)}:1`),two=action(second,`game-asset:${'2'.repeat(64)}:2`);
  assert.equal(observers.instances.length,1);assert.deepEqual(observers.instances[0].observed,[first,second]);
  observers.instances[0].trigger(first,second);assert.equal(requests.length,2);
  requests[0].listener(ready('data:image/jpeg;base64,first'));requests[1].listener(ready('data:image/jpeg;base64,second'));
  assert.equal(first.src,'data:image/jpeg;base64,first');assert.equal(second.src,'data:image/jpeg;base64,second');
  one.destroy();two.destroy();
});

test('unmount and reference replacement stop the old subscription and ignore stale completion',()=>{
  const observers=observerFactory(),requests=[],image=new FakeImage(),oldRef=`game-asset:${'3'.repeat(64)}:3`,newRef=`game-asset:${'4'.repeat(64)}:4`;
  const action=createGameImageAction((reference,listener)=>{const request={reference,listener,stopped:false};requests.push(request);return()=>{request.stopped=true;};},()=>observers.Observer);
  const mounted=action(image,oldRef);observers.instances[0].trigger(image);assert.equal(requests.length,1);
  mounted.update(newRef);assert.equal(requests[0].stopped,true);assert.equal(image.src,'');assert.equal(image.dataset.gameImageState,'loading');
  observers.instances.at(-1).trigger(image);assert.equal(requests.length,2);
  requests[0].listener(ready('data:image/jpeg;base64,stale'));assert.equal(image.src,'');
  requests[1].listener(ready('data:image/jpeg;base64,current'));assert.equal(image.src,'data:image/jpeg;base64,current');
  mounted.destroy();assert.equal(requests[1].stopped,true);
  requests[1].listener(ready('data:image/jpeg;base64,after-unmount'));assert.equal(image.src,'data:image/jpeg;base64,current');
});

test('ordinary URLs pass through without subscribing or creating an observer',async()=>{
  const observers=observerFactory(),calls=[],image=new FakeImage(),url='https://example.test/cover.jpg';
  assert.equal(gameImageSource(url),url);assert.equal(gameImageSource(`game-asset:${'a'.repeat(64)}:1`),undefined);assert.equal(gameImageSource(undefined),undefined);
  const action=createGameImageAction((...args)=>{calls.push(args);return()=>{};},()=>observers.Observer),mounted=action(image,url);
  assert.equal(image.src,url);assert.equal(image.dataset.gameImageState,'ready');assert.equal(observers.instances.length,0);assert.equal(calls.length,0);
  assert.equal(await resolveGameImage(url),url);mounted.destroy();
});

test('resolveGameImage handles ready, error, abort and synchronous ready observers',async()=>{
  const ref=`game-asset:${'5'.repeat(64)}:5`,stops=[];
  const readyPromise=resolveGameImage(ref,undefined,(reference,listener)=>{assert.equal(reference,ref);queueMicrotask(()=>listener(ready('data:image/jpeg;base64,async')));return()=>stops.push('ready');});
  assert.equal(await readyPromise,'data:image/jpeg;base64,async');assert.deepEqual(stops,['ready']);
  await assert.rejects(resolveGameImage(ref,undefined,(_reference,listener)=>{listener({status:'error',error:'expired'});return()=>stops.push('error');}),/expired/);
  assert.deepEqual(stops,['ready','error']);
  const controller=new AbortController(),gate={listener:undefined,stopped:false};
  const pending=resolveGameImage(ref,controller.signal,(_reference,listener)=>{gate.listener=listener;return()=>{gate.stopped=true;};});controller.abort();
  await assert.rejects(pending,/cancelled/);assert.equal(gate.stopped,true);
  const synchronous=await resolveGameImage(ref,undefined,(_reference,listener)=>{listener(ready('data:image/jpeg;base64,sync'));return()=>stops.push('sync');});
  assert.equal(synchronous,'data:image/jpeg;base64,sync');assert.deepEqual(stops,['ready','error','sync']);
});
