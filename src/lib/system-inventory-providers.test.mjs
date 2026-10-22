import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./profiles', './profile-inventory', './gateway-capabilities', './controller-lifecycle', './games', './system-inventory', './profile-inspection', './profile-activity', './system-inventory-providers']);
const { SystemInventoryController, gatewayObservations, correlateSystem, integrationGameId } = modules.get('./system-inventory-providers');
const plain = value => JSON.parse(JSON.stringify(value));
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
const storage = () => { const values = new Map(); return { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) }; };
const source = (id = 'wheel', extra = {}) => ({ id, name: 'Controller ' + id, role: 'Joystick', backend: 'windows-raw-input', online: true, osgHid: false, baselineReady: true, controlCount: 12, identityConfidence: 'local-path', ...extra });
const usb = (key = 'serial', extra = {}) => ({ key, port: { path: 'COM7', friendlyName: 'USB panel' }, present: true, automatic: true, admissible: true, diagnostics: [], snapshot: { connected: true, port: 'COM7', identity: { moduleId: 'verified-panel', serial: 'serial', profile: 'USB panel', board: 'RP2040' }, claimedBy: null, error: null }, ...extra });
const module = (id = 'panel', extra = {}) => ({ id, name: 'CAN panel', kind: 'Button panel', online: true, active: true, enabled: true, registered: true, published: true, settings: {}, ...extra });
const gateway = (extra = {}) => ({ id: 'gateway-one', port: { backend: 'stm32', serialNumber: 'GW-1', path: 'COM2' }, present: true, busy: false, error: '', state: { backend: 'stm32', protocolVersion: 3, canProtocol: 3, lifecycle: 'automatic', connected: true, running: true, claimedModules: [], modules: [module()] }, ...extra });
const route = (id, transport = 'usb') => ({ id, transport, providerGeneration: '1', attributes: {}, baselineReady: true, writable: true, priority: 1 });
const resource = (id = 'physical-panel', connections = [], extra = {}) => ({ id, name: 'Verified physical panel', resourceType: 'osg-module', availability: 'online', generation: '1', capabilityRevision: 'cap-1', portCount: 0, cachedPortCount: 0, connections, attributes: {}, diagnostics: [], ...extra });
const connection = (scope, key, transport = 'usb') => ({ scope, key, route: route(`${scope}:${key}`, transport) });
const game = (id = 'dcs', extra = {}) => ({ id, name: 'DCS World', provider: 'dcs-bios', installed: true, launchSupported: true, integrationSetup: { state: 'ready', live: 'receiving', message: 'Ready', liveMessage: 'Telemetry observed', connection: { host: '127.0.0.1', port: 7778, secret: 'not-an-endpoint' } }, ...extra });

function harness(options = {}) {
  const state = { controllers: [], usb: [], resources: [], companions: { host: { clients: [], instances: [] } }, games: { catalogue: [] }, native: { virtualDevices: [], hiddenDevices: [], diagnostics: [] }, gateways: [], ...options };
  const requests = [], failures = new Map(), custom = new Map(), changes = [];
  const request = async input => {
    requests.push(plain(input));
    if (failures.has(input.command)) throw failures.get(input.command);
    if (custom.has(input.command)) return custom.get(input.command)(input);
    if (input.command === 'controller_list') return { controllers: { session: 'controllers-1', offset: input.offset, total: state.controllers.length, sources: state.controllers.slice(input.offset, input.offset + input.limit) } };
    if (input.command === 'module_usb_list') return { moduleUsb: { offset: input.offset, total: state.usb.length, modules: state.usb.slice(input.offset, input.offset + input.limit) } };
    if (input.command === 'profile_resources_list') return { profileResources: { session: 'inventory-1', revision: '1', offset: input.offset, total: state.resources.length, resources: state.resources.slice(input.offset, input.offset + input.limit) } };
    if (input.command === 'companion_status') return { companions: state.companions };
    if (input.command === 'profile_status') return { profiles: { profileSession: state.runtime ?? { state: 'stopped', owner: null, appliedSnapshot: null } } };
    if (input.command === 'game_get_state') return { games: state.games };
    if (input.command === 'system_inventory') return { systemInventory: state.native };
    if (input.command === 'profile_list') return { profiles: { profiles: [] } };
    if (['controller_forget', 'forget', 'companion_revoke'].includes(input.command)) return {};
    throw Error('Unexpected provider mutation: ' + input.command);
  };
  const controller = new SystemInventoryController({ request, gateways: () => state.gateways, gatewayError: () => state.gatewayError ?? '', storage: options.storage, onChange: snapshot => changes.push(snapshot) });
  return { controller, state, requests, failures, custom, changes, component: id => controller.snapshot.components.find(component => component.id === id) };
}

test('native supplemental pages preserve the exact snapshot and never omit hidden records', async () => {
 const h=harness();
 const hidden=Array.from({length:35},(_,i)=>({id:'pn-'+i,name:'Hidden '+i,instanceId:'HID/'+i,present:true,hidden:true,diagnostics:[]}));
 h.custom.set('system_inventory',r=>({systemInventory:{observedAt:123,total:35,offset:r.offset,nextOffset:r.offset===0?32:null,virtualDevices:[],diagnostics:[],hiddenDevices:hidden.slice(r.offset,r.offset+32)}}));
 await h.controller.refresh();assert.equal(h.controller.snapshot.components.filter(c=>c.hidden).length,35);
 assert.equal(h.requests.filter(r=>r.command==='system_inventory')[1].observedAt,123);
 h.custom.set('system_inventory',r=>({systemInventory:{observedAt:r.offset===0?123:124,total:35,offset:r.offset,nextOffset:r.offset===0?32:null,virtualDevices:[],diagnostics:[],hiddenDevices:hidden.slice(r.offset,r.offset+32)}}));
 await h.controller.refresh();assert.match(h.controller.snapshot.diagnostics.join(' '),/changed/);assert.equal(h.component('visibility:pn-0').stale,true);
});

test('native association-only pages fold published HID into its actual gateway or module without inventing physical host edges', async () => {
 const h=harness({gateways:[gateway()],controllers:[source('panel-hid',{osgHid:true}),source('gateway-hid',{osgHid:true})]});
 const associations=[{controllerId:'panel-hid',gatewayId:'gateway-one',moduleId:'panel',evidence:['exact-report']},{controllerId:'gateway-hid',gatewayId:'gateway-one',evidence:['verified-gateway-serial']}];
 h.custom.set('system_inventory',r=>({systemInventory:{observedAt:321,total:2,offset:r.offset,nextOffset:r.offset===0?1:null,hiddenDevices:[],controllerAssociations:associations.slice(r.offset,r.offset+1),virtualDevices:[],diagnostics:[]}}));
 await h.controller.refresh();assert.equal(h.component('controller:panel-hid'),undefined);assert.equal(h.component('controller:gateway-hid'),undefined);
 assert.equal(h.component('module:gateway-one/panel').connections.length,1);assert.equal(h.component('module:gateway-one/panel').connections[0].transport,'can');
 assert.ok(h.component('module:gateway-one/panel').targets.some(t=>t.id==='panel-hid'));assert.ok(h.component('gateway:gateway-one').targets.some(t=>t.id==='gateway-hid'));
});

