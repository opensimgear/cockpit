import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-editor','./profile-debug-controller','./profile-debug-graph','./profile-graph','./profile-canvas','./profile-discovery','./profile-palette']);
const {blankProfile}=modules.get('./profile-editor');
const {debugControllerResource}=modules.get('./profile-debug-controller');
const {graphSetDebugController}=modules.get('./profile-debug-graph');
const {graphConnect,graphDraft,graphSetVirtual}=modules.get('./profile-graph');
const {canvasNodes,canvasStatus}=modules.get('./profile-canvas');
const {profileDiscoveredPorts}=modules.get('./profile-discovery');
const {profilePaletteCanAdd,profilePaletteComponentValid}=modules.get('./profile-palette');
const plain=value=>JSON.parse(JSON.stringify(value));

const empty=()=>({document:blankProfile('b73245e8-2279-4e15-8872-90c497f66f91','Debug test'),assignments:[]});

test('debug input blocks work without inventory, local assignments or native output slots',()=>{
  const graph=graphSetDebugController(empty(),debugControllerResource('debug-1'));
  assert.deepEqual(plain(graph.assignments),[]);
  assert.equal(profilePaletteComponentValid({kind:'debug-controller'}),true);
  assert.equal(profilePaletteCanAdd({kind:'debug-controller'},graph.document,[],[]),true);
  const node=canvasNodes(graph.document,[],[])[0];
  assert.equal(node.category,'Debug controller');
  assert.equal(canvasStatus(node),'Profile input');
  const ports=profileDiscoveredPorts(graph,'debug-1');
  assert.equal(ports.length,10);
  assert.ok(ports.every(port=>port.direction==='output'&&port.current&&port.sourceId===null));
});

test('removing a configured control retires only its routes in the same edit',()=>{
  let graph=graphSetDebugController(empty(),debugControllerResource('debug-1'));
  graph=graphSetVirtual(graph,{id:'virtual-1',name:'Virtual',required:false,layout:{buttons:['button-1'],axes:[{portId:'axis-1',range:'bipolar',fallback:0}],hats:[]}});
  graph=graphConnect(graph,{nodeId:'debug-1',portId:'axis-2'},{nodeId:'virtual-1',portId:'axis-1'});
  graph=graphConnect(graph,{nodeId:'debug-1',portId:'button-1'},{nodeId:'virtual-1',portId:'button-1'});
  graph.document.editor.wireRoutes=graph.document.bindings.map(binding=>({bindingId:binding.id,points:[{x:0,y:0},{x:100,y:0}]}));
  const before=structuredClone(graph),resource=structuredClone(graph.document.resources[0]);
  resource.ports=resource.ports.filter(port=>port.id!=='axis-2');
  const next=graphSetDebugController(graph,resource);
  assert.equal(next.document.bindings.length,1);
  assert.equal(next.document.bindings[0].from.portId,'button-1');
  assert.deepEqual(plain(next.document.editor.wireRoutes).map(route=>route.bindingId),[next.document.bindings[0].id]);
  assert.deepEqual(plain(graph),before,'the undo source remains intact');
});

test('debug controllers retain vehicle scope and reject external hardware assignments',()=>{
  const initial=empty();
  initial.document.eligibility.gameIds=['dcs'];
  initial.document.vehicleTabs=[{id:'aircraft',gameId:'dcs',vehicleId:'test-plane',vehicleName:'Test plane',nodeIds:[]}];
  const graph=graphSetDebugController(initial,debugControllerResource('debug-1'),'aircraft');
  assert.deepEqual(plain(graph.document.vehicleTabs[0].nodeIds),['debug-1']);
  const assigned=structuredClone(graph);
  assigned.assignments=[{resourceId:'debug-1',sourceId:'external',ports:assigned.document.resources[0].ports.map(port=>({profilePortId:port.id,observedPortId:port.id,expectedContractHash:`sha256:${'a'.repeat(64)}`}))}];
  assert.throws(()=>graphDraft(assigned),/Debug controllers do not use local hardware assignments/);
  assert.throws(()=>graphSetDebugController(graph,{...debugControllerResource('virtual-1'),kind:'device'}),/debug controller/i);
});
