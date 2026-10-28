import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-route-endpoints','./profile-editor']);
const {routeEndpointChoices:options,reconnectRouteEndpoint:reconnect}=modules.get('./profile-route-endpoints');
const {blankProfile}=modules.get('./profile-editor');
const {profileDiscoveredPorts:ports}=modules.get('./profile-discovery');
const {graphConnect,graphSetProcessing}=modules.get('./profile-graph');
const state={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null};
const port=(id,direction,contract=state)=>({id,name:id,direction,contract});
const ep=(nodeId,portId)=>({nodeId,portId});
const plain=value=>JSON.parse(JSON.stringify(value));
function fixture(){const document=blankProfile('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa');document.resources=[{id:'source',name:'Source',kind:'device',required:false,ports:[port('one','output'),port('two','output'),port('wrong','output',{delivery:'state',schema:{type:'number',min:0,max:1,unit:null},freshnessMs:1000,fallback:null})]},...['target','other'].map(id=>({id,name:id,kind:'device',required:false,ports:[port('in','input'),port('free','input')]}))];return graphConnect({document,assignments:[]},ep('source','one'),ep('target','in'));}
const edit=(side='from')=>({laneId:'route-1',side,endpoint:side==='from'?ep('source','one'):ep('target','in')});

test('route endpoint choices filter direction, incompatible contracts and occupied destination inputs',()=>{
 let value=fixture();value=graphConnect(value,ep('source','two'),ep('target','free'));
 assert.deepEqual(plain(options(value,edit(),ports(value,'source')).map(p=>p.key)),['one','two']);
 assert.deepEqual(plain(options(value,edit('to'),ports(value,'target')).map(p=>p.key)),['in']);
 assert.throws(()=>reconnect(value,edit('to'),ports(value,'target').find(p=>p.key==='free')),/compatible, available/);
 assert.throws(()=>reconnect(value,edit(),ports(value,'source').find(p=>p.key==='wrong')),/compatible, available/);
});
test('source and destination reconnect preserve the terminal route ID and the original draft',()=>{
 const value=fixture(),before=JSON.stringify(value);
 const source=reconnect(value,edit(),ports(value,'source').find(p=>p.key==='two'));
 assert.deepEqual(plain(source.document.bindings[0]),{id:'route-1',from:ep('source','two'),to:ep('target','in')});
 const target=reconnect(value,edit('to'),ports(value,'target').find(p=>p.key==='free'));
 assert.deepEqual(plain(target.document.bindings[0]),{id:'route-1',from:ep('source','one'),to:ep('target','free')});
 assert.equal(JSON.stringify(value),before);
 assert.throws(()=>reconnect(target,edit('to'),ports(target,'target')[0]),/endpoint changed/);
 assert.deepEqual(plain(options(target,edit('to'),ports(target,'target'))),[]);
});
test('source edits copy shared ancestors and preserve other terminal consumers and vehicle scope',()=>{
 let value=fixture();value.document.bindings=[];value.document.eligibility.gameIds=['dcs'];value.document.vehicleTabs=[{id:'aircraft',gameId:'dcs',vehicleId:'AH-6J',vehicleName:'AH-6J',nodeIds:[]}];
 const stage=id=>({id,name:id,operation:{kind:'identity'},inputs:[port('in','input')],outputs:[port('out','output')]});
 value=graphSetProcessing(value,stage('first'),'aircraft');value=graphSetProcessing(value,stage('second'),'aircraft');
 value=graphConnect(value,ep('source','one'),ep('first','in'));value=graphConnect(value,ep('first','out'),ep('second','in'));
 value=graphConnect(value,ep('second','out'),ep('target','in'));value=graphConnect(value,ep('second','out'),ep('other','in'));
 const terminal=value.document.bindings.find(b=>b.to.nodeId==='target'),other=value.document.bindings.find(b=>b.to.nodeId==='other');
 const next=reconnect(value,{...edit(),laneId:terminal.id},ports(value,'source').find(p=>p.key==='two'));
 assert.deepEqual(plain(next.document.bindings.find(b=>b.id===other.id)),plain(other));
 assert.ok(next.document.bindings.some(b=>b.from.nodeId==='source'&&b.from.portId==='one'&&b.to.nodeId==='first'));
 assert.equal(next.document.processingNodes.length,4);
 assert.equal(next.document.vehicleTabs[0].nodeIds.length,4);
 const changed=next.document.bindings.find(b=>b.id===terminal.id);assert.notEqual(changed.from.nodeId,'second');
 assert.ok(next.document.bindings.some(b=>b.from.nodeId==='source'&&b.from.portId==='two'&&b.to.nodeId!=='first'));
});
test('discovered source selection captures its exact provider contract in the same edit',()=>{
 const value=fixture(),hash='sha256:'+ '1'.repeat(64);value.assignments=[{resourceId:'source',sourceId:'physical',ports:value.document.resources[0].ports.map(port=>({profilePortId:port.id,observedPortId:port.id,expectedContractHash:hash}))}];
 const observed={descriptor:port('advertised-two','output'),contractHash:hash,current:true,routeIds:['usb']};
 const choices=ports(value,'source',{resource:{id:'physical'},ports:[observed]});
 const next=reconnect(value,edit(),choices.find(p=>p.observed?.descriptor.id==='advertised-two'));
 const pin=next.assignments[0].ports.find(pin=>pin.observedPortId==='advertised-two');
 assert.ok(pin);assert.equal(pin.expectedContractHash,hash);assert.equal(next.document.bindings[0].from.portId,pin.profilePortId);
 assert.equal(value.assignments[0].ports.length,3);
});

test('vehicle endpoint choices and reconnect ignore only mutually exclusive destination writers',()=>{
 let value=fixture();value.document.eligibility.gameIds=['dcs'];
 value.document.resources.push({...plain(value.document.resources[0]),id:'viper'});
 value.document.vehicleTabs=[{id:'hornet',gameId:'dcs',vehicleId:'hornet',vehicleName:'Hornet',nodeIds:['source']},{id:'viper-tab',gameId:'dcs',vehicleId:'viper',vehicleName:'Viper',nodeIds:['viper']}];
 value=graphConnect(value,ep('viper','one'),ep('target','free'));
 const prior=plain(value.document.bindings.find(binding=>binding.from.nodeId==='viper'));
 assert.deepEqual(plain(options(value,edit('to'),ports(value,'target')).map(port=>port.key)),['in','free']);
 const next=reconnect(value,edit('to'),ports(value,'target').find(port=>port.key==='free'));
 assert.equal(next.document.bindings.length,2);
 assert.deepEqual(plain(next.document.bindings.find(binding=>binding.id===prior.id)),prior);
 assert.deepEqual(plain(next.document.bindings.find(binding=>binding.id==='route-1').to),ep('target','free'));
});
