import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';

const {profileRouteVehicleScope:scope,profileVehicleScopesOverlap:overlap}=(await loadModules(['./profile-integration-scope'])).get('./profile-integration-scope');
const wire=(from,to)=>({id:`${from}-${to}`,from:{nodeId:from,portId:'out'},to:{nodeId:to,portId:'in'}});
const fresh=()=>({bindings:[],processingNodes:[],vehicleTabs:[{id:'hornet',nodeIds:['hornet-game']},{id:'viper',nodeIds:['viper-game']}]});

test('route scope follows either endpoint while Common and hardware remain shared',()=>{
  const document=fresh();
  assert.equal(scope(document,'common','hardware'),'');
  assert.equal(scope(document,'hornet-game','hardware'),'hornet');
  assert.equal(scope(document,'hardware','viper-game'),'viper');
  assert.equal(scope(document,'hornet-game','hornet-game'),'hornet');
  document.bindings.push(wire('hardware','hornet-game'),wire('viper-game','hardware'));
  assert.equal(scope(document,'hardware','common'),'','Other hardware routes do not scope a shared block.');
  assert.equal(scope({bindings:[]},'hardware','common'),'');
});

test('processing ancestry preserves the unique vehicle scope across a chain and cycle',()=>{
  const document=fresh();document.processingNodes=[{id:'first'},{id:'second'}];
  document.bindings=[wire('hornet-game','first'),wire('first','second'),wire('second','first')];
  assert.equal(scope(document,'second','hardware'),'hornet');
  assert.equal(scope(document,'hardware','second'),'hornet');
  document.vehicleTabs[0].nodeIds.push('first');
  assert.equal(scope(document,'second','hardware'),'hornet');
});

test('mutually exclusive endpoint or processing scopes reject an impossible route without mutation',()=>{
  const document=fresh(),before=JSON.stringify(document);
  assert.throws(()=>scope(document,'hornet-game','viper-game'),/Different vehicles/);
  assert.equal(JSON.stringify(document),before);
  document.processingNodes=[{id:'merge'}];document.bindings=[wire('hornet-game','merge'),wire('viper-game','merge')];
  assert.throws(()=>scope(document,'merge','hardware'),/Different vehicles/);
});

test('Base overlaps every execution and vehicle scopes overlap only themselves',()=>{
  assert.equal(overlap('',''),true);assert.equal(overlap('','hornet'),true);assert.equal(overlap('viper',''),true);
  assert.equal(overlap('hornet','hornet'),true);assert.equal(overlap('hornet','viper'),false);
});
