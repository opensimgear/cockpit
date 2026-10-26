import {discoveredGames} from './game-test-fixtures.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./games','./profile-vehicle-tabs','./profile-editor','./profiles','./profile-game-context']);
modules.get('./games').updateGameCatalogue(discoveredGames);
const {profileAddVehicleTab,profileRemoveVehicleTab,profileChangeVehicleTab,profileTabDocument,profileTabNodeIds,profileStampVehicleNodes,profileVehicleDocument}=modules.get('./profile-vehicle-tabs');
const {blankProfile,addProfileResources}=modules.get('./profile-editor');
const {profileDocumentError}=modules.get('./profiles');
const {profileSetGameContext}=modules.get('./profile-game-context');
const plain=value=>JSON.parse(JSON.stringify(value));
function fixture(){
  let document=blankProfile('11111111-1111-4111-8111-111111111111');document.eligibility.gameIds=['dcs'];
  document.resources.push({id:'base',name:'Shared',kind:'device',required:false,ports:[]});
  const a=profileAddVehicleTab(document,'dcs',{id:'FA-18C_hornet',name:'F/A-18C'});document=a.document;
  const b=profileAddVehicleTab(document,'dcs',{id:'F-16C_50',name:'F-16C'});document=b.document;
  document.resources.push({id:'hornet',name:'Hornet control',kind:'device',required:false,ports:[]},{id:'viper',name:'Viper control',kind:'device',required:false,ports:[]});
  document.vehicleTabs[0].nodeIds=['hornet'];document.vehicleTabs[1].nodeIds=['viper'];
  return{document,a:a.tabId,b:b.tabId};
}
test('Base and vehicle canvas views retain only their own nodes and leave the saved graph intact',()=>{
  const {document,a,b}=fixture(),before=plain(document);
  assert.deepEqual([...profileTabNodeIds(document)],['base']);
  assert.deepEqual([...profileTabNodeIds(document,a)],['hornet']);
  assert.deepEqual([...profileTabNodeIds(document,b)],['viper']);
  assert.deepEqual(plain(profileTabDocument(document,a).resources.map(node=>node.id)),['hornet']);
  assert.equal(profileTabDocument(document,a).vehicleTabs,undefined);
  assert.deepEqual(plain(document),before);assert.equal(profileDocumentError(document),'');
});
test('conflict variants combine Base with only the selected vehicle and retain their connecting edges',()=>{
 const {document,a,b}=fixture();document.bindings=[{id:'base-hornet',from:{nodeId:'base',portId:'out'},to:{nodeId:'hornet',portId:'in'}},{id:'base-viper',from:{nodeId:'base',portId:'out'},to:{nodeId:'viper',portId:'in'}}];
 const before=plain(document),variant=profileVehicleDocument(document,a);
 assert.deepEqual(plain(variant.resources.map(node=>node.id)),['base','hornet']);assert.deepEqual(plain(variant.bindings.map(binding=>binding.id)),['base-hornet']);assert.equal(variant.vehicleTabs,undefined);
 assert.deepEqual(plain(profileVehicleDocument(document,b).resources.map(node=>node.id)),['base','viper']);assert.deepEqual(plain(profileVehicleDocument(document).resources.map(node=>node.id)),['base']);assert.deepEqual(plain(document),before);
 assert.throws(()=>profileVehicleDocument(document,'missing'),/existing vehicle/);
});
test('removing a vehicle deletes its complete configuration and preserves Base and the other vehicle',()=>{
  const {document,a,b}=fixture();
  document.visibility=[{resourceId:'hornet',mode:'hidden-while-active'},{resourceId:'base',mode:'visible'}];
  document.overrides=[{resourceId:'hornet',enabled:false}];
  document.editor.positions=[{nodeId:'hornet',x:20,y:30},{nodeId:'base',x:40,y:50}];
  document.editor.groups=[{id:'g1',name:'Hornet group',members:['hornet']},{id:'g2',name:'Shared group',members:['base','viper']}];
  document.editor.collapsedPortGroups=[{nodeId:'hornet',group:'inputs'}];
  const {document:next,assignments}=profileRemoveVehicleTab(document,[{resourceId:'base',sourceId:'physical',ports:[]},{resourceId:'hornet',sourceId:'physical',ports:[]}],a);
  assert.deepEqual(plain(next.resources.map(node=>node.id)),['base','viper']);
  assert.deepEqual(plain(next.vehicleTabs.map(tab=>tab.id)),[b]);
  assert.deepEqual(plain(next.visibility),[{resourceId:'base',mode:'visible'}]);
  assert.equal(next.overrides.length,0);assert.equal(next.editor.collapsedPortGroups.length,0);
  assert.deepEqual(plain(next.editor.groups.map(group=>group.id)),['g2']);
  assert.deepEqual(plain(assignments.map(row=>row.resourceId)),['base']);
  assert.equal(document.resources.length,3);assert.equal(profileDocumentError(next),'');
});
test('new nodes are stamped once, removed membership is pruned, and duplicate vehicle tabs are rejected',()=>{
  const {document,a}=fixture(),next=plain(document);
  next.resources=next.resources.filter(node=>node.id!=='hornet');
  next.resources.push({id:'added',name:'Added',kind:'device',required:false,ports:[]});
  profileStampVehicleNodes(document,next,a);profileStampVehicleNodes(document,next,a);
  assert.deepEqual(plain(next.vehicleTabs[0].nodeIds),['added']);
  assert.throws(()=>profileAddVehicleTab(document,'dcs',{id:'F-16C_50',name:'Duplicate'}),/already/);
  assert.throws(()=>profileChangeVehicleTab(document,a,{id:'F-16C_50',name:'Duplicate'}),/already/);
  assert.equal(profileDocumentError(next),'');
});
test('the same inventory component can be added in different vehicle tabs and only once per tab',()=>{
  const {document,a,b}=fixture(),resource={id:'physical',name:'Physical input',resourceType:'host-controller'};
  const first=addProfileResources(document,[],[resource],a),second=addProfileResources(first.document,first.assignments,[resource],b);
  assert.equal(second.assignments.length,2);assert.notEqual(second.assignments[0].resourceId,second.assignments[1].resourceId);
  const third=addProfileResources(second.document,second.assignments,[resource],b);assert.equal(third.assignments.length,2);
});
test('changing game context removes all former vehicle layers and their assignments',()=>{
  const {document}=fixture();const next=profileSetGameContext({document,assignments:[{resourceId:'hornet',sourceId:'physical',ports:[]}]},'assetto-corsa-rally',[]);
  assert.equal(next.document.vehicleTabs,undefined);assert.equal(next.document.resources.some(node=>node.id==='hornet'||node.id==='viper'),false);assert.equal(next.assignments.length,0);assert.equal(profileDocumentError(next.document),'');
});

