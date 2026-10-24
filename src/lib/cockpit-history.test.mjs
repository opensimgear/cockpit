import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';

const {createCockpitHistory,cockpitRoute}=(await loadModules(['./cockpit-history'])).get('./cockpit-history');
const plain=value=>JSON.parse(JSON.stringify(value));
const settle=()=>new Promise(resolve=>setImmediate(resolve));
const urlFor=route=>`/cockpit?view=${route.view}${route.componentId?`&component=${encodeURIComponent(route.componentId)}`:''}${route.profileId?`&profile=${encodeURIComponent(route.profileId)}`:''}${route.gameId?`#games/${route.gameId}/integration`:''}`;

function browser(initialState={outside:'preserved'}) {
  const entries=[{state:initialState,url:'/cockpit'}],listeners=new Map(),calls=[];
  let position=0,location=new URL('http://localhost/cockpit');
  const host={
    get location(){return location;},
    addEventListener(name,listener){if(!listeners.has(name))listeners.set(name,new Set());listeners.get(name).add(listener);},
    removeEventListener(name,listener){listeners.get(name)?.delete(listener);},
  };
  function dispatch(type,properties={}) {
    const event={type,...properties,defaultPrevented:false,preventDefault(){this.defaultPrevented=true;}};
    for(const listener of listeners.get(type)??[])listener(event);
    return event;
  }
  function go(delta){
    calls.push(['go',delta]);const target=position+delta;
    if(target<0||target>=entries.length)return;
    position=target;location=new URL(entries[position].url,location);
    queueMicrotask(()=>dispatch('popstate',{state:plain(entries[position].state)}));
  }
  host.history={
    get state(){return entries[position].state;},
    pushState(state,_unused,url){calls.push(['push']);entries.splice(position+1);entries.push({state:plain(state),url});position++;location=new URL(url,location);},
    replaceState(state,_unused,url){calls.push(['replace']);entries[position]={state:plain(state),url};location=new URL(url,location);},
    go,back(){calls.push(['back']);go(-1);},forward(){calls.push(['forward']);go(1);},
  };
  return{host,calls,dispatch,entries,get position(){return position;},get listenerCount(){return[...listeners.values()].reduce((count,set)=>count+set.size,0);}};
}
function fixture(restore) {
  const ui=browser(),restored=[];
  const navigation=createCockpitHistory({view:'overview'},{host:ui.host,urlFor,restore:async route=>{restored.push(plain(route));return restore?await restore(route):route;}});
  return{...ui,navigation,restored,get position(){return ui.position;},get listenerCount(){return ui.listenerCount;}};
}

test('main and detail visits restore through Back and Forward without adding entries or replaying actions',async()=>{
  const ui=fixture();
  assert.equal(ui.host.history.state.outside,'preserved');
  ui.navigation.record({view:'devices'});
  ui.navigation.record({view:'devices',componentId:'controller:wheel'});
  ui.navigation.record({view:'devices',componentId:'controller:wheel'});
  assert.equal(ui.entries.length,3);
  ui.navigation.back();await settle();
  assert.deepEqual(plain(ui.navigation.route),{view:'devices'});
  ui.navigation.back();await settle();
  assert.deepEqual(plain(ui.navigation.route),{view:'overview'});
  ui.navigation.forward();await settle();
  ui.navigation.forward();await settle();
  assert.deepEqual(plain(ui.navigation.route),{view:'devices',componentId:'controller:wheel'});
  assert.equal(ui.entries.length,3);
  assert.equal(ui.calls.filter(call=>call[0]==='push').length,2);
  assert.deepEqual(ui.restored,[{view:'devices'},{view:'overview'},{view:'devices'},{view:'devices',componentId:'controller:wheel'}]);
  assert.equal(ui.navigation.ownsLocation(),true);
});