test('hidden controller discovery survives capture outage and recovery without retaining stale aliases', async () => {
 const h=harness({controllers:[source('pedals')],native:{virtualDevices:[],hiddenDevices:[{id:'pnp-pedals',name:'PnP pedals',instanceId:'HID/pedals',controllerIds:['pedals'],present:true,hidden:true,diagnostics:[]}],diagnostics:[]}});
 await h.controller.refresh();assert.equal(h.controller.snapshot.components.filter(c=>c.type==='controller').length,1);
 h.failures.set('controller_list',Error('Capture unavailable'));await h.controller.refresh();assert.equal(h.controller.snapshot.components.filter(c=>c.type==='controller').length,1);assert.equal(h.component('controller:pedals').presence,'connected');
 h.failures.delete('controller_list');await h.controller.refresh();assert.equal(h.controller.snapshot.components.filter(c=>c.type==='controller').length,1);assert.equal(h.component('visibility:pnp-pedals'),undefined);
});

test('native hidden fallback retains the known OSG physical identity/type when capture fails', async () => {
 const h=harness({controllers:[source('osg-panel',{osgHid:true})],native:{virtualDevices:[],hiddenDevices:[{id:'pnp-osg',name:'OS PnP label',instanceId:'HID\\OSG',controllerIds:['osg-panel'],present:true,hidden:true,diagnostics:[]}],diagnostics:[]}});
 await h.controller.refresh();h.failures.set('controller_list',Error('Capture unavailable'));await h.controller.refresh();
 assert.equal(h.component('controller:osg-panel').type,'osg');assert.equal(h.component('controller:osg-panel').name,'Controller osg-panel');assert.equal(h.component('controller:osg-panel').presence,'connected');
});

test('removed HidHide rules clear cached hidden flags only after successful policy inspection', async () => {
 const h=harness({native:{virtualDevices:[],hiddenDevices:[{id:'old-hid',name:'Old HID',instanceId:'HID/OLD',present:false,configured:true,hidden:true,diagnostics:[]}],visibilityInspected:true,diagnostics:[]}});
 await h.controller.refresh();assert.equal(h.component('visibility:old-hid').forget.allowed,false);
 h.state.native.hiddenDevices=[];h.state.native.visibilityInspected=false;h.state.native.diagnostics=['HidHide visibility is unknown: driver unavailable'];
 await h.controller.refresh();assert.equal(h.component('visibility:old-hid').hidden,null);assert.equal(h.component('visibility:old-hid').forget.allowed,false);
 h.state.native.visibilityInspected=true;h.state.native.diagnostics=['Virtual device inventory is unavailable'];
 await h.controller.refresh();assert.equal(h.component('visibility:old-hid').hidden,false);assert.equal(h.component('visibility:old-hid').forget.allowed,true);
 await h.controller.forget('visibility:old-hid');assert.equal(h.component('visibility:old-hid'),undefined);
 assert.equal(h.requests.some(r=>['controller_forget','forget','companion_revoke'].includes(r.command)),false);
});

test('nearest native parent bus proves USB or Bluetooth without guessing from HID names, including remembered edges', async () => {
 const h=harness({controllers:[source('usb'),source('bluetooth'),source('offline',{online:false})],native:{virtualDevices:[],diagnostics:[],hiddenDevices:[
  {id:'usb',name:'USB wheel',instanceId:'HID\\USB',controllerIds:['usb'],present:true,hidden:false,ancestors:['USB\\VID_1234','USB\\ROOT_HUB'],diagnostics:[]},
  {id:'bt',name:'Bluetooth joystick',instanceId:'HID\\BT',controllerIds:['bluetooth'],present:true,hidden:false,ancestors:['BTHLEDEVICE\\peer','USB\\RADIO'],diagnostics:[]},
  {id:'old',name:'Offline pedals',instanceId:'HID\\OLD',controllerIds:['offline'],present:false,hidden:false,ancestors:['USB\\VID_1234'],diagnostics:[]},
 ]}});await h.controller.refresh();
 assert.equal(h.component('controller:usb').connections[0].transport,'usb');assert.equal(h.component('controller:bluetooth').connections[0].transport,'bluetooth');
 assert.equal(h.component('controller:offline').connections[0].current,false);assert.equal(h.component('controller:offline').presence,'offline');
});

test('verified multiple HID collections are capabilities of one physical node across capture outage', async () => {
 const h=harness({controllers:[source('col-a'),source('col-b')],native:{virtualDevices:[],diagnostics:[],hiddenDevices:[{id:'same-pnp',name:'Same device',instanceId:'HID\\COMPOSITE',controllerIds:['col-a','col-b'],present:true,hidden:false,diagnostics:[]}]}});
 await h.controller.refresh();assert.equal(h.controller.snapshot.components.filter(c=>c.type==='controller').length,1);assert.equal(h.component('controller:col-a').targets.filter(t=>t.kind==='controller').length,2);
 h.failures.set('controller_list',Error('Capture failed'));await h.controller.refresh();assert.equal(h.controller.snapshot.components.filter(c=>c.type==='controller').length,1);
});

test('exact physical USB parent folds sibling HID collections and rules while an unproved orphan remains separate', async () => {
 const parent='USB\\VID_0079&PID_0126\\exact-parent',container='40000000-0000-4000-8000-000000000001';
 const child=(id,controllerId)=>({id,name:'Composite controller',instanceId:'HID\\VID_0079&PID_0126\\'+id,controllerIds:[controllerId],parentInstanceId:parent,containerId:container,ancestors:[parent,'USB\\ROOT_HUB'],present:true,hidden:false,diagnostics:[]});
 const h=harness({controllers:[source('col-a'),source('col-b')],native:{virtualDevices:[],diagnostics:[],hiddenDevices:[
  child('child-a','col-a'),child('child-b','col-b'),{id:'parent',name:'USB physical parent',instanceId:parent,controllerIds:[],containerId:container,present:true,hidden:true,configured:true,diagnostics:[]},
  {id:'orphan',name:'Same vendor but unproved',instanceId:'HID\\VID_0079&PID_0126\\unresolved',controllerIds:[],parentInstanceId:null,containerId:null,ancestors:[],present:false,hidden:true,diagnostics:[]},
 ]}});
 await h.controller.refresh();
 assert.equal(h.controller.snapshot.components.filter(component=>component.type==='controller').length,2);
 const physical=h.component('controller:col-a');
 assert.ok(physical);assert.equal(physical.hidden,true);assert.equal(physical.presence,'connected');
 assert.deepEqual(plain([...new Set(physical.targets.filter(target=>target.kind==='controller').map(target=>target.id))].sort()),['col-a','col-b']);
 assert.equal(h.component('controller:col-b'),undefined);assert.equal(h.component('visibility:parent'),undefined);
 assert.ok(h.component('visibility:orphan'),'VID/PID similarity cannot absorb an unproved orphan rule');
 h.failures.set('controller_list',Error('Capture lost'));
 await h.controller.refresh();
 assert.equal(h.controller.snapshot.components.filter(component=>component.type==='controller').length,2);
});

