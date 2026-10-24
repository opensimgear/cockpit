import test from 'node:test';
import assert from 'node:assert/strict';
import { INVENTORY_STORAGE_KEY, SystemInventoryRegistry, filterSystem, systemStateLabel } from './system-inventory.ts';
const observation = (id='wheel', extra={}) => ({id,name:id,type:'controller',role:'wheel',presence:'connected',hidden:null,enabled:null,owner:null,capture:'ready',status:'',diagnostics:[],connections:[{id:'usb:'+id,from:'host',to:id,transport:'usb',current:true}],targets:[],evidence:['local:'+id],references:[],observedAt:1,provider:'controllers',stale:false,forget:{allowed:false,reason:'Connected',effect:'Local record only'},...extra});
function storage() { const values=new Map();return {getItem:k=>values.get(k)??null,setItem:(k,v)=>values.set(k,v)}; }
test('one shared inventory preserves independent state, filters and durable first/last seen',async()=>{
 const disk=storage(),r=new SystemInventoryRegistry(disk);
 await r.update([observation('wheel',{hidden:true,enabled:false})],['controllers'],[],10);
 assert.match(systemStateLabel(r.snapshot.components[0]),/Connected · Hidden via HidHide · Disabled/);
 assert.equal(filterSystem(r.snapshot.components,'wheel','hidden').length,1);
 assert.equal(filterSystem(r.snapshot.components,'','offline').length,0);
 await r.update([observation()],['controllers'],[],20);
 assert.equal(r.snapshot.components[0].firstSeen,10);assert.equal(r.snapshot.components[0].lastSeen,20);
 const reload=new SystemInventoryRegistry(disk);assert.equal(reload.snapshot.components[0].presence,'unknown');assert.equal(reload.snapshot.components[0].connections[0].current,false);
});
test('provider failure retains unknown records; complete absence proves offline; forgetting never deletes references',async()=>{
 const r=new SystemInventoryRegistry(storage());await r.update([observation('wheel',{references:[{kind:'binding',id:'b',name:'Steering'}]})],['controllers']);
 await r.update([],[],['Controller failure']);assert.equal(r.snapshot.components[0].presence,'unknown');assert.equal(r.snapshot.components[0].forget.allowed,false);
 await assert.rejects(async()=>await r.forget('wheel'));
 await r.update([],['controllers']);assert.equal(r.snapshot.components[0].presence,'offline');assert.equal(r.snapshot.components[0].references[0].id,'b');
 await r.forget('wheel');assert.equal(r.snapshot.components.length,0);
});
test('durable tombstones suppress stale records and allow genuinely reconnected devices',async()=>{
 const disk=storage(),r=new SystemInventoryRegistry(disk),offline=observation('wheel',{presence:'offline',connections:[],forget:{allowed:true,reason:'',effect:'local'}});
 await r.update([offline],['controllers']);await r.forget('wheel');
 const reload=new SystemInventoryRegistry(disk);await reload.update([{...offline,observedAt:900}],['controllers']);assert.equal(reload.snapshot.components.length,0);
 await reload.update([observation()],['controllers']);assert.equal(reload.snapshot.components.length,1);
 await assert.rejects(async()=>await reload.forget('wheel'));
});

test('cached HidHide flags require fresh policy evidence and clear when rules disappear',async()=>{
 const r=new SystemInventoryRegistry(storage());
 const hidden=observation('old-hid',{provider:'native',hidden:true,targets:[{kind:'visibility',id:'HID/OLD'}],status:'Configured visibility rule'});
 await r.update([hidden],['native','visibility']);
 await r.update([],['native']);
 assert.equal(r.snapshot.components[0].hidden,null);
 assert.equal(r.snapshot.components[0].forget.allowed,false);
 assert.match(r.snapshot.components[0].forget.reason,/Refresh HidHide/);
 await assert.rejects(async()=>await r.forget('old-hid'));
 // Unrelated native failures do not invalidate a separately verified policy read.
 await r.update([],['visibility']);
 assert.equal(r.snapshot.components[0].hidden,false);
 assert.equal(r.snapshot.components[0].presence,'offline');
 assert.equal(r.snapshot.components[0].forget.allowed,true);
 assert.equal(r.snapshot.components[0].status,'No configured HidHide rule');
 await r.forget('old-hid');assert.equal(r.snapshot.components.length,0);
});

