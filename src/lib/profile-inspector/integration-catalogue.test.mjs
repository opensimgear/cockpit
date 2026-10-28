import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from '../test-module-loader.mjs';
const modules=await loadModules(['./profile-inspector/integration-catalogue']);
const {integrationCataloguePorts:collect}=modules.get('./profile-inspector/integration-catalogue');
const plain=value=>JSON.parse(JSON.stringify(value)),hash=`sha256:${'a'.repeat(64)}`;
const state={delivery:'state',schema:{type:'integer',min:0,max:65535,unit:null},freshnessMs:1500,fallback:null};
const action={delivery:'action',parameters:[{id:'value',name:'Value',schema:{type:'integer',min:0,max:65535,unit:null}}],semantics:'setter',endpoint:'LIGHT',readable:false,display:null,ownership:null};
const observed=(id,direction,contract)=>({descriptor:{id,name:id,direction,contract:plain(contract)},contractHash:hash,current:true,routeIds:['native']});
function fixture(){return {value:{document:{resources:[{id:'game',kind:'game',ports:[{id:'saved',name:'Saved telemetry',direction:'output',contract:plain(state)}]}]},assignments:[]},details:{resource:{id:'native',resourceType:'game-integration'},ports:[observed('channel:LIGHT:0','output',state),observed('command:LIGHT:set_state','input',action)]}};}

test('configured unassigned Base and aircraft expose both catalogue directions without mutation',()=>{
 for(const vehicleId of ['', 'AH-6J']){
  const {value,details}=fixture();value.document.vehicleTabs=vehicleId?[{vehicleId,nodeIds:['game']}]:[];
  const before=plain({value,details}),ports=collect(value,'game','native',details);
  assert.deepEqual(ports.map(port=>port.direction),['output','input']);assert.equal(ports.length,2);
  assert.ok(ports.every(port=>port.profilePortId===null&&port.key.startsWith('@observed:')));
  assert.deepEqual(plain({value,details}),before);
 }
});

test('missing details, wrong sources, wrong resource categories and stale native ports are ignored',()=>{
 const {value,details}=fixture();
 for(const page of [null,undefined,{...details,resource:{...details.resource,id:'different'}},{...details,resource:{...details.resource,resourceType:'host-controller'}}])assert.deepEqual(plain(collect(value,'game','native',page)),[]);
 assert.deepEqual(plain(collect(value,'missing','native',details)),[]);
 value.document.resources[0].kind='device';assert.deepEqual(plain(collect(value,'game','native',details)),[]);
 value.document.resources[0].kind='game';details.ports[0].current=false;assert.equal(collect(value,'game','native',details).length,1);
});

test('only exact assignment source, native identity, hash, direction and contract keep saved identity',()=>{
 const {value,details}=fixture();value.assignments=[{resourceId:'game',sourceId:'native',ports:[{profilePortId:'saved',observedPortId:'channel:LIGHT:0',expectedContractHash:hash}]}];
 const port=collect(value,'game','native',details)[0];assert.equal(port.profilePortId,'saved');assert.equal(port.key,'saved');assert.equal(port.name,'Saved telemetry');
 for(const alter of [v=>v.assignments[0].sourceId='other',v=>v.assignments[0].ports[0].observedPortId='other',v=>v.assignments[0].ports[0].expectedContractHash=`sha256:${'b'.repeat(64)}`,v=>v.document.resources[0].ports[0].direction='input',v=>v.document.resources[0].ports[0].contract.freshnessMs=500]){
  const changed=plain(value);alter(changed);assert.equal(collect(changed,'game','native',details)[0].profilePortId,null);
 }
});

test('action parameter display names do not change exact native contract compatibility',()=>{
 const {value,details}=fixture();value.document.resources[0].ports=[{id:'setter',name:'Saved setter',direction:'input',contract:plain(action)}];
 value.document.resources[0].ports[0].contract.parameters[0].name='My value';
 value.assignments=[{resourceId:'game',sourceId:'native',ports:[{profilePortId:'setter',observedPortId:'command:LIGHT:set_state',expectedContractHash:hash}]}];
 assert.equal(collect(value,'game','native',details)[1].profilePortId,'setter');
});