test('an unavailable or stopped controller backend cannot turn remembered presence into confirmed offline or allow Forget', async () => {
 for(const backendStatus of ['unavailable','starting','stopped']){
  const h=harness({controllers:[source()],usb:[usb()]});await h.controller.refresh();
  const lastConnected=h.component('controller:wheel').lastSeen;
  h.custom.set('controller_list',request=>({controllers:{session:'controllers-1',offset:request.offset,total:1,sources:[source('wheel',{online:false})],backendStatus,diagnostic:'Capture access unavailable'}}));
  await h.controller.refresh();
  assert.equal(h.component('controller:wheel').presence,'unknown',backendStatus);
  assert.equal(h.component('controller:wheel').stale,true);
  assert.equal(h.component('controller:wheel').capture,'unavailable');
  assert.equal(h.component('controller:wheel').forget.allowed,false);
  assert.equal(h.component('controller:wheel').lastSeen,lastConnected);
  assert.equal(h.component('usb:serial').presence,'connected');
  assert.match(h.controller.snapshot.diagnostics.join(' '),/Capture access unavailable/);
  await assert.rejects(h.controller.forget('controller:wheel'),/cannot confirm|offline|eligible/i);
  assert.equal(h.requests.some(request=>request.command==='controller_forget'),false);
  h.custom.set('controller_list',request=>({controllers:{session:'controllers-1',offset:request.offset,total:0,sources:[],backendStatus,diagnostic:'Capture access unavailable'}}));
  await h.controller.refresh();assert.equal(h.component('controller:wheel').presence,'unknown','partial empty capture cannot prove absence');
 }
});

test('startup HidHide attention remains diagnostic without replacing current capture or connected-hidden evidence', async () => {
 const sources=[source('wheel'),source('pedals',{online:false})];
 const h=harness({controllers:sources,usb:[usb()],native:{virtualDevices:[],hiddenDevices:[{id:'pedals',name:'Hidden pedals',instanceId:'HID\\PEDALS',controllerIds:['pedals'],present:true,hidden:true,diagnostics:[]}],diagnostics:[]}});
 const outcomes=['configured','unchanged','absent','not-requested','unsupported','blocked','error'];
 for(const outcome of outcomes){
  h.custom.set('controller_list',request=>({controllers:{session:'controllers-1',offset:request.offset,total:sources.length,sources,backendStatus:'ready',selfAccess:{status:outcome,ready:['configured','unchanged'].includes(outcome),changed:outcome==='error'?null:outcome==='configured',message:'Startup evidence '+outcome}}}));
  await h.controller.refresh();
  const diagnostics=h.controller.snapshot.diagnostics.join(' '),wheel=h.component('controller:wheel'),pedals=h.component('controller:pedals');
  const attention=['unsupported','blocked','error'].includes(outcome);
  assert.equal(diagnostics.includes('Startup HidHide access'),attention,outcome);
  assert.equal(wheel.diagnostics.some(d=>d.startsWith('Startup HidHide access')),attention,outcome);
  assert.equal(wheel.presence,'connected');assert.equal(wheel.capture,'ready');assert.equal(wheel.stale,false);
  assert.equal(pedals.presence,'connected');assert.equal(pedals.hidden,true);assert.equal(pedals.forget.allowed,false);
  assert.equal(h.component('usb:serial').presence,'connected');
  if(outcome==='error')assert.match(diagnostics,/final state could not be verified/);
 }
 assert.ok(h.requests.every(request=>!request.command.startsWith('controller_visibility_')),'Inventory only reads the retained startup result.');
});

test('startup access diagnostics include later controller pages and are deduplicated', async () => {
 const sources=Array.from({length:65},(_,index)=>source('page-'+index)),h=harness({controllers:sources});
 h.custom.set('controller_list',request=>({controllers:{session:'controllers-1',offset:request.offset,total:sources.length,sources:sources.slice(request.offset,request.offset+32),backendStatus:'ready',selfAccess:request.offset?{status:'blocked',ready:false,changed:false,message:'Driver control is busy.'}:undefined}}));
 await h.controller.refresh();
 assert.equal(h.requests.filter(request=>request.command==='controller_list').length,3);
 assert.equal(h.controller.snapshot.components.filter(component=>component.type==='controller').length,65);
 assert.deepEqual(plain(h.controller.snapshot.diagnostics),['Startup HidHide access (blocked): Driver control is busy.']);
 assert.equal(h.component('controller:page-64').diagnostics.filter(d=>d.startsWith('Startup HidHide access')).length,1);
});

test('unverified startup access cannot prove an uncaptured controller offline while live capture and PnP remain authoritative', async () => {
 const initial=[source('live'),source('masked'),source('pnp-present',{online:false}),source('pnp-absent',{online:false})];
 const h=harness({controllers:initial,usb:[usb()],native:{virtualDevices:[],diagnostics:[],hiddenDevices:[
  {id:'present',name:'Hidden present device',instanceId:'HID\\PRESENT',controllerIds:['pnp-present'],present:true,hidden:true,diagnostics:[]},
  {id:'absent',name:'Confirmed absent device',instanceId:'HID\\ABSENT',controllerIds:['pnp-absent'],present:false,hidden:false,diagnostics:[]},
 ]}});
 await h.controller.refresh();const lastConnected=h.component('controller:masked').lastSeen;
 let reported=initial.map(s=>s.id==='masked'?{...s,online:false}:s),outcome='blocked';
 h.custom.set('controller_list',request=>({controllers:{session:'controllers-1',offset:request.offset,total:reported.length,sources:reported,backendStatus:'ready',selfAccess:{status:outcome,ready:false,changed:outcome==='error'?null:false,message:'Startup access was not verified.'}}}));
 for(outcome of ['blocked','error','unsupported']){
  await h.controller.refresh();const masked=h.component('controller:masked'),live=h.component('controller:live'),present=h.component('controller:pnp-present'),absent=h.component('controller:pnp-absent');
  assert.equal(masked.presence,'unknown',outcome);assert.equal(masked.capture,'unavailable');assert.equal(masked.stale,true);assert.equal(masked.forget.allowed,false);assert.equal(masked.lastSeen,lastConnected);
  assert.equal(live.presence,'connected');assert.equal(live.capture,'ready');assert.equal(live.stale,false);
  assert.equal(present.presence,'connected');assert.equal(present.hidden,true);assert.equal(present.stale,false);assert.equal(present.forget.allowed,false);
  assert.equal(absent.presence,'offline');assert.equal(absent.stale,false);
  assert.equal(h.component('usb:serial').presence,'connected');
 }
 await assert.rejects(h.controller.forget('controller:masked'),/cannot confirm.*offline/);
 assert.equal(h.requests.some(request=>request.command==='controller_forget'),false);
 reported=reported.filter(s=>s.id!=='masked');await h.controller.refresh();assert.equal(h.component('controller:masked').presence,'unknown','Incomplete capture cannot confirm absence when the record is omitted.');
 reported=[...reported,source('masked')];await h.controller.refresh();
 assert.equal(h.component('controller:masked').presence,'connected','Current capture overrides a historical startup warning.');assert.equal(h.component('controller:masked').capture,'ready');assert.equal(h.component('controller:masked').stale,false);
});

test('dual transport input ownership and configuration writer are independent observed facts', async () => {
 const h=harness({gateways:[gateway({state:{...gateway().state,modules:[module('panel',{directUsb:true})]}})],usb:[usb('serial',{...usb(),snapshot:{...usb().snapshot,claimedBy:'USB setup writer'}})],resources:[resource('proven',[connection('standalone-usb','serial'),connection('gateway:gateway-one','panel')])]});
 await h.controller.refresh();const component=h.component('resource:proven');assert.equal(component.inputOwner,'Device USB');assert.equal(component.configurationOwner,'USB setup writer');assert.equal(component.configurationRoute,'USB · COM7');assert.equal(component.connections.length,2);
});

