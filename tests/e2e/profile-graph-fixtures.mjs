// Pure portable graph builder; no process or hardware owner.
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
export function setterProfile(blank,resource,observed,value){
  const contract=structuredClone(observed.descriptor.contract),parameter=contract.parameters[0];
  assert.equal(contract.delivery,'action');assert.equal(contract.semantics,'setter');assert.equal(contract.readable,true);
  assert.equal(contract.ownership?.restorePolicy,'if-unchanged');assert.equal(contract.ownership?.fallbackPolicy,'capture-at-claim');
  assert.equal(contract.parameters.length,1);
  const state={delivery:'state',schema:parameter.schema,freshnessMs:1000,fallback:null};
  const output={delivery:'invocation',parameters:contract.parameters,semantics:'setter'};
  const port=(id,direction,contract)=>({id,name:id,direction,contract});
  const document=structuredClone(blank);document.id=randomUUID();document.name='Owned simulated output';
  document.resources=[{id:'panel',name:'Output panel',kind:'device',required:true,ports:[port('set','input',contract)]}];
  document.processingNodes=[{id:'desired',name:'Desired state',operation:{kind:'constant',value},inputs:[],outputs:[port('value','output',state)]},{id:'setter',name:'Explicit setter',operation:{kind:'set-on-change',arguments:[parameter.id]},inputs:[port(parameter.id,'input',state)],outputs:[port(parameter.id==='requested'?'invocation':'requested','output',output)]}];
  document.bindings=[{id:'argument',from:{nodeId:'desired',portId:'value'},to:{nodeId:'setter',portId:parameter.id}},{id:'invoke',from:{nodeId:'setter',portId:parameter.id==='requested'?'invocation':'requested'},to:{nodeId:'panel',portId:'set'}}];
  const assignments=[{resourceId:'panel',sourceId:resource.id,ports:[{profilePortId:'set',observedPortId:observed.descriptor.id,expectedContractHash:observed.contractHash}]}];
  return {document,assignments};
}
