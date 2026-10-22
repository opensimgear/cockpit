import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';
const {laneEndpointWires:wires}=(await loadModules(['./profile-lane-wires'])).get('./profile-lane-wires');
const rect=(left,right,top=0,bottom=30)=>({left,right,top,bottom});
const plain=value=>JSON.parse(JSON.stringify(value));
test('lane connectors attach to every source and the first and last compact transform',()=>{
 const result=wires([rect(0,100,0,28),rect(0,100,31,59)],[rect(124,236,14,44),rect(270,382,14,44)],rect(600,750,14,44),rect(116,420,0,59),'smooth',24);
 assert.deepEqual(plain(result.map(wire=>wire.side)),['source','source','destination']);
 assert.match(result[0].path,/^M100,14/);assert.match(result[1].path,/^M100,45/);
 assert.match(result[0].path,/124,29$/);assert.match(result[2].path,/^M382,29/);assert.match(result[2].path,/600,29$/);
 assert.ok(result.every(wire=>!wire.path.includes('NaN')));
});
test('direct rows connect source to destination and preserve canvas arrow preferences',()=>{
 const sources=[rect(0,100)],destination=rect(600,750,40,70),track=rect(116,420);
 const smooth=wires(sources,[],destination,track,'smooth',24),squared=wires(sources,[],destination,track,'orthogonal',0),rounded=wires(sources,[],destination,track,'orthogonal',24);
 assert.equal(smooth[0].side,'direct');assert.match(smooth[0].path,/C/);assert.match(squared[0].path,/L/);assert.doesNotMatch(squared[0].path,/A/);assert.match(rounded[0].path,/A/);
 assert.deepEqual(plain(wires([],[],destination,track,'smooth',24)),[]);
});
test('horizontal scrolling clips endpoint connectors to the visible chain boundaries',()=>{
 const result=wires([rect(0,100)],[rect(-30,82),rect(460,572)],rect(600,750),rect(116,420),'orthogonal',24);
 assert.match(result[0].path,/116,15$/);assert.match(result[1].path,/^M420,15/);
});