test('a controller response alone cannot clear a previously observed HidHide rule',async()=>{
 const r=new SystemInventoryRegistry(storage());
 await r.update([observation('wheel',{hidden:true,targets:[{kind:'visibility',id:'HID/WHEEL'}]})],['controllers','visibility']);
 await r.update([observation('wheel',{presence:'offline',forget:{allowed:true,reason:'',effect:'local'}})],['controllers']);
 assert.equal(r.snapshot.components[0].hidden,null);assert.equal(r.snapshot.components[0].forget.allowed,false);
 await assert.rejects(async()=>await r.forget('wheel'));
 await r.update([observation('wheel',{presence:'offline',forget:{allowed:true,reason:'',effect:'local'}})],['controllers','visibility']);
 assert.equal(r.snapshot.components[0].forget.allowed,true);
});
test('duplicates are rejected; all known types remain searchable without connection evidence',async()=>{
 const r=new SystemInventoryRegistry();await assert.rejects(async()=>await r.update([observation(),observation()],['controllers']),/duplicate/);
 const components=(await r.update(['integration','virtual','gateway','osg','companion'].map(type=>observation(type,{type,connections:[],presence:'unknown'})),[])).components;
 assert.equal(filterSystem(components,'','all').length,4);assert.equal(filterSystem(components,'','virtual').length,1);assert.equal(filterSystem(components,'missing','all').length,0);
});

test('transport filters include current and remembered routes and unknown unconnected nodes',async()=>{
 const r=new SystemInventoryRegistry();const components=(await r.update([observation('wheel'),observation('network',{connections:[{from:'host',to:'network',id:'n',transport:'network',current:false}]}),observation('unknown',{connections:[]})],[])).components;
 assert.equal(filterSystem(components,'','all','usb').length,1);assert.equal(filterSystem(components,'','all','network')[0].id,'network');assert.equal(filterSystem(components,'','all','unknown')[0].id,'unknown');
});

test('unavailable providers and restart clear current input/configuration ownership rather than repeating old claims',async()=>{
 const disk=storage(),r=new SystemInventoryRegistry(disk);
 await r.update([observation('wheel',{owner:'Setup',inputOwner:'USB',configurationOwner:'Setup',configurationRoute:'USB · COM7'})],['controllers']);
 const reload=new SystemInventoryRegistry(disk);assert.equal(reload.snapshot.components[0].configurationOwner,null);assert.equal(reload.snapshot.components[0].inputOwner,null);
 await r.update([],[]);assert.equal(r.snapshot.components[0].configurationRoute,null);assert.equal(r.snapshot.components[0].configurationOwner,null);
});
test('storage errors are explicit and failed durable removal is not reported successful',async()=>{
 const disk={getItem:()=>'{',setItem:()=>{throw Error('full');}},r=new SystemInventoryRegistry(disk);
 assert.match(r.snapshot.diagnostics[0],/loaded/);await r.update([observation('wheel',{presence:'offline',forget:{allowed:true,reason:'',effect:'local'}})],['controllers']);
 assert.match(r.snapshot.diagnostics[0],/loaded/);await assert.rejects(async()=>await r.forget('wheel'));
});

const offline = (id, extra={}) => observation(id,{presence:'offline',connections:[],forget:{allowed:true,reason:'',effect:'Local cleanup'},...extra});
const staleGateway = () => offline('gateway:old',{type:'gateway',provider:'gateways',targets:[{kind:'gateway',id:'old'}],connections:[{id:'gateway-usb',from:'host',to:'gateway:old',transport:'usb',current:false}]});
const gatewayChild = (extra={}) => offline('resource:panel',{type:'osg',provider:'gateways',targets:[{kind:'module',id:'panel',gatewayId:'old'}],evidence:['route:old/panel','physical-identity'],references:[{kind:'binding',id:'steering',name:'Steering'}],connections:[{id:'panel-can',from:'gateway:old',to:'resource:panel',transport:'can',current:false}],...extra});

