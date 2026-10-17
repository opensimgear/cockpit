import test from 'node:test';
import assert from 'node:assert/strict';
import {currentActionReady,waitForCurrentAction} from './profile-action-readiness.mjs';
const expected={resourceId:'retained-resource',gatewayId:'stm32:0123456789abcdef01234567',moduleId:'0123456789abcdef0123456789abcdef',portId:'action:led.rgb.set_color',contractHash:'sha256:'+'a'.repeat(64),epoch:65536,node:1,lease:65537,boot:1,configurationGeneration:1};
function detail(current=true){
  const routeId=`can:${expected.gatewayId}:${expected.moduleId}`;
  return {session:'new-native-inventory',revision:'2',offset:0,total:1,resource:{id:expected.resourceId,resourceType:'osg-module',availability:'online',attributes:{gatewayId:expected.gatewayId,moduleId:expected.moduleId},connections:[{scope:`gateway:${expected.gatewayId}`,key:expected.moduleId,route:{id:routeId,transport:'can',providerGeneration:'[65536,1,65537,1,null]',attributes:{gatewayId:expected.gatewayId,moduleId:expected.moduleId,node:'1',boot:'1'},baselineReady:false,writable:true,priority:50}}]},ports:[{descriptor:{id:expected.portId,direction:'input',contract:{delivery:'action'}},contractHash:expected.contractHash,routeIds:current?[routeId]:[],current}]};
}
test('retained catalogue is metadata until the same native route publishes current ports',async()=>{
  const states=[detail(false),detail(false),detail(true)],calls=[];
  const inspect=async()=>{calls.push('inspect');return states.shift();};
  const boundedWait=async(operation,predicate)=>{for(let i=0;i<3;i++){const value=await operation();if(predicate(value))return value;}assert.fail('bounded wait expired');};
  const ready=await waitForCurrentAction(inspect,expected,boundedWait);
  assert.equal(ready.ports[0].current,true);calls.push('one Apply');
  assert.deepEqual(calls,['inspect','inspect','inspect','one Apply']);
});
test('wrong resource, contract, module or physical route can never admit Apply',()=>{
  const changes=[
    d=>d.resource.id='other',d=>d.ports[0].contractHash='sha256:'+'b'.repeat(64),
    d=>d.resource.attributes.moduleId='other',d=>d.resource.attributes.gatewayId='other',
    d=>d.resource.connections[0].route.transport='usb',d=>d.resource.connections[0].route.id='other',
    d=>d.resource.connections[0].route.attributes.node='2',d=>d.resource.connections[0].route.attributes.boot='2',
    d=>d.resource.connections[0].route.providerGeneration='[65536,1,999,1,null]',
    d=>d.resource.connections.push(structuredClone(d.resource.connections[0])),
    d=>d.ports[0].routeIds=['other'],d=>d.ports[0].current='true',
  ];
  for(const mutate of changes){const value=detail();mutate(value);assert.throws(()=>currentActionReady(value,expected));}
  const wrongAction=detail();wrongAction.ports[0].descriptor.id='action:other';assert.equal(currentActionReady(wrongAction,expected),false);
});
test('offline, stale, absent and unwritable metadata stay not ready',()=>{
  assert.equal(currentActionReady(detail(false),expected),false);
  for(const mutate of [d=>d.resource.availability='offline',d=>d.resource.connections=[],d=>d.resource.connections[0].route.writable=false]){const value=detail();mutate(value);assert.equal(currentActionReady(value,expected),false);}
});
test('a second stale port prevents treating a partial catalogue as complete readiness',()=>{
  const value=detail();value.total=2;value.ports.push({...structuredClone(value.ports[0]),descriptor:{id:'action:led.boolean.set_enabled',direction:'input',contract:{delivery:'action'}},current:false,routeIds:[]});
  assert.equal(currentActionReady(value,expected),false);
});
test('native inspection errors propagate without an Apply attempt',async()=>{
  let applyCalls=0;
  await assert.rejects(async()=>{
    await waitForCurrentAction(async()=>{throw Error('native read failed');},expected,async(operation,predicate)=>{const value=await operation();return predicate(value)?value:null;});
    applyCalls++;
  },/native read failed/);
  assert.equal(applyCalls,0);
});
