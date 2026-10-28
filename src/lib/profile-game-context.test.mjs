import {discoveredGames} from './game-test-fixtures.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./games','./profile-game-context','./profile-editor','./profile-graph-history','./profiles']);
modules.get('./games').updateGameCatalogue(discoveredGames);
const {profileSetGameContext,profileSyncGameContext,profileGameContext,profileGameContextNeedsSync,profileAddVehicleIntegration,profileGameTabInventory,profileDcsResourceMatchesVehicle,profileReconcileDcsVehicleCatalogue,profileChangeVehicleIntegration}=modules.get('./profile-game-context');
const {blankProfile,ProfileEditorStore}=modules.get('./profile-editor'),{ProfileGraphHistory}=modules.get('./profile-graph-history');
const id='11111111-1111-4111-8111-111111111111',plain=value=>JSON.parse(JSON.stringify(value));
const source=gameId=>({id:`native:${gameId}`,name:gameId,resourceType:'game-integration',availability:'offline',attributes:{gameId,...(gameId==='dcs'?{aircraftModule:'CommonData'}:{})}});
const aircraft=(module,alias=module)=>({...source('dcs'),id:`native:dcs:${module}`,attributes:{gameId:'dcs',aircraftModule:module,vehicleAliases:JSON.stringify([module,alias])}});
const inventory=[source('dcs'),source('x-plane-12')],hornet=aircraft('FA-18C_hornet'),viper=aircraft('F-16C','F-16C_50');
const draft=()=>({document:blankProfile(id,'Context rig'),assignments:[]});

test('vehicle canvases own independent simulator integration blocks and same-game sync preserves all scopes',()=>{
 let value=profileSetGameContext(draft(),'dcs',inventory);
 value.document.vehicleTabs=[{id:'hornet',gameId:'dcs',vehicleId:'FA-18C_hornet',vehicleName:'Hornet',nodeIds:[]},{id:'viper',gameId:'dcs',vehicleId:'F-16C_50',vehicleName:'Viper',nodeIds:[]}];
 value=profileAddVehicleIntegration(value,'hornet',[]);value=profileAddVehicleIntegration(value,'viper',[]);
 assert.equal(value.document.resources.length,3);assert.equal(value.assignments.length,1);assert.equal(profileGameContextNeedsSync(value,inventory),false);assert.equal(profileGameContextNeedsSync(value,[...inventory,hornet,viper]),true);
 const scoped=plain(value.document.vehicleTabs),synced=profileSetGameContext(value,'dcs',[...inventory,hornet,viper]);
 assert.deepEqual(plain(synced.document.vehicleTabs),scoped);assert.equal(synced.document.resources.length,3);assert.equal(synced.assignments.length,3);assert.equal(profileGameContextNeedsSync(synced,inventory),false);
 assert.equal(new Set(synced.assignments.map(row=>row.sourceId)).size,3);
 assert.deepEqual(plain(profileAddVehicleIntegration(synced,'hornet',inventory)),plain(synced));
 assert.equal(profileSetGameContext(synced,'',inventory).document.resources.length,0);
});