test('forgetting a stale gateway removes its sole-route children and suppresses unchanged provider rediscovery across restart',async()=>{
 const disk=storage(),registry=new SystemInventoryRegistry(disk),observations=[staleGateway(),gatewayChild()];
 await registry.update(observations,['gateways'],[],10);await registry.forget('gateway:old');
 assert.equal(registry.snapshot.components.length,0);
 const saved=JSON.parse(disk.getItem(INVENTORY_STORAGE_KEY));
 assert.deepEqual(Object.keys(saved.forgotten).sort(),['gateway:old','resource:panel']);
 const restarted=new SystemInventoryRegistry(disk);
 await restarted.update(observations.map(component=>({...component,observedAt:999,status:'Polling counter changed'})),['gateways'],[],20);
 assert.equal(restarted.snapshot.components.length,0);
 await restarted.update([{...staleGateway(),presence:'connected',connections:[{id:'gateway-usb',from:'host',to:'gateway:old',transport:'usb',current:true}]},{...gatewayChild(),presence:'connected',connections:[{id:'panel-can',from:'gateway:old',to:'resource:panel',transport:'can',current:true}]}],['gateways'],[],30);
 assert.equal(restarted.snapshot.components.length,2);
 assert.equal(restarted.snapshot.components.every(component=>component.presence==='connected'),true);
});

test('gateway cleanup retains a physical component with independently observed USB, routes, targets and binding references',async()=>{
 const registry=new SystemInventoryRegistry(storage());
 const child=gatewayChild({presence:'connected',owner:'USB input session',targets:[{kind:'module',id:'panel',gatewayId:'old'},{kind:'usb',id:'serial'},{kind:'controller',id:'hid-interface'}],connections:[...gatewayChild().connections,{id:'panel-usb',from:'host',to:'resource:panel',transport:'usb',current:true}],evidence:['route:old/panel','physical-identity','usb-serial']});
 const before=JSON.stringify(child);
 await registry.update([staleGateway(),child],['gateways','usb','controllers']);await registry.forget('gateway:old');
 const retained=registry.snapshot.components[0];
 assert.equal(retained.id,'resource:panel');assert.equal(retained.presence,'connected');assert.equal(retained.owner,'USB input session');
 assert.equal(retained.provider,'usb');
 assert.deepEqual(retained.connections.map(connection=>connection.id),['panel-usb']);
 assert.deepEqual(retained.targets.map(target=>target.kind),['usb','controller']);
 assert.deepEqual(retained.references,[{kind:'binding',id:'steering',name:'Steering'}]);
 assert.deepEqual(retained.evidence,['physical-identity','usb-serial']);
 assert.equal(JSON.stringify(child),before,'cleanup must not mutate provider snapshots');
 await registry.update([staleGateway(),child],['gateways','usb','controllers']);
 assert.equal(registry.snapshot.components.length,1);
 assert.equal(registry.snapshot.components[0].connections.some(connection=>connection.from==='gateway:old'),false);
 await registry.update([],['gateways']);
 assert.equal(registry.snapshot.components[0].presence,'unknown','remaining USB provider failure must not be mistaken for gateway-confirmed absence');
});