test('newly verified USB CAN grouping supersedes remembered sightings and preserves their history', async () => {
 const h=harness({gateways:[gateway()],usb:[usb()]});await h.controller.refresh();assert.ok(h.component('usb:serial'));assert.ok(h.component('module:gateway-one/panel'));
 h.state.resources=[resource('proven',[connection('standalone-usb','serial'),connection('gateway:gateway-one','panel')])];
 await h.controller.refresh();assert.equal(h.controller.snapshot.components.filter(c=>c.type==='osg').length,1);assert.ok(h.component('resource:proven'));
});

test('forget review explains exact retained local bindings and degrades honestly on missing metadata', async () => {
 const h=harness({controllers:[source('offline',{online:false})]}); await h.controller.refresh();
 h.custom.set('profile_list',()=>({profiles:{profiles:[{id:'saved',name:'Driving'}]}}));
 h.custom.set('profile_assignments_get',()=>({profiles:{assignments:[{resourceId:'steering',sourceId:'offline',ports:[]}]}}));
 const review=await h.controller.reviewForget(['controller:offline']);assert.equal(review[0].references[0].name,'Driving / steering');
 assert.ok(h.component('controller:offline'));assert.equal(h.requests.some(r=>r.command==='controller_forget'),false);
 assert.equal(review[0].forget.allowed,false);assert.match(review[0].forget.reason,/Used in profiles/);
 await assert.rejects(h.controller.forget('controller:offline'),/Used in profiles/);
 h.failures.set('profile_list',Error('Metadata unavailable'));const partial=await h.controller.reviewForget(['controller:offline']);assert.equal(partial[0].forget.allowed,false);assert.match(partial[0].diagnostics.join(' '),/fully inspected/);
 await assert.rejects(h.controller.forget('controller:offline'),/Profile usage could not be checked/);
});

test('forget review resolves actual host facade UUID assignments and preserves source evidence through an offline provider refresh', async () => {
 const facade='10000000-0000-4000-8000-000000000001',unrelated='10000000-0000-4000-8000-000000000002';
 const h=harness({controllers:[source('offline',{online:false})],resources:[resource(facade,[connection('host-controllers','offline','hid')],{resourceType:'host-controller',availability:'offline'})]});
 h.custom.set('profile_list',()=>({profiles:{profiles:[{id:'saved',name:'Driving'}]}}));
 h.custom.set('profile_assignments_get',()=>({profiles:{assignments:[{resourceId:'steering',sourceId:facade,ports:[]},{resourceId:'unrelated',sourceId:unrelated,ports:[]}]}}));
 await h.controller.refresh();
 let review=await h.controller.reviewForget(['controller:offline']);
 assert.deepEqual(plain(review[0].references.map(reference=>reference.name)),['Driving / steering']);
 assert.ok(h.component('controller:offline').evidence.includes('binding-source:'+facade));
 h.state.resources=[];
 await h.controller.refresh();
 review=await h.controller.reviewForget(['controller:offline']);
 assert.deepEqual(plain(review[0].references.map(reference=>reference.name)),['Driving / steering']);
 assert.equal(h.requests.some(request=>/forget|revoke|apply|save/.test(request.command)),false);
});

test('gateway forget confirmation enumerates real facade UUID bindings affected by sole child-route cleanup', async () => {
 const facade='20000000-0000-4000-8000-000000000001';
 const h=harness({gateways:[gateway({present:false,state:{...gateway().state,connected:false,modules:[module('panel',{online:false})]}})],resources:[resource(facade,[connection('gateway:gateway-one','panel','can')],{availability:'offline'})]});
 h.custom.set('profile_list',()=>({profiles:{profiles:[{id:'saved',name:'Flight'}]}}));
 h.custom.set('profile_assignments_get',()=>({profiles:{assignments:[{resourceId:'radio',sourceId:facade,ports:[]}]}}));
 await h.controller.refresh();
 const review=await h.controller.reviewForget(['gateway:gateway-one']);
 assert.deepEqual(plain(review[0].references.map(reference=>reference.name)),['Flight / radio']);
 assert.ok(h.component('resource:'+facade));
 assert.equal(h.requests.some(request=>request.command==='forget'),false);
});

test('companion installation revoke confirmation includes assigned-instance bindings via native companions scope and facade UUID', async () => {
 const instance='30000000-0000-4000-8000-000000000001',facade='30000000-0000-4000-8000-000000000002';
 const h=harness({companions:{host:{clients:[{companionId:'tablet',name:'Tablet',connected:false}],instances:[{instanceId:instance,sourceId:instance,companionId:'tablet',name:'Assigned radio',connected:false,armed:false,sessionId:null}]}},resources:[resource(facade,[connection('companions',instance,'network')],{resourceType:'companion',availability:'offline'})]});
 h.custom.set('profile_list',()=>({profiles:{profiles:[{id:'saved',name:'Flight'}]}}));
 h.custom.set('profile_assignments_get',()=>({profiles:{assignments:[{resourceId:'radio-panel',sourceId:facade,ports:[]}]}}));
 await h.controller.refresh();
 assert.ok(h.component('companion-instance:'+instance).evidence.includes('binding-source:'+facade));
 const review=await h.controller.reviewForget(['companion:tablet']);
 assert.deepEqual(plain(review[0].references.map(reference=>reference.name)),['Flight / radio-panel']);
 assert.equal(h.requests.some(request=>request.command==='companion_revoke'),false);
});

test('companion transport is evidence-backed, pending requests remain visible, and USB presence prevents revocation', async () => {
 const h=harness({companions:{host:{clients:[{companionId:'serial',name:'USB-only panel',kind:'esp32',connected:false}],instances:[],pending:[{requestId:'request-1',companionId:'unverified',name:'Pending tablet',approved:false}]},usb:{connected:true,port:'COM8',device:{companionId:'serial'}}}});
 await h.controller.refresh();const serial=h.component('companion:serial');assert.deepEqual(plain(serial.connections.map(c=>c.transport)),['usb']);assert.equal(serial.forget.allowed,false);assert.equal(serial.presence,'connected');
 assert.equal(h.component('companion-pending:request-1').presence,'unknown');assert.equal(h.component('companion-pending:request-1').connections.length,0);
 assert.equal(h.requests.some(r=>/approve|revoke|pair/.test(r.command)),false);
});

test('offline companions keep observed attachments while configured integration endpoints stay explicitly unconfirmed', async () => {
 const h=harness({companions:{host:{clients:[{companionId:'tablet',name:'Tablet',kind:'browser',connected:true,transports:['network'],peer:'192.0.2.2'}],instances:[]}},games:{catalogue:[game('dcs',{integrationSetup:{state:'ready',live:'waiting',message:'Waiting',connection:{host:'127.0.0.1',port:7778}}})]}});
 await h.controller.refresh();assert.equal(h.component('integration:dcs').connections[0].observed,false);
 h.state.companions.host.clients[0].connected=false;delete h.state.companions.host.clients[0].peer;h.state.games.catalogue[0].integrationSetup.live='receiving';await h.controller.refresh();
 assert.equal(h.component('companion:tablet').connections[0].current,false);assert.equal(h.component('companion:tablet').connections[0].endpoint,'192.0.2.2');assert.equal(h.component('integration:dcs').connections[0].observed,true);
 h.state.games.catalogue[0].integrationSetup.live='waiting';await h.controller.refresh();assert.equal(h.component('integration:dcs').connections[0].observed,true);assert.equal(h.component('integration:dcs').connections[0].current,false);
 assert.equal(h.component('companion:tablet').connections[0].endpoint,'192.0.2.2','Configured browser transport must not erase last-known network attachment on later polls.');
 h.state.companions.host.clients[0].kind='esp32';h.state.companions.firmware={tablet:{connectionMode:'serial'}};await h.controller.refresh();
 const changed=h.component('companion:tablet').connections;
 assert.ok(changed.some(c=>c.transport==='network'&&c.endpoint==='192.0.2.2'&&!c.current&&c.observed!==false));
 assert.ok(changed.some(c=>c.transport==='usb'&&!c.current&&c.observed===false),'A changed offline setup is an additional unconfirmed route.');
 h.state.companions.host.clients[0].connected=true;h.state.companions.host.clients[0].transports=['usb'];await h.controller.refresh();
 assert.ok(h.component('companion:tablet').connections.every(c=>c.transport==='usb'&&c.current),'Fresh actual USB connection replaces old observed routes.');
});

