import test from 'node:test';
import assert from 'node:assert/strict';
import { INVENTORY_STORAGE_KEY, SystemInventoryRegistry, filterSystem, systemStateLabel } from './system-inventory.ts';
const observation = (id='wheel', extra={}) => ({id,name:id,type:'controller',role:'wheel',presence:'connected',hidden:null,enabled:null,owner:null,capture:'ready',status:'',diagnostics:[],connections:[{id:'usb:'+id,from:'host',to:id,transport:'usb',current:true}],targets:[],evidence:['local:'+id],references:[],observedAt:1,provider:'controllers',stale:false,forget:{allowed:false,reason:'Connected',effect:'Local record only'},...extra});
function storage() { const values=new Map();return {getItem:k=>values.get(k)??null,setItem:(k,v)=>values.set(k,v)}; }
test('one shared inventory preserves independent state, filters and durable first/last seen',()=>{
 const disk=storage(),r=new SystemInventoryRegistry(disk);
 r.update([observation('wheel',{hidden:true,enabled:false})],['controllers'],[],10);
 assert.match(systemStateLabel(r.snapshot.components[0]),/Connected · Hidden via HidHide · Disabled/);
 assert.equal(filterSystem(r.snapshot.components,'wheel','hidden').length,1);
 assert.equal(filterSystem(r.snapshot.components,'','offline').length,0);
 r.update([observation()],['controllers'],[],20);
 assert.equal(r.snapshot.components[0].firstSeen,10);assert.equal(r.snapshot.components[0].lastSeen,20);
 const reload=new SystemInventoryRegistry(disk);assert.equal(reload.snapshot.components[0].presence,'unknown');assert.equal(reload.snapshot.components[0].connections[0].current,false);
});
test('provider failure retains unknown records; complete absence proves offline; forgetting never deletes references',()=>{
 const r=new SystemInventoryRegistry(storage());r.update([observation('wheel',{references:[{kind:'binding',id:'b',name:'Steering'}]})],['controllers']);
 r.update([],[],['Controller failure']);assert.equal(r.snapshot.components[0].presence,'unknown');assert.equal(r.snapshot.components[0].forget.allowed,false);
 assert.throws(()=>r.forget('wheel'));
 r.update([],['controllers']);assert.equal(r.snapshot.components[0].presence,'offline');assert.equal(r.snapshot.components[0].references[0].id,'b');
 r.forget('wheel');assert.equal(r.snapshot.components.length,0);
});
test('durable tombstones suppress stale records and allow genuinely reconnected devices',()=>{
 const disk=storage(),r=new SystemInventoryRegistry(disk),offline=observation('wheel',{presence:'offline',connections:[],forget:{allowed:true,reason:'',effect:'local'}});
 r.update([offline],['controllers']);r.forget('wheel');
 const reload=new SystemInventoryRegistry(disk);reload.update([{...offline,observedAt:900}],['controllers']);assert.equal(reload.snapshot.components.length,0);
 reload.update([observation()],['controllers']);assert.equal(reload.snapshot.components.length,1);
 assert.throws(()=>reload.forget('wheel'));
});
test('duplicates are rejected; all known types remain searchable without connection evidence',()=>{
 const r=new SystemInventoryRegistry();assert.throws(()=>r.update([observation(),observation()],['controllers']),/duplicate/);
 const components=r.update(['integration','virtual','gateway','osg','companion'].map(type=>observation(type,{type,connections:[],presence:'unknown'})),[]).components;
 assert.equal(filterSystem(components,'','all').length,4);assert.equal(filterSystem(components,'','virtual').length,1);assert.equal(filterSystem(components,'missing','all').length,0);
});

test('transport filters include current and remembered routes and unknown unconnected nodes',()=>{
 const r=new SystemInventoryRegistry();const components=r.update([observation('wheel'),observation('network',{connections:[{from:'host',to:'network',id:'n',transport:'network',current:false}]}),observation('unknown',{connections:[]})],[]).components;
 assert.equal(filterSystem(components,'','all','usb').length,1);assert.equal(filterSystem(components,'','all','network')[0].id,'network');assert.equal(filterSystem(components,'','all','unknown')[0].id,'unknown');
});

