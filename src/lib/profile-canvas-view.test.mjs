import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-canvas-view']);
const {profileCanvasView:view}=modules.get('./profile-canvas-view');
const {profileTabDocument}=modules.get('./profile-vehicle-tabs');
const {signalOverview}=modules.get('./profile-signal-overview');
const fixture=JSON.parse(await readFile(new URL('../../examples/dcs-bios/feature-lab.profile.json',import.meta.url),'utf8'));
const plain=value=>JSON.parse(JSON.stringify(value));
const ids=document=>[...document.resources,...document.processingNodes,...document.virtualOutputs,...(document.desktopOutputs??[])].map(node=>node.id);

test('Base projection is unchanged and has no references',()=>{
 const original=plain(fixture),result=view(fixture);
 assert.deepEqual(plain(result.document),plain(profileTabDocument(fixture)));assert.equal(result.referenceNodeIds.size,0);assert.equal(result.referenceProcessingIds.size,0);assert.deepEqual(fixture,original);
});

test('actual AH-6J routes gain their two complete arrows and only the required Base references',()=>{
 const original=plain(fixture),result=view(fixture,'vehicle-1'),bundles=signalOverview(result.document);
 assert.equal(bundles.length,2);
 assert.deepEqual(plain(bundles.map(bundle=>[bundle.binding.from.nodeId,bundle.binding.to.nodeId]).sort()),[['ah6-dcs','aux-panel'],['aux-panel','ah6-dcs']]);
 assert.deepEqual([...result.referenceNodeIds],['aux-panel']);assert.deepEqual([...result.referenceProcessingIds],['panel-lighting']);
 assert.equal(result.document.bindings.length,8);assert.ok(result.document.bindings.some(binding=>binding.id==='route-53'));assert.ok(result.document.bindings.some(binding=>binding.id==='route-64'));
 assert.ok(ids(result.document).every(id=>!id.startsWith('hornet-')));assert.ok(!ids(result.document).includes('right-stick'));assert.ok(!ids(result.document).includes('game-context-dcs-1'));
 assert.deepEqual(fixture,original);
});

test('reference endpoint positions are deterministic presentation data and local positions remain exact',()=>{
 const original=plain(fixture),first=view(fixture,'vehicle-1'),second=view(fixture,'vehicle-1');
 assert.deepEqual(plain(first.document.editor.positions),plain(second.document.editor.positions));
 for(const id of fixture.vehicleTabs[0].nodeIds)assert.deepEqual(plain(first.document.editor.positions.find(position=>position.nodeId===id)),fixture.editor.positions.find(position=>position.nodeId===id));
 const reference=first.document.editor.positions.find(position=>position.nodeId==='aux-panel');assert.ok(reference);assert.notDeepEqual(plain(reference),fixture.editor.positions.find(position=>position.nodeId==='aux-panel'));
 reference.x+=900;first.document.resources.find(resource=>resource.id==='aux-panel').name='Changed projection';assert.deepEqual(fixture,original);
});

test('other aircraft remains isolated while new unconnected local blocks and local incomplete routes survive',()=>{
 const document=plain(fixture),tab=document.vehicleTabs[1];
 document.resources.push({id:'new-local',name:'New local',kind:'game',required:false,ports:[]});tab.nodeIds.push('new-local');
 document.processingNodes.push({...plain(document.processingNodes.find(node=>node.id==='hornet-brightness-ratio')),id:'local-incomplete'});tab.nodeIds.push('local-incomplete');
 document.bindings.push({id:'incomplete-local',from:{nodeId:'new-local',portId:'missing'},to:{nodeId:'local-incomplete',portId:'in'}});
 const before=plain(document),result=view(document,tab.id);
 assert.ok(ids(result.document).includes('new-local'));assert.ok(ids(result.document).includes('local-incomplete'));assert.ok(result.document.bindings.some(binding=>binding.id==='incomplete-local'));
 assert.ok(ids(result.document).every(id=>!id.startsWith('ah6-')));assert.equal(signalOverview(result.document).length,2);assert.deepEqual(document,before);
 assert.throws(()=>view(document,'missing'),/existing vehicle tab/);
});

test('a local transformation between Base endpoints retains its complete path and excludes unrelated Base edges',()=>{
 const document=plain(fixture),processor=plain(document.processingNodes[0]);processor.id='local-transform';
 document.resources=[{...plain(document.resources[0]),id:'base-source'},{...plain(document.resources[0]),id:'base-destination'},{...plain(document.resources[0]),id:'unused-base'}];
 document.processingNodes=[processor];document.virtualOutputs=[];delete document.desktopOutputs;
 document.vehicleTabs=[{id:'tab',gameId:'dcs',vehicleId:'AH-6J',vehicleName:'AH-6J',nodeIds:['local-transform']}];
 document.bindings=[{id:'upstream',from:{nodeId:'base-source',portId:'out'},to:{nodeId:'local-transform',portId:'in'}},{id:'terminal',from:{nodeId:'local-transform',portId:'out'},to:{nodeId:'base-destination',portId:'in'}},{id:'unrelated',from:{nodeId:'unused-base',portId:'out'},to:{nodeId:'base-destination',portId:'other'}}];
 const result=view(document,'tab');assert.deepEqual([...result.referenceNodeIds],['base-source','base-destination']);assert.equal(result.referenceProcessingIds.size,0);assert.deepEqual(plain(result.document.bindings.map(binding=>binding.id)),['upstream','terminal']);assert.ok(!ids(result.document).includes('unused-base'));
});
