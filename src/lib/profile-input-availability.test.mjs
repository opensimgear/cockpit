import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';

const {inputAvailability,portBindingCounts}= (await loadModules(['./profile-input-availability'])).get('./profile-input-availability');
const choice=(id,direction='input')=>({id,direction});
const wire=(id,nodeId,portId)=>({id,from:{nodeId:'source',portId:'out'},to:{nodeId,portId}});
const plain=value=>JSON.parse(JSON.stringify(value));

test('generic drop stays available until every known input has a writer',()=>{
  const ports=[choice('first'),choice('second'),choice('out','output')];
  const document={bindings:[wire('route-1','target','first')]};
  assert.deepEqual(plain(inputAvailability(document,'target',ports)),{full:false,total:2,available:1,reason:''});
  document.bindings.push(wire('route-2','target','second'));
  assert.deepEqual(plain(inputAvailability(document,'target',ports)),{full:true,total:2,available:0,reason:'All inputs are connected.'});
});

test('an occupied named input is disabled while its other inputs remain available',()=>{
  const document={bindings:[wire('route-1','target','first')]},ports=[choice('first'),choice('second')];
  assert.deepEqual(plain(inputAvailability(document,'target',ports,'first')),{full:true,total:2,available:1,reason:'This input is already connected.'});
  assert.deepEqual(plain(inputAvailability(document,'target',ports,'second')),{full:false,total:2,available:1,reason:''});
});

test('reconnecting a wire frees its own input without freeing other occupied inputs',()=>{
  const document={bindings:[wire('route-1','target','first'),wire('route-2','target','second')]};
  const ports=[choice('first'),choice('second')];
  assert.equal(inputAvailability(document,'target',ports).full,true);
  assert.deepEqual(plain(inputAvailability(document,'target',ports,undefined,'route-1')),{full:false,total:2,available:1,reason:''});
  assert.equal(inputAvailability(document,'target',ports,'first','route-1').full,false);
  assert.equal(inputAvailability(document,'target',ports,'second','route-1').full,true);
  assert.equal(inputAvailability(document,'target',ports,'second','not-a-route').full,true);
});

test('discovered inputs count alongside saved ports without inferring names or observed identities',()=>{
  const observed='@observed:["rpm","sha256:abc"]',otherVariant='@observed:["rpm","sha256:def"]';
  const ports=[{...choice('logical-rpm'),name:'RPM'}, {...choice(observed),name:'RPM'},choice(otherVariant)];
  const document={bindings:[wire('route-1','target','logical-rpm')]};
  assert.deepEqual(plain(inputAvailability(document,'target',ports)),{full:false,total:3,available:2,reason:''});
  assert.equal(inputAvailability(document,'target',ports,observed).full,false);
  assert.equal(inputAvailability(document,'target',ports,otherVariant).full,false);
  // The discovered choice is replaced by its exact saved ID after authoring.
  const captured=[choice('logical-rpm'),choice('port-2'),choice(otherVariant)];
  document.bindings.push(wire('route-2','target','port-2'));
  assert.deepEqual(plain(inputAvailability(document,'target',captured)),{full:false,total:3,available:1,reason:''});
});

test('bindings to other nodes, absent ports and outputs cannot consume a known input',()=>{
  const document={bindings:[wire('other-node','elsewhere','in'),wire('removed-port','target','removed'),wire('invalid-direction','target','out')]};
  const ports=[choice('in'),choice('out','output')];
  assert.deepEqual(plain(inputAvailability(document,'target',ports)),{full:false,total:1,available:1,reason:''});
  assert.match(inputAvailability(document,'target',ports,'out').reason,/no longer available/);
  assert.equal(inputAvailability(document,'target',ports,'out').full,true);
});

test('empty catalogues are not treated as fully connected and counts use unique input identities',()=>{
  const document={bindings:[wire('route-1','target','in'),wire('duplicate-invalid-writer','target','in')]};
  assert.deepEqual(plain(inputAvailability(document,'target',[])),{full:false,total:0,available:0,reason:''});
  assert.deepEqual(plain(inputAvailability(document,'target',[choice('out','output')])),{full:false,total:0,available:0,reason:''});
  assert.deepEqual(plain(inputAvailability(document,'target',[choice('in'),choice('in')])),{full:true,total:1,available:0,reason:'All inputs are connected.'});
});

test('availability leaves its frozen document and choices unchanged',()=>{
  const binding=wire('route-1','target','in');Object.freeze(binding.to);Object.freeze(binding.from);Object.freeze(binding);
  const document=Object.freeze({bindings:Object.freeze([binding])}),ports=Object.freeze([Object.freeze(choice('in'))]);
  const before=JSON.stringify({document,ports});
  assert.equal(inputAvailability(document,'target',ports,'in').full,true);
  assert.equal(JSON.stringify({document,ports}),before);
});

test('port counts report unique input and output bindings against their directional catalogues',()=>{
  const ports=[choice('in-a'),choice('in-a'),choice('in-b'),choice('out-a','output'),choice('out-b','output')];
  const document={bindings:[
    {id:'a',from:{nodeId:'target',portId:'out-a'},to:{nodeId:'sink',portId:'in-a'}},
    {id:'b',from:{nodeId:'target',portId:'out-a'},to:{nodeId:'sink',portId:'in-b'}},
    {id:'c',from:{nodeId:'target',portId:'out-b'},to:{nodeId:'target',portId:'in-a'}},
  ]};
  assert.deepEqual(plain(portBindingCounts(document,'target',ports,'input')),{bound:1,total:2});
  assert.deepEqual(plain(portBindingCounts(document,'target',ports,'output')),{bound:2,total:2});
});

test('counts ignore other nodes, wrong directions, and endpoint ports removed from the catalogue',()=>{
  const ports=[choice('in'),choice('out','output')];
  const document={bindings:[
    wire('input-to-target','target','in'),
    {id:'known-output',from:{nodeId:'target',portId:'out'},to:{nodeId:'sink',portId:'in'}},
    {id:'other-source',from:{nodeId:'elsewhere',portId:'out'},to:{nodeId:'sink',portId:'in'}},
    {id:'removed-input',from:{nodeId:'target',portId:'gone'},to:{nodeId:'sink',portId:'in'}},
    {id:'removed-output',from:{nodeId:'target',portId:'removed'},to:{nodeId:'sink',portId:'in'}},
  ]};
  assert.deepEqual(plain(portBindingCounts(document,'target',ports,'input')),{bound:1,total:1});
  assert.deepEqual(plain(portBindingCounts(document,'target',ports,'output')),{bound:1,total:1});
});

test('port counts do not mutate frozen graph inputs or catalogue',()=>{
  const binding=wire('route','target','in');Object.freeze(binding.to);Object.freeze(binding.from);Object.freeze(binding);
  const document=Object.freeze({bindings:Object.freeze([binding])});
  const ports=Object.freeze([Object.freeze(choice('in')),Object.freeze(choice('out','output'))]);
  const before=JSON.stringify({document,ports});
  assert.deepEqual(plain(portBindingCounts(document,'target',ports,'input')),{bound:1,total:1});
  assert.equal(JSON.stringify({document,ports}),before);
});