test('gateway cleanup retains another gateway route and recursively removes only orphaned downstream nodes',async()=>{
 const registry=new SystemInventoryRegistry();
 const other=offline('gateway:other',{type:'gateway',provider:'gateways',targets:[{kind:'gateway',id:'other'}]});
 const child=gatewayChild({targets:[...gatewayChild().targets,{kind:'module',id:'panel-other',gatewayId:'other'}],connections:[...gatewayChild().connections,{id:'other-can',from:'gateway:other',to:'resource:panel',transport:'can',current:false}]});
 const sole=gatewayChild({id:'module:old/sole',targets:[{kind:'module',id:'sole',gatewayId:'old'}],connections:[{id:'sole-can',from:'gateway:old',to:'module:old/sole',transport:'can',current:false}]});
 const downstream=offline('software:dependent',{type:'integration',connections:[{id:'dependent',from:sole.id,to:'software:dependent',transport:'software',current:false}]});
 await registry.update([staleGateway(),other,child,downstream,sole],['gateways']);await registry.forget('gateway:old');
 assert.deepEqual(registry.snapshot.components.map(component=>component.id).sort(),['gateway:other','resource:panel']);
 const panel=registry.snapshot.components.find(component=>component.id==='resource:panel');
 assert.deepEqual(panel.targets,[{kind:'module',id:'panel-other',gatewayId:'other'}]);
 assert.deepEqual(panel.connections.map(connection=>connection.from),['gateway:other']);
 assert.equal(registry.snapshot.components.some(component=>component.connections.some(connection=>['gateway:old','module:old/sole'].includes(connection.from))),false);
});

test('an owned sole-route child blocks parent cleanup before persistence changes',async()=>{
 const disk=storage(),registry=new SystemInventoryRegistry(disk);
 await registry.update([staleGateway(),gatewayChild({owner:'Setup session'})],['gateways']);
 const before=disk.getItem(INVENTORY_STORAGE_KEY),snapshot=registry.snapshot;
 await assert.rejects(async()=>await registry.forget('gateway:old'),/owned child/);
 assert.equal(registry.snapshot,snapshot);assert.equal(disk.getItem(INVENTORY_STORAGE_KEY),before);
});

test('failed durable cleanup leaves visible records, hidden memory and all child routes intact, then permits a clean retry',async()=>{
 const backing=storage();let fail=false;
 const disk={getItem:key=>backing.getItem(key),setItem:(key,value)=>{if(fail)throw Error('quota');backing.setItem(key,value);}};
 const registry=new SystemInventoryRegistry(disk),observations=[staleGateway(),gatewayChild()];
 await registry.update(observations,['gateways'],[],10);
 const before=backing.getItem(INVENTORY_STORAGE_KEY),snapshot=registry.snapshot;
 fail=true;await assert.rejects(async()=>await registry.forget('gateway:old'),/saved/);
 assert.equal(registry.snapshot,snapshot);assert.equal(backing.getItem(INVENTORY_STORAGE_KEY),before);
 await registry.update(observations,['gateways'],[],20);
 assert.equal(registry.snapshot.components.length,2,'failed cleanup must not leave in-memory tombstones');
 assert.equal(registry.snapshot.components.find(component=>component.type==='osg').connections[0].from,'gateway:old');
 fail=false;await registry.forget('gateway:old');assert.equal(registry.snapshot.components.length,0);
 assert.equal(registry.snapshot.diagnostics.some(diagnostic=>diagnostic.includes('could not be saved')),false);
});

test('duplicate refreshes do not partially clear durable tombstones, and never-connected records have no fabricated connected timestamp',async()=>{
 const registry=new SystemInventoryRegistry(storage());
 await registry.update([offline('never')],['controllers'],[],10);
 assert.equal(registry.snapshot.components[0].firstSeen,10);assert.equal(registry.snapshot.components[0].lastSeen,0);
 await registry.forget('never');
 await assert.rejects(async()=>await registry.update([observation('never'),observation('never')],['controllers']),/duplicate/);
 await registry.update([offline('never')],['controllers']);assert.equal(registry.snapshot.components.length,0);
 await registry.update([observation('never')],['controllers'],[],30);
 assert.equal(registry.snapshot.components[0].lastSeen,30);
 await registry.update([offline('never')],['controllers'],[],40);assert.equal(registry.snapshot.components[0].lastSeen,30);
});