test('gateway cleanup review includes child binding references without deleting or rewriting them', async () => {
 const g=gateway({present:false,state:{...gateway().state,connected:false,modules:[module('panel',{online:false})]}}),h=harness({gateways:[g]});await h.controller.refresh();
 h.custom.set('profile_list',()=>({profiles:{profiles:[{id:'saved',name:'Flight'}]}}));h.custom.set('profile_assignments_get',()=>({profiles:{assignments:[{resourceId:'can-switches',sourceId:'panel',ports:[]}]}}));
 const review=await h.controller.reviewForget(['gateway:gateway-one']);assert.equal(review[0].references[0].name,'Flight / can-switches');assert.ok(h.component('module:gateway-one/panel'));
 assert.equal(review[0].forget.allowed,false);await assert.rejects(h.controller.forget('gateway:gateway-one'),/Used in profiles/);
});

test('cleanup cancellation fences pending reference checks but retains a completed provider removal', async () => {
 const h=harness({controllers:[source('first',{online:false}),source('second',{online:false})],storage:storage()});
 await h.controller.refresh();
 const check=deferred(), started=deferred(), abort=new AbortController();
 h.custom.set('profile_list',()=>{started.resolve();return check.promise;});
 const cancelled=h.controller.forget('controller:first',abort.signal);
 await started.promise;abort.abort();check.resolve({profiles:{profiles:[]}});
 await assert.rejects(cancelled,/abort/i);assert.ok(h.component('controller:first'));
 assert.equal(h.requests.some(r=>r.command==='controller_forget'),false);
 h.custom.delete('profile_list');
 const removal=deferred(), removing=deferred(), stop=new AbortController();
 h.custom.set('controller_forget',()=>{removing.resolve();return removal.promise;});
 const committed=h.controller.forget('controller:first',stop.signal);
 await removing.promise;stop.abort();removal.resolve({});await committed;
 assert.equal(h.component('controller:first'),undefined);assert.ok(h.component('controller:second'));
 await h.controller.refresh();assert.equal(h.component('controller:first'),undefined);
});

test('malformed duplicate provider identities cannot prevent unrelated native inventory from publishing', async () => {
 const h=harness({controllers:[source('same'),source('same')],native:{virtualDevices:[{id:'vjoy:1',name:'Virtual',available:true,status:'free',createdByCockpit:null,diagnostics:[]}],hiddenDevices:[],diagnostics:[]}});
 await h.controller.refresh();assert.ok(h.component('virtual:vjoy:1'));assert.equal(h.component('controller:same'),undefined);assert.match(h.controller.snapshot.diagnostics.join(' '),/duplicate identities/);
});

test('context-only games stay out of integration inventory without hiding real providers',async()=>{
 const id='custom-11111111-1111-4111-8111-111111111111',h=harness({games:{catalogue:[game(),{id,name:'Custom game',provider:null,custom:true,installed:true,launchSupported:true}]}});
 await h.controller.refresh();assert.ok(h.component('integration:dcs'));assert.equal(h.component(`integration:${id}`),undefined);
});

test('failed gateway discovery cannot turn remembered gateways and modules into confirmed offline records', async () => {
 const h=harness({gateways:[gateway()]});await h.controller.refresh();h.state.gateways=[];h.state.gatewayError='Serial discovery unavailable';await h.controller.refresh();
 assert.equal(h.component('gateway:gateway-one').presence,'unknown');assert.equal(h.component('module:gateway-one/panel').forget.allowed,false);assert.match(h.controller.snapshot.diagnostics.join(' '),/Serial discovery unavailable/);
});

test('current inventory includes every category, current transport, diagnostics and unsupported integration without any profile requirement', async () => {
  const h = harness({ controllers: [source()], usb: [usb()], gateways: [gateway()], games: { catalogue: [game(), game('x-plane-12', { installed: false, provider: 'x-plane', integrationSetup: undefined })], error: 'Catalogue warning' }, companions: { host: { clients: [{ companionId: 'tablet', name: 'Tablet', kind: 'mobile', connected: true, transports: ['network'] }, { companionId: 'offline', name: 'Desktop', connected: false }, { name: 'Malformed without identity' }], instances: [{ instanceId: 'panel-1', name: 'Radio', companionId: 'tablet', connected: true, armed: true, sessionId: 'session-1' }, { instanceId: 'saved', name: 'Saved panel', companionId: 'offline', connected: false, armed: false, sessionId: null }, {}] }, usb: { devices: [{ connected: true, port: 'COM10', device: { companionId: 'tablet', name: 'Same tablet', boardProfile: 'ESP32' } }, { connected: false, port: 'COM11', stage: 'Disconnected', device: { name: 'Unpaired USB companion' } }, { connected: false }] }, listener: { lastError: 'Listener warning' } }, native: { virtualDevices: [{ id: 'vjoy-1', name: 'vJoy 1', available: true, status: 'Available', createdByCockpit: true, ownerPid: 123, diagnostics: ['Virtual diagnostic'] }, { id: 'vjoy-2', name: 'vJoy 2', available: false, status: 'Missing', createdByCockpit: null, diagnostics: [] }, { id: 'vjoy-3', name: 'vJoy 3', available: null, status: 'Unknown', createdByCockpit: false, owner: 'External app', diagnostics: [] }], hiddenDevices: [{ id: 'hidden', name: 'Hidden external device', instanceId: 'HID\\external', present: null, hidden: true, configured: true, inverse: true, diagnostics: ['External rule'] }], diagnostics: ['Native inspection warning'] } });
  const snapshot = await h.controller.refresh();
  assert.deepEqual([...new Set(snapshot.components.map(component => component.type))].sort(), ['companion', 'controller', 'gateway', 'host', 'integration', 'osg', 'virtual']);
  assert.equal(h.component('controller:wheel').capture, 'ready');
  assert.equal(h.component('module:gateway-one/panel').presence, 'connected');
  assert.equal(h.component('module:gateway-one/panel').connections[0].transport, 'can');
  assert.equal(h.component('usb:serial').connections[0].transport, 'usb');
  assert.equal(h.component('virtual:vjoy-1').owner, 'PID 123');
  assert.match(h.component('virtual:vjoy-1').status, /Created by Cockpit/);
  assert.equal(h.component('virtual:vjoy-2').forget.allowed, true);
  assert.equal(h.component('virtual:vjoy-3').presence, 'unknown');
  assert.equal(h.component('virtual:vjoy-3').owner, 'External app');
  assert.equal(h.component('integration:dcs').presence, 'connected');
  assert.match(h.component('integration:dcs').connections[0].endpoint, /127\.0\.0\.1.*7778/);
  assert.doesNotMatch(h.component('integration:dcs').connections[0].endpoint, /secret/);
  assert.equal(h.component('integration:x-plane-12').presence, 'unavailable');
  assert.equal(integrationGameId(h.component('integration:dcs')), 'dcs');
  assert.equal(integrationGameId(h.component('controller:wheel')), undefined);
  assert.deepEqual(plain(h.component('companion:tablet').connections.map(edge => edge.transport).sort()), ['network', 'usb']);
  assert.equal(h.component('companion-instance:panel-1').owner, 'session-1');
  assert.equal(h.component('companion-instance:saved').presence, 'offline');
  assert.match(h.component('visibility:hidden').diagnostics.join(' '), /inverse/);
  assert.match(snapshot.diagnostics.join(' '), /Catalogue warning.*Native inspection warning|Native inspection warning.*Catalogue warning/);
  assert.equal(h.requests.some(request => /claim|apply|activate|session|arm|profile_(?!resources|status)/.test(request.command)), false);
});