test('unavailable providers and restart clear current input/configuration ownership rather than repeating old claims',()=>{
 const disk=storage(),r=new SystemInventoryRegistry(disk);
 r.update([observation('wheel',{owner:'Setup',inputOwner:'USB',configurationOwner:'Setup',configurationRoute:'USB · COM7'})],['controllers']);
 const reload=new SystemInventoryRegistry(disk);assert.equal(reload.snapshot.components[0].configurationOwner,null);assert.equal(reload.snapshot.components[0].inputOwner,null);
 r.update([],[]);assert.equal(r.snapshot.components[0].configurationRoute,null);assert.equal(r.snapshot.components[0].configurationOwner,null);
});
test('storage errors are explicit and failed durable removal is not reported successful',()=>{
 const disk={getItem:()=>'{',setItem:()=>{throw Error('full');}},r=new SystemInventoryRegistry(disk);
 assert.match(r.snapshot.diagnostics[0],/loaded/);r.update([observation('wheel',{presence:'offline',forget:{allowed:true,reason:'',effect:'local'}})],['controllers']);
 assert.match(r.snapshot.diagnostics[0],/saved/);assert.throws(()=>r.forget('wheel'),/saved/);
});

const offline = (id, extra={}) => observation(id,{presence:'offline',connections:[],forget:{allowed:true,reason:'',effect:'Local cleanup'},...extra});
const staleGateway = () => offline('gateway:old',{type:'gateway',provider:'gateways',targets:[{kind:'gateway',id:'old'}],connections:[{id:'gateway-usb',from:'host',to:'gateway:old',transport:'usb',current:false}]});
const gatewayChild = (extra={}) => offline('resource:panel',{type:'osg',provider:'gateways',targets:[{kind:'module',id:'panel',gatewayId:'old'}],evidence:['route:old/panel','physical-identity'],references:[{kind:'binding',id:'steering',name:'Steering'}],connections:[{id:'panel-can',from:'gateway:old',to:'resource:panel',transport:'can',current:false}],...extra});

test('forgetting a stale gateway removes its sole-route children and suppresses unchanged provider rediscovery across restart',()=>{
 const disk=storage(),registry=new SystemInventoryRegistry(disk),observations=[staleGateway(),gatewayChild()];
 registry.update(observations,['gateways'],[],10);registry.forget('gateway:old');
 assert.equal(registry.snapshot.components.length,0);
 const saved=JSON.parse(disk.getItem(INVENTORY_STORAGE_KEY));
 assert.deepEqual(Object.keys(saved.forgotten).sort(),['gateway:old','resource:panel']);
 const restarted=new SystemInventoryRegistry(disk);
 restarted.update(observations.map(component=>({...component,observedAt:999,status:'Polling counter changed'})),['gateways'],[],20);
 assert.equal(restarted.snapshot.components.length,0);
 restarted.update([{...staleGateway(),presence:'connected',connections:[{id:'gateway-usb',from:'host',to:'gateway:old',transport:'usb',current:true}]},{...gatewayChild(),presence:'connected',connections:[{id:'panel-can',from:'gateway:old',to:'resource:panel',transport:'can',current:true}]}],['gateways'],[],30);
 assert.equal(restarted.snapshot.components.length,2);
 assert.equal(restarted.snapshot.components.every(component=>component.presence==='connected'),true);
});