test('legacy tombstones remain readable and malformed persisted tombstones cannot break future refreshes',async()=>{
 for(const forgotten of [[],{bad:null},{bad:{signature:1,sawAbsent:false}},{bad:{signature:'old',sawAbsent:false,type:'bad'}}]){
  const disk=storage();disk.setItem(INVENTORY_STORAGE_KEY,JSON.stringify({components:[],forgotten}));
  const registry=new SystemInventoryRegistry(disk);await registry.update([observation()],['controllers']);assert.equal(registry.snapshot.components.length,1);
 }
 const disk=storage(),registry=new SystemInventoryRegistry(disk),item=offline('__proto__');
 await registry.update([item],['controllers']);await registry.forget('__proto__');
 const restarted=new SystemInventoryRegistry(disk);await restarted.update([item],['controllers']);assert.equal(restarted.snapshot.components.length,0);
});

test('bound integrations appear without telemetry and panels stay on companions',async()=>{
 const items=[observation('integration:acr',{type:'integration',presence:'unknown',evidence:['active-bindings']}),observation('integration:dcs',{type:'integration'}),observation('companion-instance:panel',{type:'companion'}),observation('companion:tablet',{type:'companion',status:'Panel: Radio'})];
 assert.deepEqual(filterSystem(items,'','all').map(c=>c.id),['integration:acr','companion:tablet']);
 assert.deepEqual(filterSystem(items,'Radio','all').map(c=>c.id),['companion:tablet']);
 assert.equal(filterSystem([{...items[0],stale:true}],'','all').length,0);
});

test('device inventory excludes unpaired setup records and drops revoked installations',async()=>{
 const disk=storage(),registry=new SystemInventoryRegistry(disk);
 const paired=observation('companion:tablet',{type:'companion',provider:'companions',presence:'offline'});
 const setup=observation('companion-usb:COM7',{type:'companion',provider:'companions'});
 const pending=observation('companion-pending:request',{type:'companion',provider:'companions'});
 await registry.update([paired,setup,pending],['companions']);
 assert.deepEqual(filterSystem(registry.snapshot.components,'','all').map(c=>c.id),['companion:tablet']);
 const reloaded=new SystemInventoryRegistry(disk);assert.deepEqual(filterSystem(reloaded.snapshot.components,'','all').map(c=>c.id),['companion:tablet']);
 await reloaded.update([],[],['Companions unavailable']);assert.equal(filterSystem(reloaded.snapshot.components,'','all').length,1);
 await reloaded.update([],['companions']);assert.equal(filterSystem(reloaded.snapshot.components,'','all').length,0);
});
test('a stale registry cannot resurrect another windows acknowledged Forget tombstone',async()=>{
 const disk=storage(),first=new SystemInventoryRegistry(disk),second=new SystemInventoryRegistry(disk),item=offline('shared');
 await first.update([item],['controllers']);await second.update([item],['controllers']);await first.forget('shared');await second.update([item],['controllers']);
 assert.equal(second.snapshot.components.length,0);assert.ok(JSON.parse(disk.getItem(INVENTORY_STORAGE_KEY)).forgotten.shared);
});
test('Forget waits for the durable tombstone before removing the visible record',async()=>{
 const disk=storage(),original=disk.setItem;let release,delay=false;disk.setItem=async(key,value)=>{if(delay)await new Promise(resolve=>release=resolve);original(key,value);};
 const registry=new SystemInventoryRegistry(disk);await registry.update([offline('pending')],['controllers']);delay=true;let completed=false;const forget=registry.forget('pending').then(()=>completed=true);
 while(!release)await new Promise(setImmediate);assert.equal(completed,false);assert.equal(registry.snapshot.components.length,1);release();await forget;assert.equal(registry.snapshot.components.length,0);assert.ok(JSON.parse(disk.getItem(INVENTORY_STORAGE_KEY)).forgotten.pending);
});
test('corrupt stored inventory remains untouched while live inspection remains available',async()=>{
 let writes=0;const disk={getItem:()=>'{corrupt',setItem:()=>writes++};const registry=new SystemInventoryRegistry(disk);await registry.update([observation()],['controllers']);assert.equal(registry.snapshot.components.length,1);assert.match(registry.snapshot.diagnostics[0],/loaded/);assert.equal(writes,0);
});