test('providers traverse every controller, USB and verified-identity resource page', async () => {
  const h = harness({ controllers: Array.from({ length: 67 }, (_, index) => source('wheel-' + index)), usb: Array.from({ length: 35 }, (_, index) => usb('usb-' + index)), resources: Array.from({ length: 65 }, (_, index) => resource('resource-' + index)) });
  await h.controller.refresh();
  for (const [command, expected] of [['controller_list', [0, 32, 64]], ['module_usb_list', [0, 32]], ['profile_resources_list', [0, 32, 64]]]) assert.deepEqual(h.requests.filter(request => request.command === command).map(request => request.offset), expected);
  assert.equal(h.controller.snapshot.components.filter(component => component.type === 'controller').length, 67);
  assert.equal(h.controller.snapshot.components.filter(component => component.type === 'osg').length, 35);
});

test('only native verified identity combines USB, CAN and host HID; ambiguous lookalikes retain independent nodes', async () => {
  const h = harness({ controllers: [source('panel-hid', { osgHid: true }), source('same-name', { name: 'CAN panel' })], usb: [usb(), usb('ambiguous', { port: { path: 'COM8', friendlyName: 'CAN panel' } })], gateways: [gateway()], resources: [resource('physical-panel', [connection('gateway:gateway-one', 'panel', 'can'), connection('standalone-usb', 'serial'), connection('host-controllers', 'panel-hid', 'hid')]), resource('unrelated', [connection('unsupported-scope', 'ambiguous')])] });
  await h.controller.refresh();
  const grouped = h.component('resource:physical-panel');
  assert.equal(grouped.type, 'osg');
  assert.equal(grouped.name, 'CAN panel');
  assert.equal(grouped.connections.length, 3);
  assert.ok(grouped.connections.every(edge => edge.to === grouped.id));
  assert.deepEqual(plain(grouped.targets.map(target => target.kind).sort()), ['controller', 'module', 'usb']);
  assert.equal(h.component('controller:panel-hid'), undefined);
  assert.ok(h.component('controller:same-name'));
  assert.ok(h.component('usb:ambiguous'));
  assert.equal(h.component('resource:unrelated'), undefined);
});

test('hidden PnP collections augment both ordinary and OSG host controllers without duplicate visibility nodes', async () => {
  for (const osgHid of [false, true]) {
    const h = harness({ controllers: [source('hidden-controller', { online: false, osgHid })], native: { virtualDevices: [], hiddenDevices: [{ id: 'pnp', name: 'Native same controller', instanceId: 'HID\\panel', present: true, hidden: true, controllerIds: ['hidden-controller'], diagnostics: ['Matched physical PnP collection'] }], diagnostics: [] } });
    await h.controller.refresh();
    assert.equal(h.component('visibility:pnp'), undefined, `supplemental hidden node must be consumed for osgHid=${osgHid}`);
    const controller = h.component('controller:hidden-controller');
    assert.equal(controller.hidden, true);
    assert.equal(controller.presence, 'connected');
    assert.equal(controller.forget.allowed, false);
    assert.match(controller.diagnostics.join(' '), /PnP/);
    assert.equal(h.controller.snapshot.components.length, 2);
  }
});

test('a connected but unarmed real companion session is not advertised as an active input session', async () => {
  const h = harness({ companions: { host: { clients: [{ companionId: 'display', name: 'Display', connected: true }], instances: [{ instanceId: 'radio', name: 'Radio', companionId: 'display', connected: true, armed: false, sessionId: 'live-session', generation: '1' }] } } });
  await h.controller.refresh();
  const instance = h.component('companion-instance:radio');
  assert.equal(instance.presence, 'connected');
  assert.notEqual(instance.capture, 'ready');
  assert.doesNotMatch(instance.status, /input session active/i);
});

test('gateway states keep presence, input ownership, availability and conservative forget capability independent', () => {
  const entries = [gateway({ error: 'Gateway error', state: { ...gateway().state, claimedModules: ['claimed'], reconnecting: true, modules: [module('claimed', { online: false, outputOnly: true, settings: { label: 'Output label' } }), module('direct', { directUsb: true, online: false, enabled: false, published: true }), module('offline', { online: false, boot: 42 })] } }), gateway({ id: 'unavailable', present: true, busy: true, port: { backend: 'pico' }, state: null }), gateway({ id: 'missing', present: false, port: { backend: 'pico' }, state: { connected: false, modules: [module('old', { online: false })] } })];
  const observations = gatewayObservations(entries, 20);
  const get = id => observations.find(component => component.id === id);
  assert.equal(get('gateway:gateway-one').status, 'Reconnecting');
  assert.deepEqual(plain(get('gateway:gateway-one').diagnostics), ['Gateway error']);
  assert.equal(get('module:gateway-one/claimed').owner, 'Setup session');
  assert.equal(get('module:gateway-one/claimed').capture, 'unavailable');
  assert.equal(get('module:gateway-one/claimed').name, 'Output label');
  assert.equal(get('module:gateway-one/direct').owner, 'Direct USB input owner');
  assert.equal(get('module:gateway-one/direct').enabled, false);
  assert.match(get('module:gateway-one/direct').status, /local attachment unconfirmed/);
  assert.match(get('module:gateway-one/offline').status, /Neutral controller retained/);
  assert.equal(get('gateway:unavailable').presence, 'unavailable');
  assert.equal(get('gateway:unavailable').status, 'Working');
  assert.equal(get('gateway:missing').forget.allowed, true);
  assert.equal(get('module:missing/old').presence, 'offline');
  assert.equal(get('module:missing/old').connections[0].transport, 'software');
});

test('failed providers retain stale unknown records while successful empty providers prove absence', async () => {
  const h = harness({ controllers: [source()], usb: [usb()], games: { catalogue: [game()] } });
  await h.controller.refresh();
  h.failures.set('controller_list', Error('Controller service disconnected'));
  h.failures.set('system_inventory', 'Native inspection unavailable');
  h.state.usb = [];
  await h.controller.refresh();
  assert.equal(h.component('controller:wheel').presence, 'unknown');
  assert.equal(h.component('controller:wheel').stale, true);
  assert.equal(h.component('controller:wheel').connections[0].current, false);
  assert.equal(h.component('controller:wheel').forget.allowed, false);
  assert.equal(h.component('usb:serial').presence, 'offline');
  assert.equal(h.component('usb:serial').stale, false);
  assert.equal(h.component('integration:dcs').presence, 'connected');
  assert.match(h.controller.snapshot.diagnostics.join(' '), /Controller service disconnected/);
  h.failures.clear();
  await h.controller.refresh();
  assert.equal(h.component('controller:wheel').presence, 'connected');
  assert.equal(h.component('controller:wheel').stale, false);
});

