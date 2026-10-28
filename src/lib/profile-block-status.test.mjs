import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';

const {canvasBlockStatus}=(await loadModules(['./profile-block-status'])).get('./profile-block-status');
const plain=value=>JSON.parse(JSON.stringify(value));
const node=(availability='online',kind='device',attributes={})=>({
  id:'block',name:'Controller',kind:'resource',category:'Controller',position:{x:0,y:0},
  resource:{id:'block',kind},assignment:{sourceId:'source'},
  observation:{resourceType:kind==='game'?'game-integration':'host-controller',availability,attributes},
});
const status=(value,context)=>plain(canvasBlockStatus(value,context));

test('block dots distinguish available, offline, unresolved and unavailable resources',()=>{
  assert.deepEqual(status(node()),{tone:'accent',label:'Online'});
  assert.deepEqual(status(node('offline')),{tone:'muted',label:'Offline'});
  assert.deepEqual(status(node('unavailable')),{tone:'warning',label:'Unavailable'});
  assert.deepEqual(status({...node(),observation:undefined}),{tone:'warning',label:'Source removed from inventory'});
  assert.deepEqual(status({...node(),observation:undefined},{inventoryKnown:false}),{tone:'muted',label:'Checking availability…'});
  assert.deepEqual(status({...node(),assignment:undefined}),{tone:'warning',label:'Unassigned'});
  assert.deepEqual(status({...node(),resource:{kind:'debug-controller'},assignment:undefined,observation:undefined}),{tone:'accent',label:'Available'});
});

test('local outputs and complete transforms are ready while drafts and issues need attention',()=>{
  for(const kind of ['virtual','desktop','processing'])assert.deepEqual(status({kind}),{tone:'accent',label:'Ready'});
  assert.deepEqual(status({kind:'processing',processing:{draft:true}}),{tone:'warning',label:'Needs input'});
  assert.deepEqual(status(node(),{discovering:true}),{tone:'muted',label:'Discovering inputs and outputs…'});
  assert.deepEqual(status(node(),{needsPanel:true}),{tone:'warning',label:'Choose a panel'});
  assert.deepEqual(status(node('offline'),{error:'Port discovery failed'}),{tone:'warning',label:'Port discovery failed'});
  assert.deepEqual(status({kind:'virtual'},{issue:'No matching output slot'}),{tone:'warning',label:'No matching output slot'});
});

test('game dots follow native integration setup, installation and process state',()=>{
  const attributes={installed:'true',setupStatus:'ready',processVerified:'true'};
  assert.deepEqual(status(node('online','game',attributes)),{tone:'accent',label:'Running'});
  assert.deepEqual(status(node('unavailable','game',{...attributes,processVerified:'false'})),{tone:'muted',label:'Not running'});
  assert.deepEqual(status(node('unavailable','game',{...attributes,setupStatus:'setup-required'})),{tone:'warning',label:'Configuration needed'});
  assert.deepEqual(status(node('offline','game',{...attributes,installed:'false'})),{tone:'muted',label:'Missing'});
});

test('vehicle integrations stay muted until that vehicle is observed active and online',()=>{
  const attributes={installed:'true',setupStatus:'ready',processVerified:'true',aircraftModule:'F-16C_50'};
  const vehicle=node('online','game',attributes),context={vehicleId:'F-16C_50'};
  assert.deepEqual(status(vehicle,context),{tone:'muted',label:'Vehicle inactive'});
  assert.deepEqual(status(vehicle,{...context,activeVehicleId:'FA-18C_hornet'}),{tone:'muted',label:'Vehicle inactive'});
  assert.deepEqual(status(vehicle,{...context,activeVehicleId:context.vehicleId}),{tone:'accent',label:'Running'});
  assert.deepEqual(status(node('offline','game',attributes),{...context,activeVehicleId:context.vehicleId}),{tone:'muted',label:'Waiting for vehicle integration'});
  assert.deepEqual(status(node('online','game',{...attributes,currentVehicleId:context.vehicleId}),context),{tone:'accent',label:'Running'});
  assert.deepEqual(status(node('online','game',{...attributes,activeVehicleId:context.vehicleId}),context),{tone:'accent',label:'Running'});
});