test('DCS tab catalogues match aliases, expose only their module and safely retarget empty integrations',()=>{
 let value=profileSetGameContext(draft(),'dcs',inventory);value.document.vehicleTabs=[{id:'aircraft',gameId:'dcs',vehicleId:'F-16C_50',vehicleName:'Viper',nodeIds:[]}];
 const liveBase={...source('dcs'),attributes:{...source('dcs').attributes,vehicleId:'F-16C_50',vehicleAliases:'["F-16C_50"]'}},scopedInventory=[liveBase,source('x-plane-12'),hornet,viper];
 assert.equal(profileDcsResourceMatchesVehicle(liveBase,'F-16C_50'),false);assert.equal(profileDcsResourceMatchesVehicle(liveBase,''),true);
 value=profileAddVehicleIntegration(value,'aircraft',scopedInventory);const node=value.document.vehicleTabs[0].nodeIds[0];
 assert.equal(value.assignments.find(row=>row.resourceId===node).sourceId,viper.id);
 assert.deepEqual(profileGameTabInventory(value.document,scopedInventory,'aircraft').map(row=>row.id),['native:x-plane-12',viper.id]);
 assert.deepEqual(profileGameTabInventory(value.document,scopedInventory).map(row=>row.id),['native:dcs','native:x-plane-12']);
 const pending=plain(value);pending.assignments=pending.assignments.filter(row=>row.resourceId!==node);assert.equal(profileSetGameContext(pending,'dcs',scopedInventory).assignments.find(row=>row.resourceId===node).sourceId,viper.id);
 assert.equal(profileDcsResourceMatchesVehicle({...viper,attributes:{...viper.attributes,vehicleAliases:'bad json'}},'F-16C_50'),false);
 const changed=profileChangeVehicleIntegration(value,'aircraft',{id:'FA-18C_hornet',name:'Hornet'},[...inventory,hornet,viper]);
 assert.equal(changed.assignments.find(row=>row.resourceId===node).sourceId,hornet.id);assert.equal(value.assignments.find(row=>row.resourceId===node).sourceId,viper.id);
 const unknown=profileChangeVehicleIntegration(value,'aircraft',{id:'unknown',name:'Unknown'},[...inventory,hornet,viper]);assert.equal(unknown.assignments.some(row=>row.resourceId===node),false);
});

test('legacy DCS pins migrate only with every exact contract; aircraft retarget never loses controls or routes',()=>{
 let value=profileSetGameContext(draft(),'dcs',inventory);value.document.vehicleTabs=[{id:'aircraft',gameId:'dcs',vehicleId:'F-16C_50',vehicleName:'Viper',nodeIds:[]}];
 value=profileAddVehicleIntegration(value,'aircraft',[]);const node=value.document.vehicleTabs[0].nodeIds[0],contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false},hash=`sha256:${'a'.repeat(64)}`;
 value.document.resources.find(row=>row.id===node).ports=[{id:'lamp',name:'Aircraft lamp',direction:'output',contract}];value.assignments.push({resourceId:node,sourceId:'native:dcs',ports:[{profilePortId:'lamp',observedPortId:'MASTER_CAUTION',expectedContractHash:hash}]});
 value.document.resources.push({id:'indicator',name:'Indicator',kind:'device',required:false,ports:[{id:'lamp',name:'Lamp',direction:'input',contract}]});value.document.vehicleTabs[0].nodeIds.push('indicator');value.document.bindings.push({id:'caution',from:{nodeId:node,portId:'lamp'},to:{nodeId:'indicator',portId:'lamp'}});
 const port={descriptor:{id:'MASTER_CAUTION',name:'Master caution',direction:'output',contract},contractHash:hash,routeIds:[],current:false};
 const next=profileReconcileDcsVehicleCatalogue(value,'aircraft',viper,[port],[...inventory,hornet,viper]);
 assert.equal(next.assignments.find(row=>row.resourceId===node).sourceId,viper.id);assert.deepEqual(plain(next.document),plain(value.document));assert.deepEqual(plain(next.assignments.find(row=>row.resourceId===node).ports),plain(value.assignments.find(row=>row.resourceId===node).ports));
 assert.throws(()=>profileReconcileDcsVehicleCatalogue(value,'aircraft',viper,[{...port,contractHash:`sha256:${'b'.repeat(64)}`}],inventory),/cannot preserve/);
 assert.throws(()=>profileReconcileDcsVehicleCatalogue(value,'aircraft',viper,[{...port,descriptor:{...port.descriptor,contract:{...contract,freshnessMs:500}}}],inventory),/differs/);
 assert.throws(()=>profileReconcileDcsVehicleCatalogue(value,'aircraft',hornet,[port],inventory),/does not match/);
 assert.throws(()=>profileChangeVehicleIntegration(next,'aircraft',{id:'FA-18C_hornet',name:'Hornet'},[...inventory,hornet,viper]),/configured DCS controls/);
 assert.deepEqual(plain(profileChangeVehicleIntegration(next,'aircraft',{id:'F-16C',name:'Viper canonical'},[...inventory,hornet,viper]).assignments),plain(next.assignments));
 assert.equal(value.assignments.find(row=>row.resourceId===node).sourceId,'native:dcs');assert.equal(value.document.bindings.length,1);
});