test('temporarily unavailable identity evidence preserves previously proved grouping without duplicate remembered devices', async () => {
  const h = harness({ usb: [usb()], gateways: [gateway()], resources: [resource('physical-panel', [connection('standalone-usb', 'serial'), connection('gateway:gateway-one', 'panel', 'can')])] });
  await h.controller.refresh();
  h.failures.set('profile_resources_list', Error('Identity provider restarting'));
  await h.controller.refresh();
  assert.equal(h.controller.snapshot.components.filter(component => component.type === 'osg').length, 1);
  assert.equal(h.component('resource:physical-panel').presence, 'connected');
  assert.match(h.controller.snapshot.diagnostics.join(' '), /Identity provider restarting/);
});

test('partial native inspection keeps omitted devices unknown while retaining affirmative current observations', async () => {
  const virtual = id => ({ id, name: id, available: true, status: 'Available', createdByCockpit: null, diagnostics: [] });
  const hidden = id => ({ id, name: id, instanceId: 'HID\\' + id, present: true, hidden: true, diagnostics: [] });
  const h = harness({ native: { virtualDevices: [virtual('current'), virtual('omitted')], hiddenDevices: [hidden('hidden')], diagnostics: [] } });
  await h.controller.refresh();
  h.state.native = { virtualDevices: [virtual('current')], hiddenDevices: [], diagnostics: ['HidHide inspection was incomplete; permission unavailable.'] };
  await h.controller.refresh();
  assert.equal(h.component('virtual:current').presence, 'connected');
  assert.equal(h.component('virtual:omitted').presence, 'unknown');
  assert.equal(h.component('virtual:omitted').stale, true);
  assert.equal(h.component('virtual:omitted').forget.allowed, false);
  assert.equal(h.component('visibility:hidden').presence, 'unknown');
  assert.equal(h.component('visibility:hidden').connections.every(connection => !connection.current), true);
  assert.match(h.controller.snapshot.diagnostics.join(' '), /incomplete/);
  h.state.native = { virtualDevices: [virtual('current')], hiddenDevices: [], diagnostics: [] };
  await h.controller.refresh();
  assert.equal(h.component('virtual:omitted').presence, 'offline');
  assert.equal(h.component('visibility:hidden').presence, 'offline');
});

test('malformed, changed-session and truncated pages fail one provider without discarding other categories', async () => {
  const badPages = [
    request => ({ controllers: { session: 'a', offset: request.offset + 1, total: 1, sources: [source()] } }),
    request => ({ controllers: { session: 'a', offset: request.offset, total: 1, sources: [] } }),
    request => ({ controllers: { session: 'a', offset: request.offset, total: 5000, sources: [source()] } }),
    request => ({ controllers: { session: request.offset ? 'changed' : 'first', offset: request.offset, total: 2, sources: [source('at-' + request.offset)] } }),
    request => ({ controllers: { session: 'a', offset: request.offset, total: 1, sources: [source('first'), source('overflow')] } }),
  ];
  for (const bad of badPages) {
    const h = harness({ usb: [usb()] }); h.custom.set('controller_list', bad);
    await h.controller.refresh();
    assert.equal(h.controller.snapshot.components.filter(component => component.type === 'controller').length, 0);
    assert.ok(h.component('usb:serial'));
    assert.match(h.controller.snapshot.diagnostics.join(' '), /Controller inventory/);
  }
  const h = harness();
  h.custom.set('module_usb_list', request => ({ moduleUsb: { offset: request.offset, total: 2, modules: request.offset ? [] : [usb()] } }));
  await h.controller.refresh();
  assert.match(h.controller.snapshot.diagnostics.join(' '), /USB inventory/);
});

test('missing capabilities are surfaced as diagnostics instead of silently treating providers as empty', async () => {
  const h = harness();
  for (const command of ['controller_list', 'module_usb_list', 'companion_status', 'game_get_state', 'system_inventory', 'profile_resources_list']) h.custom.set(command, () => ({}));
  await h.controller.refresh();
  assert.equal(h.controller.snapshot.components.length, 1);
  assert.equal(h.controller.snapshot.diagnostics.length, 6);
  assert.match(h.controller.snapshot.diagnostics.join(' '), /Identity correlation/);
});

test('refresh coalesces concurrent reads, publishes once, and ignores late provider responses after disposal', async () => {
  const h = harness({ controllers: [source()] }), gate = deferred();
  h.custom.set('controller_list', () => gate.promise);
  const first = h.controller.refresh(), second = h.controller.refresh();
  assert.equal(first, second);
  h.controller.dispose();
  gate.resolve({ controllers: { session: 'one', offset: 0, total: 1, sources: [source()] } });
  await first;
  assert.equal(h.changes.length, 0);
  assert.equal(h.controller.snapshot.components.length, 0);
  const reads = h.requests.length;
  await h.controller.refresh();
  assert.equal(h.requests.length, reads);
});

test('late identity-facade response cannot publish a disposed inventory', async () => {
  const h = harness({ controllers: [source()] }), gate = deferred(), entered = deferred();
  h.custom.set('profile_resources_list', () => { entered.resolve(); return gate.promise; });
  const pending = h.controller.refresh();
  await entered.promise;
  h.controller.dispose();
  gate.resolve({ profileResources: { session: 's', revision: '1', offset: 0, total: 0, resources: [] } });
  await pending;
  assert.equal(h.changes.length, 0);
});

test('restart restores remembered components conservatively and a fresh observation restores current state', async () => {
  const disk = storage(), first = harness({ storage: disk, controllers: [source()] });
  await first.controller.refresh();
  const firstSeen = first.component('controller:wheel').firstSeen;
  first.controller.dispose();
  const second = harness({ storage: disk, controllers: [source()] });
  assert.equal(second.component('controller:wheel').presence, 'unknown');
  assert.equal(second.component('controller:wheel').forget.allowed, false);
  await second.controller.refresh();
  assert.equal(second.component('controller:wheel').presence, 'connected');
  assert.equal(second.component('controller:wheel').firstSeen, firstSeen);
});

test('forget refreshes presence immediately and refuses a controller that reconnected since the displayed snapshot', async () => {
  const h = harness({ controllers: [source('wheel', { online: false })] });
  await h.controller.refresh();
  assert.equal(h.component('controller:wheel').forget.allowed, true);
  h.state.controllers = [source()];
  await assert.rejects(h.controller.forget('controller:wheel'), /connected|Disconnect/i);
  assert.equal(h.requests.some(request => request.command === 'controller_forget'), false);
});

