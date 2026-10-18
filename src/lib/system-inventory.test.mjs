import test from 'node:test';
import assert from 'node:assert/strict';
import { SystemInventoryRegistry, filterSystem, systemStateLabel } from './system-inventory.ts';
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
 assert.equal(filterSystem(components,'','all').length,5);assert.equal(filterSystem(components,'','virtual').length,1);assert.equal(filterSystem(components,'missing','all').length,0);
});
test('storage errors are explicit and failed durable removal is not reported successful',()=>{
 const disk={getItem:()=>'{',setItem:()=>{throw Error('full');}},r=new SystemInventoryRegistry(disk);
 assert.match(r.snapshot.diagnostics[0],/loaded/);r.update([observation('wheel',{presence:'offline',forget:{allowed:true,reason:'',effect:'local'}})],['controllers']);
 assert.match(r.snapshot.diagnostics[0],/saved/);assert.throws(()=>r.forget('wheel'),/saved/);
});