test('selecting a saved context adds exactly its integration with a native assignment in one Undo edit',()=>{
 const before=draft(),history=new ProfileGraphHistory(before),next=profileSetGameContext(before,'dcs',inventory);
 history.commit(next,'Change game context');assert.equal(profileGameContext(next.document),'dcs');assert.deepEqual(plain(next.document.eligibility.gameIds),['dcs']);assert.equal(next.document.resources.length,1);assert.equal(next.document.resources[0].kind,'game');assert.equal(next.assignments[0].sourceId,'native:dcs');assert.ok(next.document.editor.positions.some(position=>position.nodeId===next.document.resources[0].id));
 assert.deepEqual(plain(history.undo()),plain(before));assert.deepEqual(plain(history.redo()),plain(next));assert.deepEqual(plain(before),plain(draft()));
 assert.deepEqual(plain(profileSetGameContext(next,'dcs',inventory)),plain(next));
});

test('changing or removing context replaces only its integration and routes and retains other components and groups',()=>{
 let value=profileSetGameContext(draft(),'dcs',inventory);const game=value.document.resources[0];
 const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};game.ports=[{id:'signal',name:'Signal',direction:'output',contract}];value.assignments[0].ports=[{profilePortId:'signal',observedPortId:'native-signal',expectedContractHash:`sha256:${'a'.repeat(64)}`}];
 value.document.resources.push({id:'device',name:'Device',kind:'device',required:false,ports:[{id:'signal',name:'Signal',direction:'input',contract}]});value.document.bindings=[{id:'route',from:{nodeId:game.id,portId:'signal'},to:{nodeId:'device',portId:'signal'}}];value.document.editor.groups=[{id:'controls',name:'Controls',members:[game.id]}];
 const same=profileSetGameContext(value,'dcs',inventory);assert.deepEqual(plain(same),plain(value));
 const changed=profileSetGameContext(value,'x-plane-12',inventory);assert.deepEqual(plain(changed.document.eligibility.gameIds),['x-plane-12']);assert.equal(changed.document.resources.length,2);assert.equal(changed.document.resources.filter(resource=>resource.kind==='game').length,1);assert.deepEqual(plain(changed.document.bindings),[]);assert.equal(changed.assignments[0].sourceId,'native:x-plane-12');assert.equal(changed.document.editor.groups[0].name,'Controls');
 const none=profileSetGameContext(changed,'',inventory);assert.equal(profileGameContext(none.document),'');assert.deepEqual(plain(none.document.resources.map(resource=>resource.id)),['device']);assert.deepEqual(plain(none.assignments),[]);assert.equal(none.document.editor.groups.length,1);assert.equal(value.document.bindings.length,1);
});

test('context persists without inventory and acquires only an empty integration when inventory becomes available',()=>{
 const next=profileSetGameContext(draft(),'dcs',[]);assert.equal(next.document.resources[0].name,'DCS World');assert.deepEqual(plain(next.assignments),[]);assert.equal(profileGameContextNeedsSync(next,[]),false);assert.equal(profileGameContextNeedsSync(next,inventory),true);
 const assigned=profileSetGameContext(next,'dcs',inventory);assert.equal(assigned.document.resources[0].id,next.document.resources[0].id);assert.equal(assigned.assignments[0].sourceId,'native:dcs');assert.equal(profileGameContextNeedsSync(assigned,inventory),false);
 assert.equal(profileGameContextNeedsSync({document:{...blankProfile(id),eligibility:{gameIds:['dcs']}},assignments:[]},[]),false);
 assert.throws(()=>profileSetGameContext(draft(),'unknown',inventory),/discovered game/);
 const full=draft();full.document.resources=Array.from({length:64},(_,index)=>({id:`resource-${index}`,name:'Device',kind:'device',required:false,ports:[]}));assert.throws(()=>profileSetGameContext(full,'dcs',inventory),/64 resources/);assert.equal(full.document.eligibility.gameIds.length,0);
});

