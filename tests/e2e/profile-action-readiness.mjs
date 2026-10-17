// Passive metadata only. No discovery command, resource token or output request.
import assert from 'node:assert/strict';

export function currentActionReady(details,expected){
  const resource=details?.resource;
  assert.equal(resource?.id,expected.resourceId,'The exact retained physical resource must remain selected');
  assert.equal(resource.resourceType,'osg-module');
  assert.equal(resource.attributes?.gatewayId,expected.gatewayId);
  assert.equal(resource.attributes?.moduleId,expected.moduleId);
  assert.ok(['online','offline','unavailable'].includes(resource.availability));
  assert.ok(Array.isArray(resource.connections));
  assert.ok(Array.isArray(details.ports));
  assert.equal(details.offset,0);assert.ok(Number.isInteger(details.total)&&details.total>=0&&details.total<=128);
  assert.equal(details.ports.length,details.total,'Read one complete bounded native catalogue page');
  const ports=details.ports.filter(port=>port.descriptor?.id===expected.portId);
  assert.ok(ports.length<=1,'The selected action must be unambiguous');
  if(!ports.length)return false;
  const port=ports[0];
  assert.equal(port.contractHash,expected.contractHash,'The original exact action contract must remain unchanged');
  assert.equal(port.descriptor.direction,'input');assert.equal(port.descriptor.contract?.delivery,'action');
  assert.equal(typeof port.current,'boolean');
  for(const observed of details.ports)assert.equal(typeof observed.current,'boolean');
  if(resource.availability!=='online'||!port.current||details.ports.some(observed=>!observed.current))return false;
  assert.ok(resource.connections.length<=1,'No additional physical publisher may substitute for the captured CAN route');
  if(!resource.connections.length)return false;
  const connection=resource.connections[0],route=connection.route;
  assert.equal(connection.scope,`gateway:${expected.gatewayId}`);assert.equal(connection.key,expected.moduleId);
  assert.equal(route?.transport,'can');
  const routeId=`can:${expected.gatewayId}:${expected.moduleId}`;
  assert.equal(route.id,routeId);
  assert.equal(route.attributes?.gatewayId,expected.gatewayId);assert.equal(route.attributes?.moduleId,expected.moduleId);
  assert.equal(route.attributes?.node,String(expected.node));assert.equal(route.attributes?.boot,String(expected.boot));
  let generation;try{generation=JSON.parse(route.providerGeneration);}catch{assert.fail('The native CAN provider generation must be present');}
  assert.ok(Array.isArray(generation)&&generation.length===5);
  assert.deepEqual(generation.slice(0,4),[expected.epoch,expected.boot,expected.lease,expected.configurationGeneration],'Only the captured physical route/configuration may become ready');
  assert.ok(generation[4]===null||(Number.isInteger(generation[4])&&generation[4]>0&&generation[4]<=0xffffffff));
  assert.equal(typeof route.writable,'boolean');if(!route.writable)return false;
  assert.deepEqual(port.routeIds,[routeId],'Current action must belong to that exact verified route');
  return true;
}

export async function waitForCurrentAction(inspect,expected,wait){
  // Reuse the caller's existing bounded passive wait. Never retry Apply or
  // replace the saved assignment/contract with newly discovered metadata.
  return wait(inspect,details=>currentActionReady(details,expected),'same complete current native action catalogue after reconnect');
}
