import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-game-context']);
const {profileReconcileDcsVehicleCatalogue:reconcile}=modules.get('./profile-game-context');
const document=JSON.parse(await readFile(new URL('../../examples/dcs-bios/feature-lab.profile.json',import.meta.url),'utf8'));
const plain=value=>JSON.parse(JSON.stringify(value)),hash=`sha256:${'a'.repeat(64)}`;
const source=module=>({id:`native:${module}`,resourceType:'game-integration',attributes:{gameId:'dcs',aircraftModule:module,vehicleAliases:JSON.stringify([module])}});

test('opening each aircraft in the actual unassigned imported Feature Lab preserves every graph object',()=>{
 const value={document:plain(document),assignments:[]},before=plain(value);
 for(const tab of value.document.vehicleTabs){
  const next=reconcile(value,tab.id,source(tab.vehicleId),[],[]);
  assert.deepEqual(plain(next),before);assert.deepEqual(plain(value),before);
  assert.notEqual(next,value);assert.notEqual(next.document,value.document);
 }
});

test('configured assigned imports still reject stale exact contract pins without losing routes',()=>{
 const value={document:plain(document),assignments:[]},tab=value.document.vehicleTabs[0],block=value.document.resources.find(resource=>resource.kind==='game'&&tab.nodeIds.includes(resource.id));
 value.assignments=[{resourceId:block.id,sourceId:'old-game-wide-source',ports:block.ports.map(port=>({profilePortId:port.id,observedPortId:`native:${port.id}`,expectedContractHash:hash}))}];
 const before=plain(value),ports=block.ports.map(port=>({descriptor:{...plain(port),id:`native:${port.id}`},contractHash:`sha256:${'b'.repeat(64)}`,current:true,routeIds:[]}));
 assert.throws(()=>reconcile(value,tab.id,source(tab.vehicleId),ports,[]),/cannot preserve/);
 assert.deepEqual(plain(value),before);assert.ok(value.document.bindings.some(binding=>binding.from.nodeId===block.id));
});
