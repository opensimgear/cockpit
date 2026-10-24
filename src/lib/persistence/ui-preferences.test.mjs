import test from 'node:test';
import assert from 'node:assert/strict';
import {UiPreferences,UI_KEYS,validateUiValue,uiPreference,uiPreferences} from './ui-preferences.ts';
function native(){
 const entries=Object.fromEntries(UI_KEYS.map(key=>[key,{revision:0,value:null}])),writes=[];
 return {entries,writes,read:async()=>structuredClone({entries}),write:async(key,expected,value)=>{
  writes.push({key,expected,value:structuredClone(value)});if(entries[key].revision!==expected)throw Error('revision conflict');entries[key]={revision:expected+1,value:structuredClone(value)};return structuredClone(entries[key]);
 }};
}
test('native hydration completes before saves and restores all preferences at exact revisions',async()=>{
 const disk=native();disk.entries.theme={revision:4,value:true};disk.entries['devices-mode']={revision:2,value:'topology'};
 const store=new UiPreferences(disk);assert.equal(store.get('theme').ready,false);await assert.rejects(store.save('theme',false),/Refresh/);assert.equal(disk.writes.length,0);
 assert.equal(await store.hydrate(),true);assert.equal(store.get('theme').value,true);assert.equal(store.get('devices-mode').value,'topology');assert.equal(store.get('canvas-styling').value.cornerRadius,24);
 await store.save('theme',false);assert.deepEqual(disk.writes,[{key:'theme',expected:4,value:false}]);const restarted=new UiPreferences(disk);await restarted.hydrate();assert.equal(restarted.get('theme').value,false);assert.equal(restarted.get('theme').revision,5);
});
test('two windows conflict without overwriting a newer commit and refresh is required before retry',async()=>{
 const disk=native(),first=new UiPreferences(disk),second=new UiPreferences(disk);await Promise.all([first.hydrate(),second.hydrate()]);await first.save('theme',true);
 await assert.rejects(second.save('theme',false),/revision conflict/);assert.equal(disk.entries.theme.value,true);assert.equal(second.get('theme').ready,false);assert.match(second.get('theme').error,/Refresh/);await assert.rejects(second.save('theme',false),/Refresh/);
 await second.changed('theme',1);assert.equal(second.get('theme').value,true);await second.save('theme',false);assert.equal(disk.entries.theme.revision,2);
});
test('values and subscribers advance only after commit and in-flight duplicate saves are rejected',async()=>{
 const disk=native();let release;const original=disk.write;disk.write=async(...args)=>{await new Promise(resolve=>release=resolve);return original(...args);};
 const store=new UiPreferences(disk);await store.hydrate();const seen=[];const unsubscribe=store.subscribe('theme',entry=>seen.push(entry));const pending=store.save('theme',true);
 assert.equal(store.get('theme').value,false);assert.equal(store.get('theme').busy,true);await assert.rejects(store.save('theme',false),/Wait/);release();await pending;assert.equal(store.get('theme').value,true);assert.equal(store.get('theme').busy,false);
 unsubscribe();const length=seen.length;await store.changed('theme',1);assert.equal(seen.length,length);assert.equal(disk.writes.length,1);
});
test('database failure and malformed or uncertain acknowledgements preserve displayed values and block blind retry',async()=>{
 for(const write of [async()=>{throw Error('busy');},async()=>({revision:99,value:true}),async()=>({revision:1,value:'bad'}),async()=>({revision:1,value:false})]){
  const disk=native();disk.write=write;const store=new UiPreferences(disk);await store.hydrate();await assert.rejects(store.save('theme',true));assert.equal(store.get('theme').value,false);assert.equal(store.get('theme').ready,false);assert.equal(store.get('theme').busy,false);assert.match(store.get('theme').error,/could not be saved/);
 }
});
test('incomplete, corrupt or inaccessible native snapshots never fall back to browser storage or permit saves',async()=>{
 for(const read of [async()=>{throw Error('corrupt SQL');},async()=>({entries:{}}),async()=>null,async()=>{const disk=native();disk.entries.theme.revision=-1;return disk.read();},async()=>{const disk=native();disk.entries.theme.value='invalid';return disk.read();}]){
  const store=new UiPreferences({read,write:async()=>{throw Error('must not write');}});assert.equal(await store.hydrate(),false);assert.match(store.get('theme').error,/could not be loaded/);await assert.rejects(store.save('theme',true),/Refresh/);
 }
});
test('cross-window events refresh shared revisions and stale events cannot rewind state',async()=>{
 const disk=native(),store=new UiPreferences(disk);await store.hydrate();disk.entries['devices-mode']={revision:3,value:'topology'};await store.changed('devices-mode',3);assert.equal(store.get('devices-mode').value,'topology');
 disk.entries['devices-mode']={revision:2,value:'list'};await store.hydrate();assert.equal(store.get('devices-mode').revision,3);assert.equal(store.get('devices-mode').value,'topology');
 await store.changed('devices-mode',2);await store.changed('unknown',10);await store.changed('theme',NaN);
});
test('an event overtaking an old in-flight read triggers another read and never loses the new revision',async()=>{
 const disk=native(),store=new UiPreferences(disk);await store.hydrate();let release;const read=disk.read;disk.read=async()=>{const snapshot=await read();await new Promise(resolve=>release=resolve);return snapshot;};
 const pending=store.hydrate();while(!release)await new Promise(resolve=>setImmediate(resolve));assert.equal(store.hydrate(),pending);disk.entries.theme={revision:1,value:true};disk.read=read;const changed=store.changed('theme',1);release();await Promise.all([pending,changed]);assert.equal(store.get('theme').value,true);
});
test('a delayed successful save cannot overwrite a newer observed native edit',async()=>{
 const disk=native(),store=new UiPreferences(disk);await store.hydrate();let release;const write=disk.write;disk.write=async(...args)=>{const result=await write(...args);await new Promise(resolve=>release=resolve);return result;};
 const pending=store.save('theme',true);while(!release)await new Promise(resolve=>setImmediate(resolve));disk.entries.theme={revision:2,value:false};await store.changed('theme',2);release();await pending;assert.equal(store.get('theme').value,false);assert.equal(store.get('theme').revision,2);
});
test('preview storage is session memory and cannot silently switch authority after an edit',async()=>{
 const preview=new UiPreferences();assert.equal(await preview.hydrate(),true);await preview.save('theme',true);assert.throws(()=>preview.connect(native()),/already initialized/);assert.equal(new UiPreferences().get('theme').value,false);
 const connected=new UiPreferences();connected.connect(native());assert.equal(connected.get('theme').ready,false);assert.throws(()=>connected.connect(native()),/already initialized/);await connected.hydrate();
 const theme=uiPreference('theme'),seen=[];const stop=theme.subscribe(value=>seen.push(value));await theme.save(true);assert.equal(theme.get().value,true);assert.equal(uiPreferences.get('theme').value,true);stop();
});
test('typed preference validation rejects unknown fields and malformed values before writes',()=>{
 const styling={arrowStyle:'orthogonal',cornerRadius:0,collapseParallelWires:false};assert.deepEqual(validateUiValue('canvas-styling',styling),styling);assert.deepEqual(validateUiValue('remembered',{components:[],forgotten:{}}),{components:[],forgotten:{}});
 for(const [key,value] of [['theme','true'],['devices-mode','grid'],['canvas-navigation',{wheelAction:'other'}],['canvas-navigation',{wheelAction:'pan',extra:true}],['canvas-styling',{...styling,cornerRadius:101}],['canvas-styling',{...styling,cornerRadius:NaN}],['canvas-styling',{...styling,collapseParallelWires:1}],['remembered',[]],['remembered',{components:new Array(1025),forgotten:{}}],['remembered',{components:[],forgotten:new Array(1)}]])assert.throws(()=>validateUiValue(key,value),/invalid/);
});