test('gateway cleanup retains a physical component with independently observed USB, routes, targets and binding references',()=>{
 const registry=new SystemInventoryRegistry(storage());
 const child=gatewayChild({presence:'connected',owner:'USB input session',targets:[{kind:'module',id:'panel',gatewayId:'old'},{kind:'usb',id:'serial'},{kind:'controller',id:'hid-interface'}],connections:[...gatewayChild().connections,{id:'panel-usb',from:'host',to:'resource:panel',transport:'usb',current:true}],evidence:['route:old/panel','physical-identity','usb-serial']});
 const before=JSON.stringify(child);
 registry.update([staleGateway(),child],['gateways','usb','controllers']);registry.forget('gateway:old');
 const retained=registry.snapshot.components[0];
 assert.equal(retained.id,'resource:panel');assert.equal(retained.presence,'connected');assert.equal(retained.owner,'USB input session');
 assert.equal(retained.provider,'usb');
 assert.deepEqual(retained.connections.map(connection=>connection.id),['panel-usb']);
 assert.deepEqual(retained.targets.map(target=>target.kind),['usb','controller']);
 assert.deepEqual(retained.references,[{kind:'binding',id:'steering',name:'Steering'}]);
 assert.deepEqual(retained.evidence,['physical-identity','usb-serial']);
 assert.equal(JSON.stringify(child),before,'cleanup must not mutate provider snapshots');
 registry.update([staleGateway(),child],['gateways','usb','controllers']);
 assert.equal(registry.snapshot.components.length,1);
 assert.equal(registry.snapshot.components[0].connections.some(connection=>connection.from==='gateway:old'),false);
 registry.update([],['gateways']);
 assert.equal(registry.snapshot.components[0].presence,'unknown','remaining USB provider failure must not be mistaken for gateway-confirmed absence');
});

test('gateway cleanup retains another gateway route and recursively removes only orphaned downstream nodes',()=>{
 const registry=new SystemInventoryRegistry();
 const other=offline('gateway:other',{type:'gateway',provider:'gateways',targets:[{kind:'gateway',id:'other'}]});
 const child=gatewayChild({targets:[...gatewayChild().targets,{kind:'module',id:'panel-other',gatewayId:'other'}],connections:[...gatewayChild().connections,{id:'other-can',from:'gateway:other',to:'resource:panel',transport:'can',current:false}]});
 const sole=gatewayChild({id:'module:old/sole',targets:[{kind:'module',id:'sole',gatewayId:'old'}],connections:[{id:'sole-can',from:'gateway:old',to:'module:old/sole',transport:'can',current:false}]});
 const downstream=offline('software:dependent',{type:'integration',connections:[{id:'dependent',from:sole.id,to:'software:dependent',transport:'software',current:false}]});
 registry.update([staleGateway(),other,child,downstream,sole],['gateways']);registry.forget('gateway:old');
 assert.deepEqual(registry.snapshot.components.map(component=>component.id).sort(),['gateway:other','resource:panel']);
 const panel=registry.snapshot.components.find(component=>component.id==='resource:panel');
 assert.deepEqual(panel.targets,[{kind:'module',id:'panel-other',gatewayId:'other'}]);
 assert.deepEqual(panel.connections.map(connection=>connection.from),['gateway:other']);
 assert.equal(registry.snapshot.components.some(component=>component.connections.some(connection=>['gateway:old','module:old/sole'].includes(connection.from))),false);
});

test('an owned sole-route child blocks parent cleanup before persistence changes',()=>{
 const disk=storage(),registry=new SystemInventoryRegistry(disk);
 registry.update([staleGateway(),gatewayChild({owner:'Setup session'})],['gateways']);
 const before=disk.getItem(INVENTORY_STORAGE_KEY),snapshot=registry.snapshot;
 assert.throws(()=>registry.forget('gateway:old'),/owned child/);
 assert.equal(registry.snapshot,snapshot);assert.equal(disk.getItem(INVENTORY_STORAGE_KEY),before);
});

test('failed durable cleanup leaves visible records, hidden memory and all child routes intact, then permits a clean retry',()=>{
 const backing=storage();let fail=false;
 const disk={getItem:key=>backing.getItem(key),setItem:(key,value)=>{if(fail)throw Error('quota');backing.setItem(key,value);}};
 const registry=new SystemInventoryRegistry(disk),observations=[staleGateway(),gatewayChild()];
 registry.update(observations,['gateways'],[],10);
 const before=backing.getItem(INVENTORY_STORAGE_KEY),snapshot=registry.snapshot;
 fail=true;assert.throws(()=>registry.forget('gateway:old'),/saved/);
 assert.equal(registry.snapshot,snapshot);assert.equal(backing.getItem(INVENTORY_STORAGE_KEY),before);
 registry.update(observations,['gateways'],[],20);
 assert.equal(registry.snapshot.components.length,2,'failed cleanup must not leave in-memory tombstones');
 assert.equal(registry.snapshot.components.find(component=>component.type==='osg').connections[0].from,'gateway:old');
 fail=false;registry.forget('gateway:old');assert.equal(registry.snapshot.components.length,0);
 assert.equal(registry.snapshot.diagnostics.some(diagnostic=>diagnostic.includes('could not be saved')),false);
});