test('Debug is a standalone history route restored between the profile editor and Games',async()=>{
  const ui=fixture();assert.deepEqual(plain(cockpitRoute({view:'debug'})),{view:'debug'});
  ui.navigation.record({view:'profiles',profileId:'saved-profile'});ui.navigation.record({view:'debug'});ui.navigation.record({view:'games',gameId:'dcs'});
  ui.navigation.back();await settle();assert.deepEqual(plain(ui.navigation.route),{view:'debug'});
  ui.navigation.back();await settle();assert.deepEqual(plain(ui.navigation.route),{view:'profiles',profileId:'saved-profile'});
  ui.navigation.forward();await settle();assert.deepEqual(plain(ui.navigation.route),{view:'debug'});
  assert.deepEqual(ui.restored,[{view:'debug'},{view:'profiles',profileId:'saved-profile'},{view:'debug'}]);assert.equal(ui.entries.length,4);
});

test('mouse Back and Forward consume their complete event sequence once and ordinary clicks remain untouched',async()=>{
  const ui=fixture();ui.navigation.record({view:'companions'});ui.navigation.record({view:'preferences'});
  for(const type of ['mousedown','mouseup','auxclick'])assert.equal(ui.dispatch(type,{button:3}).defaultPrevented,true);
  await settle();assert.deepEqual(plain(ui.navigation.route),{view:'companions'});
  assert.equal(ui.calls.filter(call=>call[0]==='back').length,1);
  for(const type of ['mousedown','mouseup','auxclick'])assert.equal(ui.dispatch(type,{button:4}).defaultPrevented,true);
  await settle();assert.deepEqual(plain(ui.navigation.route),{view:'preferences'});
  assert.equal(ui.calls.filter(call=>call[0]==='forward').length,1);
  for(const button of [0,1,2])assert.equal(ui.dispatch('mouseup',{button}).defaultPrevented,false);
  assert.equal(ui.dispatch('keydown',{key:'ArrowLeft',altKey:true}).defaultPrevented,true);await settle();
  assert.deepEqual(plain(ui.navigation.route),{view:'companions'});
  assert.equal(ui.dispatch('keydown',{key:'ArrowLeft',altKey:true,ctrlKey:true}).defaultPrevented,false);
  ui.navigation.dispose();assert.equal(ui.listenerCount,0);
  ui.dispatch('mouseup',{button:3});await settle();assert.equal(ui.restored.length,3);
});

test('new navigation after Back replaces the forward branch and replacement updates the current entry',async()=>{
  const ui=fixture();ui.navigation.record({view:'devices'});ui.navigation.record({view:'games',gameId:'dcs'});
  ui.navigation.back();await settle();
  ui.navigation.record({view:'profiles'});assert.equal(ui.entries.length,3);
  ui.navigation.forward();await settle();assert.deepEqual(plain(ui.navigation.route),{view:'profiles'});
  ui.navigation.record({view:'profiles',profileId:'saved'},true);assert.equal(ui.entries.length,3);
  assert.deepEqual(ui.host.history.state.osgCockpit.route,{view:'profiles',profileId:'saved'});
  ui.navigation.back();await settle();assert.deepEqual(plain(ui.navigation.route),{view:'devices'});
});

test('Keep editing after a delayed profile guard repairs the browser index and accepts the next Back',async()=>{
  let answer;const ui=fixture(()=>new Promise(resolve=>{answer=resolve;}));
  ui.navigation.record({view:'profiles'});ui.navigation.record({view:'profiles',profileId:'dirty'});
  ui.navigation.back();await settle();assert.equal(ui.navigation.navigating,true);
  assert.deepEqual(plain(ui.navigation.route),{view:'profiles',profileId:'dirty'});
  ui.navigation.record({view:'activity'});assert.equal(ui.entries.length,3);
  ui.dispatch('mouseup',{button:3});assert.equal(ui.restored.length,1);
  answer(false);await settle();await settle();
  assert.equal(ui.navigation.navigating,false);assert.deepEqual(ui.host.history.state.osgCockpit.route,{view:'profiles',profileId:'dirty'});
  assert.equal(ui.entries.length,3);
  ui.navigation.back();await settle();answer({view:'profiles'});await settle();
  assert.deepEqual(plain(ui.navigation.route),{view:'profiles'});assert.equal(ui.navigation.navigating,false);
  assert.equal(ui.calls.filter(call=>call[0]==='push').length,2);
});