test('eligible forget dispatches only its provider action and durable tombstones suppress stale rediscovery', async () => {
  for (const [state, id, command] of [
    [{ controllers: [source('wheel', { online: false })] }, 'controller:wheel', 'controller_forget'],
    [{ companions: { host: { clients: [{ companionId: 'tablet', name: 'Tablet', connected: false }], instances: [] } } }, 'companion:tablet', 'companion_revoke'],
    [{ gateways: [gateway({ state: { ...gateway().state, modules: [module('panel', { online: false })] } })] }, 'module:gateway-one/panel', 'forget'],
  ]) {
    const h = harness({ ...state, storage: storage() });
    await h.controller.refresh();
    await h.controller.forget(id);
    assert.equal(h.component(id), undefined);
    assert.equal(h.requests.filter(request => request.command === command).length, 1);
    await h.controller.refresh();
    assert.equal(h.component(id), undefined, 'unchanged offline provider record must remain forgotten');
  }
});

test('local-only forget leaves unavailable providers untouched and failed native forget retains the visible record', async () => {
  const h = harness({ usb: [usb('old-usb', { present: false, snapshot: { connected: false, identity: null, claimedBy: null, error: 'Removed' } })] });
  await h.controller.refresh();
  await h.controller.forget('usb:old-usb');
  assert.equal(h.component('usb:old-usb'), undefined);
  assert.equal(h.requests.some(request => /forget|revoke/.test(request.command)), false);
  const failure = harness({ controllers: [source('wheel', { online: false })] });
  failure.failures.set('controller_forget', Error('Native removal refused'));
  await assert.rejects(failure.controller.forget('controller:wheel'), /Native removal refused/);
  assert.ok(failure.component('controller:wheel'));
});

test('identity grouping combines independent hidden, ownership and readiness evidence without weakening removal safety', () => {
  const sample = gatewayObservations([gateway({ state: { ...gateway().state, modules: [module('panel', { online: false })] } })], 1).find(component => component.type === 'osg');
  const grouped = correlateSystem([sample, { ...sample, id: 'usb:serial', targets: [{ kind: 'usb', id: 'serial' }], presence: 'connected', owner: 'External writer', hidden: true, capture: 'ready', evidence: ['USB proof'], diagnostics: ['USB diagnostic'] }], [resource('same', [connection('gateway:gateway-one', 'panel'), connection('standalone-usb', 'serial')])]);
  assert.equal(grouped.length, 1);
  assert.equal(grouped[0].hidden, true);
  assert.equal(grouped[0].owner, 'External writer');
  assert.equal(grouped[0].capture, 'ready');
  assert.equal(grouped[0].forget.allowed, false);
});

test('integration visibility follows immutable applied bindings even while telemetry waits', async()=>{
 const {filterSystem}=modules.get('./system-inventory');
 const id='00000000-0000-4000-8000-000000000001';
 const owner={session:id,generation:1,profileId:id,profileRevision:1,gameId:'assetto-corsa-rally'};
 const snapshot={profileId:id,revision:1,semanticHash:'sha256:'+'a'.repeat(64)};
 const h=harness({runtime:{state:'running',owner,appliedSnapshot:snapshot},games:{catalogue:[game('assetto-corsa-rally',{integrationSetup:{state:'ready',live:'waiting'}}),game('dcs')]},companions:{host:{clients:[{companionId:'tablet',name:'Tablet',connected:true}],instances:[{instanceId:'radio',name:'Radio',companionId:'tablet'}]}}});
 let edges=[{id:'telemetry',from:{nodeId:'acr',portId:'speed'},to:{nodeId:'radio',portId:'text'}}];
 h.custom.set('profile_runtime_topology',request=>({profileTopology:{schemaVersion:1,owner,appliedSnapshot:snapshot,programHash:'sha256:'+'b'.repeat(64),capturedUs:'1',offset:0,total:request.section==='nodes'?2:edges.length,section:request.section,items:request.section==='nodes'?[{id:'acr',name:'ACR',kind:'game',summary:'Integration'},{id:'radio',name:'Radio',kind:'companion',summary:'Panel'}]:edges}}));
 await h.controller.refresh();
 let visible=filterSystem(h.controller.snapshot.components,'','all');
 assert.ok(visible.some(c=>c.id==='integration:assetto-corsa-rally'));
 assert.ok(!visible.some(c=>c.id==='integration:dcs'||c.id.startsWith('companion-instance:')));
 assert.match(h.component('companion:tablet').status,/Panel: Radio/);
 edges=[];await h.controller.refresh();assert.ok(!filterSystem(h.controller.snapshot.components,'','all').some(c=>c.type==='integration'));
 edges=[{id:'telemetry',from:{nodeId:'acr',portId:'speed'},to:{nodeId:'radio',portId:'text'}}];h.state.runtime={state:'stopped',owner:null,appliedSnapshot:null};await h.controller.refresh();assert.ok(!filterSystem(h.controller.snapshot.components,'','all').some(c=>c.type==='integration'));
});

test('offline companions expose configured transports on first load and retain them across polls',async()=>{
 const disk=storage();const h=harness({storage:disk,companions:{firmware:{wifi:{version:'1',connectionMode:'wifi'},direct:{version:'1',connectionMode:'serial'}},host:{clients:[{companionId:'wifi',kind:'esp32'},{companionId:'direct',kind:'esp32'},{companionId:'browser',kind:'browser'},{companionId:'legacy',kind:'esp32'}],instances:[]}}});
 await h.controller.refresh();
 for(const [id,transport] of [['wifi','network'],['direct','usb'],['browser','network']]){
 const c=h.component('companion:'+id);assert.equal(c.presence,'offline');assert.equal(c.connections[0].transport,transport);assert.equal(c.connections[0].current,false);assert.equal(c.connections[0].observed,false);
 }
 assert.equal(h.component('companion:legacy').connections.length,0);
 h.state.companions.firmware={};await h.controller.refresh();assert.equal(h.component('companion:wifi').connections[0].transport,'network');
 const reloaded=harness({storage:disk,companions:h.state.companions});await reloaded.controller.refresh();assert.equal(reloaded.component('companion:direct').connections[0].transport,'usb');
 h.state.companions.usb={connected:true,port:'COM7',device:{companionId:'wifi',connectionMode:'serial'}};await h.controller.refresh();assert.ok(h.component('companion:wifi').connections.some(c=>c.transport==='usb'&&c.current));
});

test('startup native correlation waits for controller and USB captures before publishing a single count',async()=>{
  const h=harness({controllers:[source('wheel')]}),controllerGate=deferred(),usbGate=deferred();
  h.custom.set('controller_list',()=>controllerGate.promise);h.custom.set('module_usb_list',()=>usbGate.promise);
  h.custom.set('system_inventory',()=>({systemInventory:{virtualDevices:[],diagnostics:[],hiddenDevices:[{id:'pnp-wheel',name:'Wheel collection',instanceId:'HID/wheel',present:true,hidden:false,controllerIds:['wheel'],diagnostics:[]}]}}));
  const pending=h.controller.refresh();await Promise.resolve();assert.equal(h.requests.some(r=>r.command==='system_inventory'),false);
  controllerGate.resolve({controllers:{session:'ready',offset:0,total:1,sources:[source('wheel')]}});await new Promise(resolve=>setImmediate(resolve));assert.equal(h.requests.some(r=>r.command==='system_inventory'),false);
  usbGate.resolve({moduleUsb:{offset:0,total:0,modules:[]}});await pending;
  assert.equal(h.changes.length,1);assert.equal(h.controller.snapshot.components.filter(c=>c.type!=='host').length,1);assert.ok(h.component('controller:wheel'));assert.equal(h.component('visibility:pnp-wheel'),undefined);
});
