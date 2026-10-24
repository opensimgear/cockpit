import assert from 'node:assert/strict';
import test from 'node:test';
import {progressNotificationSettings,PROGRESS_NOTIFICATION_POSITIONS} from './progress-notification-settings.ts';
import {UiPreferences,UI_KEYS} from './persistence/ui-preferences.ts';
test('every selectable notification style and corner uses a validated preference',()=>{
 for(const delivery of ['system','window'])for(const [position] of PROGRESS_NOTIFICATION_POSITIONS)assert.deepEqual(progressNotificationSettings({delivery,position}),{delivery,position});
 for(const value of [null,{},[],{delivery:'off',position:'top-left'},{delivery:'window',position:'center'},{position:'top-left'},{delivery:'window'}])assert.throws(()=>progressNotificationSettings(value),/unavailable/);
});
test('progress choices acknowledge the exact shared revision and preserve the other preference',async()=>{
 const entries=Object.fromEntries(UI_KEYS.map(key=>[key,{revision:0,value:null}])),writes=[];const store=new UiPreferences({read:async()=>({entries}),write:async(key,revision,value)=>{writes.push({key,revision,value});entries[key]={revision:revision+1,value};return entries[key];}});await store.hydrate();
 for(const [position] of PROGRESS_NOTIFICATION_POSITIONS){await store.save('progress',{...store.get('progress').value,position});assert.equal(store.get('progress').value.delivery,'system');}
 await store.save('progress',{...store.get('progress').value,delivery:'window'});assert.equal(store.get('progress').value.position,'top-left');assert.deepEqual(writes.map(w=>w.revision),[0,1,2,3,4]);assert.ok(writes.every(w=>w.key==='progress'));assert.equal(store.get('progress').revision,5);
});
