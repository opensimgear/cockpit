import test from 'node:test';
import assert from 'node:assert/strict';
import { loadModules } from '../test-module-loader.mjs';

const modules=await loadModules(['./system-inventory-providers'],new Map([
  ['./game-read-models/shared',{observeGamesState(){return ()=>{};}}],
  ['./controller-read-models/shared',{observeControllerInventory(){return ()=>{};}}],
  ['./profile-read-models/shared',{observeProfileRuntime(){return ()=>{};}}],
]));
const {gatewayObservations}=modules.get('./system-inventory-providers');
const entry=()=>({id:'gateway',port:{backend:'stm32',path:'COM1'},present:true,busy:false,error:'',stale:false,
  state:{connected:true,claimedModules:['panel'],modules:[{id:'panel',name:'Panel',online:true,active:true,registered:true,settings:{}}]}});

test('workspace paging retains known presence while loss withdraws capture and editing authority',()=>{
  for(const loading of [true,false]){
    const row={...entry(),stale:true,loading};
    const [gateway,module]=gatewayObservations([row],1);
    assert.equal(gateway.presence,loading?'connected':'unknown');
    assert.equal(module.presence,loading?'connected':'unknown');
    assert.equal(module.capture,loading?'ready':'unknown');
    assert.equal(module.configurationOwner,loading?'Setup session':null);
    assert.equal(module.stale,!loading);
    assert.equal(gateway.connections[0].current,loading);
    assert.equal(module.connections[0].current,loading);
    assert.equal(module.forget.allowed,false);
  }
});
