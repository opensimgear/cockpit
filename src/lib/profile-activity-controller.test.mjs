import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
import {deferred,graph,known,node,owner,pathSamples,plain,port,topologyPage} from './profile-activity-fixtures.mjs';

const modules=await loadModules(['./profiles','./profile-inspection','./profile-activity','./profile-activity-controller']);
const {ProfileTopologyClient}=modules.get('./profile-activity');
const {ProfileActivityController,profileActivityPort}=modules.get('./profile-activity-controller');

function harness(data=graph()){
  const calls=[],topology=new ProfileTopologyClient(async request=>{calls.push(request);return topologyPage(request,data);});
  const streams=pathSamples(data);
  return {calls,topology,streams,controller:new ProfileActivityController(topology,streams.observe)};
}

test('only topology loads on open and exact path samples update without another native request',async()=>{
  const {controller,calls,streams}=harness();
  assert.equal(await controller.refresh(),false);
  assert.equal(await controller.selectBlock('source'),false);
  assert.equal(await controller.selectEdge('a'),false);
  assert.deepEqual(calls,[]);
  await controller.open(owner(),'invert');
  assert.deepEqual(plain(controller.state.path.edges.map(edge=>edge.id)),['a','b']);
  assert.deepEqual([...streams.active.keys()],['invert','light','source']);
  assert.equal(controller.state.stale,false);
  const source={nodeId:'source',portId:'button:0'},destination={nodeId:'light',portId:'indicator'};
  assert.equal(profileActivityPort(controller.state,source).sample.value.value,false);
  assert.equal(profileActivityPort(controller.state,destination).sample.value.value,true);
  assert.ok(Object.isFrozen(controller.state.captures));
  assert.ok(Object.isFrozen(controller.state.captures[0].ports[0].sample));
  const reads=calls.length;
  const updated={...port('source','button:0','output',known(true)),observedUs:'111'};
  streams.emit('source',{status:'ready',value:streams.value(owner(),'source',[updated],'110')});
  assert.equal(profileActivityPort(controller.state,source).sample.value.value,true);
  assert.equal(profileActivityPort(controller.state,source).observedUs,'111');
  assert.equal(controller.state.captures.find(capture=>capture.nodeId==='source').capturedThroughUs,'110');
  assert.equal(calls.length,reads);
  assert.ok(calls.every(call=>call.command==='profile_runtime_topology'));
  await controller.open(owner(),'simulator');
  assert.equal(calls.length,reads);
  assert.equal(controller.state.edgeId,'command');
  assert.deepEqual([...streams.active.keys()],['simulator','source']);
});

test('path and owner changes release old subscriptions and ignore callbacks already in flight',async()=>{
  const {controller,topology,streams}=harness(),first=deferred(),second=deferred();
  let loads=0;
  topology.load=()=>++loads===1?first.promise:second.promise;
  const old=controller.open(owner()),newOwner={...owner(),generation:4},current=controller.open(newOwner,'invert');
  second.resolve({...graph(),owner:newOwner});
  assert.equal(await current,true);
  first.resolve(graph());
  assert.equal(await old,false);
  const retired=streams.active.get('source');
  await controller.selectEdge('command',false);
  assert.equal(controller.state.expanded,false);
  retired.listener({status:'lost',error:'Old path lost.'});
  assert.equal(controller.state.stale,false);
  assert.equal(controller.state.error,'');
  const currentListener=streams.active.get('source');
  controller.dispose();
  assert.equal(streams.active.size,0);
  currentListener.listener({status:'ready',value:streams.value(newOwner,'source',[])});
  assert.equal(controller.state.owner,null);
  assert.equal(controller.state.captures.length,0);
  assert.equal(await controller.open(owner()),false);
});

test('all exact baselines are required and loss or regressing samples keep prior data stale',async()=>{
  const {controller,streams}=harness();
  const initial=streams.initial;
  streams.initial=()=>({status:'connecting'});
  await controller.open(owner(),'invert');
  assert.equal(controller.state.loading,true);
  for(const nodeId of ['invert','light'])streams.emit(nodeId,initial(owner(),nodeId));
  assert.equal(controller.state.stale,true);
  streams.emit('source',initial(owner(),'source'));
  assert.equal(controller.state.loading,false);
  assert.equal(controller.state.stale,false);

  const baseline=initial(owner(),'source').value;
  for(const changed of [
    {...baseline,nodeId:'other'},
    {...baseline,programHash:'different'},
    {...baseline,owner:{...baseline.owner,generation:4}},
    {...baseline,appliedSnapshot:{...baseline.appliedSnapshot,semanticHash:'different'}},
    {...baseline,capturedUs:'100'},
    {...baseline,ports:[]},
    {...baseline,ports:baseline.ports.map(port=>({...port,direction:'input'}))},
  ]){
    streams.emit('source',{status:'ready',value:changed});
    assert.equal(controller.state.stale,true);
    assert.match(controller.state.error,/no longer match/);
    assert.equal(controller.state.captures.length,3);
    streams.emit('source',{status:'ready',value:baseline});
    assert.equal(controller.state.stale,false);
  }
  streams.emit('source',{status:'lost',error:'Source lost.'});
  assert.equal(controller.state.stale,true);
  assert.equal(controller.state.error,'Source lost.');
  streams.emit('source',initial(owner(),'source'));
  assert.equal(controller.state.stale,false);
  const count=streams.opened.length;
  await controller.refresh();
  assert.equal(streams.opened.length,count+3);
  assert.equal(streams.active.size,3);
});

test('disconnected applied blocks still expose declared ports and draft-only selections release every reader',async()=>{
  const data=graph();
  data.nodes.push(node('unused','device'));
  const {controller,streams}=harness(data);
  await controller.open(owner(),'unused');
  assert.equal(controller.state.path.nodes[0].id,'unused');
  assert.equal(controller.state.path.edges.length,0);
  assert.equal(controller.state.stale,false);
  assert.equal(controller.state.captures[0].ports.length,0);
  assert.equal(await controller.selectBlock('new-draft-only'),false);
  assert.match(controller.state.error,/absent from this applied revision/);
  assert.equal(controller.state.captures.length,0);
  assert.equal(streams.active.size,0);
});

test('duplicate same-owner opens share pending topology and cleared requests cannot publish late errors',async()=>{
  const {controller,topology,streams}=harness(),wait=deferred();
  let loads=0;
  topology.load=()=>{loads++;return wait.promise;};
  const opening=controller.open(owner(),'invert');
  assert.equal(await controller.open(owner(),'invert'),false);
  assert.equal(await controller.refresh(),false);
  assert.equal(loads,1);
  assert.equal(controller.state.loading,true);
  assert.equal(streams.active.size,0);
  controller.clear();
  wait.reject(new Error('Old topology failure.'));
  assert.equal(await opening,false);
  assert.equal(controller.state.error,'');
  topology.load=async()=>{throw new Error('Native topology unavailable.');};
  assert.equal(await controller.open(owner()),false);
  assert.match(controller.state.error,/Native topology unavailable/);
});
