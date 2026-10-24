import test from 'node:test';
import assert from 'node:assert/strict';
import {inventoryStorage} from './inventory-storage.ts';
import {uiPreferences,UI_KEYS} from './ui-preferences.ts';
import {INVENTORY_STORAGE_KEY} from '../system-inventory.ts';
test('the inventory adapter uses hydrated revisioned native data and never browser storage',async()=>{
 const entries=Object.fromEntries(UI_KEYS.map(key=>[key,{revision:0,value:null}]));let fail=false,reads=0;const writes=[];
 uiPreferences.connect({read:async()=>{reads++;if(fail)throw Error('SQL unavailable');return{entries:structuredClone(entries)};},write:async(key,revision,value)=>{writes.push({key,revision,value});if(fail)throw Error('busy');entries[key]={revision:revision+1,value};return entries[key];}});await inventoryStorage.refresh();assert.equal(inventoryStorage.getItem(INVENTORY_STORAGE_KEY),null);
 const memory={components:[],forgotten:{offline:{signature:'known',sawAbsent:false}}};await inventoryStorage.setItem(INVENTORY_STORAGE_KEY,JSON.stringify(memory));assert.deepEqual(JSON.parse(inventoryStorage.getItem(INVENTORY_STORAGE_KEY)),memory);assert.deepEqual(writes[0],{key:'remembered',revision:0,value:memory});
 assert.throws(()=>inventoryStorage.getItem('secrets'),/Unknown/);await assert.rejects(inventoryStorage.setItem('secrets','null'),/Unknown/);await assert.rejects(inventoryStorage.setItem(INVENTORY_STORAGE_KEY,'invalid'));
 fail=true;await assert.rejects(inventoryStorage.setItem(INVENTORY_STORAGE_KEY,'null'),/busy/);assert.throws(()=>inventoryStorage.getItem(INVENTORY_STORAGE_KEY),/busy/);await assert.rejects(inventoryStorage.refresh(),/refreshed/);
 fail=false;await inventoryStorage.refresh();assert.ok(reads>=3);assert.deepEqual(JSON.parse(inventoryStorage.getItem(INVENTORY_STORAGE_KEY)),memory);
});