test('duplicate refreshes do not partially clear durable tombstones, and never-connected records have no fabricated connected timestamp',()=>{
 const registry=new SystemInventoryRegistry(storage());
 registry.update([offline('never')],['controllers'],[],10);
 assert.equal(registry.snapshot.components[0].firstSeen,10);assert.equal(registry.snapshot.components[0].lastSeen,0);
 registry.forget('never');
 assert.throws(()=>registry.update([observation('never'),observation('never')],['controllers']),/duplicate/);
 registry.update([offline('never')],['controllers']);assert.equal(registry.snapshot.components.length,0);
 registry.update([observation('never')],['controllers'],[],30);
 assert.equal(registry.snapshot.components[0].lastSeen,30);
 registry.update([offline('never')],['controllers'],[],40);assert.equal(registry.snapshot.components[0].lastSeen,30);
});

test('legacy tombstones remain readable and malformed persisted tombstones cannot break future refreshes',()=>{
 for(const forgotten of [[],{bad:null},{bad:{signature:1,sawAbsent:false}},{bad:{signature:'old',sawAbsent:false,type:'bad'}}]){
  const disk=storage();disk.setItem(INVENTORY_STORAGE_KEY,JSON.stringify({version:1,components:[],forgotten}));
  const registry=new SystemInventoryRegistry(disk);registry.update([observation()],['controllers']);assert.equal(registry.snapshot.components.length,1);
 }
 const disk=storage(),registry=new SystemInventoryRegistry(disk),item=offline('__proto__');
 registry.update([item],['controllers']);registry.forget('__proto__');
 const restarted=new SystemInventoryRegistry(disk);restarted.update([item],['controllers']);assert.equal(restarted.snapshot.components.length,0);
});

test('bound integrations appear without telemetry and panels stay on companions',()=>{
 const items=[observation('integration:acr',{type:'integration',presence:'unknown',evidence:['active-bindings']}),observation('integration:dcs',{type:'integration'}),observation('companion-instance:panel',{type:'companion'}),observation('companion:tablet',{type:'companion',status:'Panel: Radio'})];
 assert.deepEqual(filterSystem(items,'','all').map(c=>c.id),['integration:acr','companion:tablet']);
 assert.deepEqual(filterSystem(items,'Radio','all').map(c=>c.id),['companion:tablet']);
 assert.equal(filterSystem([{...items[0],stale:true}],'','all').length,0);
});

test('device inventory excludes unpaired setup records and drops revoked installations',()=>{
 const disk=storage(),registry=new SystemInventoryRegistry(disk);
 const paired=observation('companion:tablet',{type:'companion',provider:'companions',presence:'offline'});
 const setup=observation('companion-usb:COM7',{type:'companion',provider:'companions'});
 const pending=observation('companion-pending:request',{type:'companion',provider:'companions'});
 registry.update([paired,setup,pending],['companions']);
 assert.deepEqual(filterSystem(registry.snapshot.components,'','all').map(c=>c.id),['companion:tablet']);
 const reloaded=new SystemInventoryRegistry(disk);assert.deepEqual(filterSystem(reloaded.snapshot.components,'','all').map(c=>c.id),['companion:tablet']);
 reloaded.update([],[],['Companions unavailable']);assert.equal(filterSystem(reloaded.snapshot.components,'','all').length,1);
 reloaded.update([],['companions']);assert.equal(filterSystem(reloaded.snapshot.components,'','all').length,0);
});