for(const operation of ['add','change'])test(`${operation} catalogue vehicle stores a valid Unicode tab label while preserving the full choice and exact ID`,()=>{
  const unicodeLabel='A'.repeat(79)+'🚗',trimmedLabel='R'.repeat(79);
  for(const [name,label] of [[unicodeLabel+' Track edition',unicodeLabel],[trimmedLabel+'\u2003Performance edition',trimmedLabel]]){
    let document=blankProfile('11111111-1111-4111-8111-111111111111');document.eligibility.gameIds=['assetto-corsa'];
    const initial=profileAddVehicleTab(document,'assetto-corsa',{id:'existing_car',name:'Existing car'});document=initial.document;
    const before=plain(document),choice=Object.freeze({id:'My_Mod_variant_S1',name});
    const next=operation==='add'?profileAddVehicleTab(document,'assetto-corsa',choice).document:profileChangeVehicleTab(document,initial.tabId,choice);
    const tab=next.vehicleTabs.find(tab=>tab.vehicleId===choice.id);
    assert.equal(tab.vehicleId,'My_Mod_variant_S1');assert.equal(tab.vehicleName,label);
    assert.equal(profileDocumentError(next),'','The selected catalogue car must remain saveable.');
    assert.deepEqual(plain(document),before);assert.equal(choice.name,name);
    assert.equal(choice.id,'My_Mod_variant_S1');
  }
});