test('passive sync preserves removed Common while binding only existing empty vehicle blocks',()=>{
 let value=profileSetGameContext(draft(),'dcs',inventory);
 value.document.vehicleTabs=[{id:'hornet',gameId:'dcs',vehicleId:'FA-18C_hornet',vehicleName:'Hornet',nodeIds:[]}];
 value=profileAddVehicleIntegration(value,'hornet',[]);
 const common=value.document.resources.find(resource=>!value.document.vehicleTabs[0].nodeIds.includes(resource.id));
 value=modules.get('./profile-graph').graphRemoveNode(value,common.id);
 const before=plain(value),available=[...inventory,hornet];
 assert.equal(profileGameContextNeedsSync(value,available),true);
 const synced=profileSyncGameContext(value,available);
 assert.equal(synced.document.resources.length,1);assert.deepEqual(plain(synced.document),before.document);
 assert.equal(synced.assignments.length,1);assert.equal(synced.assignments[0].sourceId,hornet.id);
 assert.equal(profileGameContextNeedsSync(synced,available),false);assert.deepEqual(plain(value),before);
 const missing=plain(synced);missing.assignments[0].sourceId='old-source';
 assert.equal(profileSyncGameContext(missing,available).assignments[0].sourceId,hornet.id);
 const explicit=profileSetGameContext(synced,'dcs',available);
 assert.equal(explicit.document.resources.length,2,'Explicit context selection still adds Common.');
 value=modules.get('./profile-graph').graphRemoveNode(synced,synced.document.resources[0].id);
 assert.equal(profileGameContextNeedsSync(value,available),false);
 assert.equal(profileSyncGameContext(value,available).document.resources.length,0);
});

test('saved context survives profile editor save, reopen, portable export and copy',async()=>{
 const {DEFAULT_PROFILE_ID,exportProfileDocument,parseProfileDocument}=modules.get('./profiles'),docs=new Map([[DEFAULT_PROFILE_ID,{...blankProfile(id,'Default'),id:DEFAULT_PROFILE_ID}]]),pins=new Map(),summary=document=>({id:document.id,name:document.name,revision:document.revision,isDefault:document.id===DEFAULT_PROFILE_ID,semanticHash:`sha256:${'a'.repeat(64)}`,gameIds:plain(document.eligibility.gameIds)}),session={selectedProfileId:DEFAULT_PROFILE_ID,selectionMode:'manual',state:'stopped',appliedSnapshot:null,fault:null};
 const api={async list(){return{profiles:[...docs.values()].map(summary),session};},async getAssignments(profileId){return{document:plain(docs.get(profileId)),assignments:plain(pins.get(profileId)??[]),session};},async editorSave(document,expectedRevision,assignments){const saved={...plain(document),revision:(expectedRevision??0)+1};docs.set(document.id,saved);pins.set(document.id,plain(assignments));return{document:saved,assignments:plain(assignments),profiles:[...docs.values()].map(summary),session};}};
 let identities=0;const store=new ProfileEditorStore(api,{listAll:async()=>({resources:inventory})},()=>identities++===0?id:'33333333-3333-4333-8333-333333333333');await store.refresh();store.beginBlank([]);const next=profileSetGameContext(store.state.editor,'dcs',inventory);store.update(next.document,next.assignments);await store.save();store.back();await store.open(id);assert.equal(profileGameContext(store.state.editor.document),'dcs');assert.deepEqual(plain(store.state.profiles.find(row=>row.id===id).gameIds),['dcs']);assert.equal(profileGameContext(parseProfileDocument(exportProfileDocument(store.state.editor.document))),'dcs');store.back();await store.beginCopy(id);assert.equal(profileGameContext(store.state.editor.document),'dcs');assert.notEqual(store.state.editor.document.id,id);assert.deepEqual(plain(store.state.editor.assignments),[]);
});