test('missing detail restoration can fall back to its main page in the same entry',async()=>{
  const ui=fixture(route=>route.profileId?{view:'profiles'}:route);
  ui.navigation.record({view:'profiles',profileId:'discarded-draft'});ui.navigation.record({view:'overview'});
  ui.navigation.back();await settle();
  assert.deepEqual(plain(ui.navigation.route),{view:'profiles'});
  assert.deepEqual(ui.host.history.state.osgCockpit.route,{view:'profiles'});assert.equal(ui.entries.length,3);
});

test('a rejected asynchronous restoration reports the error and repairs the cursor before another Back',async()=>{
  const ui=browser(),errors=[],failure=new Error('The profile list could not be loaded.');let rejectRestore,rejectNext=true;
  const navigation=createCockpitHistory({view:'overview'},{host:ui.host,urlFor,onError:error=>errors.push(error),restore:async route=>rejectNext?await new Promise((_resolve,reject)=>{rejectRestore=reject;}):route});
  navigation.record({view:'profiles'});navigation.record({view:'profiles',profileId:'saved'});
  navigation.back();await settle();assert.equal(navigation.navigating,true);
  rejectRestore(failure);await settle();await settle();
  assert.deepEqual(errors,[failure]);assert.equal(navigation.navigating,false);assert.equal(ui.position,2);
  assert.deepEqual(plain(navigation.route),{view:'profiles',profileId:'saved'});
  assert.deepEqual(ui.host.history.state.osgCockpit.route,{view:'profiles',profileId:'saved'});assert.equal(ui.entries.length,3);
  rejectNext=false;navigation.back();await settle();
  assert.deepEqual(plain(navigation.route),{view:'profiles'});assert.equal(ui.position,1);assert.equal(errors.length,1);
});

test('adopting a hash navigation uses the existing browser entry and restores the adjacent pages',async()=>{
  const ui=fixture();ui.navigation.record({view:'profiles',profileId:'saved'});
  const game={view:'games',gameId:'dcs'};
  ui.host.history.pushState(null,'',urlFor(game));
  assert.equal(ui.entries.length,3);assert.equal(ui.navigation.ownsLocation(),false);
  const pushes=ui.calls.filter(call=>call[0]==='push').length;
  ui.navigation.adopt(game);
  assert.equal(ui.entries.length,3);assert.equal(ui.calls.filter(call=>call[0]==='push').length,pushes);
  assert.equal(ui.host.history.state.osgCockpit.index,2);assert.equal(ui.navigation.ownsLocation(),true);
  ui.navigation.back();await settle();assert.deepEqual(plain(ui.navigation.route),{view:'profiles',profileId:'saved'});
  ui.navigation.forward();await settle();assert.deepEqual(plain(ui.navigation.route),game);
  assert.equal(ui.entries.length,3);assert.deepEqual(ui.restored,[{view:'profiles',profileId:'saved'},game]);
});

test('malformed foreign entries and invalid routes never reach the restoration callback',async()=>{
  const ui=fixture();ui.navigation.record({view:'devices'});
  for(const route of [null,{},[],{view:'unknown'},{view:'devices',componentId:''},{view:'companions',companion:{kind:'invalid'}},{view:'profiles',profileId:7},{view:'devices',deviceId:'x'.repeat(1025)}])assert.equal(cockpitRoute(route),null);
  for(const state of [null,{}, {osgCockpit:{index:-1,route:{view:'overview'}}},{osgCockpit:{index:0.5,route:{view:'overview'}}},{osgCockpit:{index:0,route:{view:'unknown'}}}])ui.dispatch('popstate',{state});
  await settle();assert.deepEqual(ui.restored,[]);assert.deepEqual(plain(ui.navigation.route),{view:'devices'});
  ui.navigation.record({view:'unknown'});assert.equal(ui.entries.length,2);
  ui.navigation.dispose();ui.navigation.record({view:'preferences'});assert.equal(ui.entries.length,2);
});


test('widget content editors have restorable navigation entries',()=>{
 const route={view:'companions',companion:{kind:'widget',id:'capsule'}};
 assert.deepEqual(plain(cockpitRoute(route)),route);
 assert.equal(cockpitRoute({view:'companions',companion:{kind:'widget',id:''}}),null);
});
