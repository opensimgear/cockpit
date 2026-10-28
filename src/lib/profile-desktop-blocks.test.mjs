import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-desktop-blocks','./profiles','./profile-editor','./profile-graph','./profile-palette','./profile-canvas']);
const {desktopBlock,desktopDevices,desktopBlockLabel,desktopOutputsValid,desktopInputLabel,desktopKeycap}=modules.get('./profile-desktop-blocks');
const {DESKTOP_KEYS,desktopActionContract,desktopActionValid}=modules.get('./profile-desktop');
const {graphSetDesktop,graphRemoveNode}=modules.get('./profile-graph');
const {profileDocumentError,parseProfileDocument,exportProfileDocument}=modules.get('./profiles');
const {blankProfile}=modules.get('./profile-editor'),{profilePaletteCanAdd}=modules.get('./profile-palette');
const plain=value=>JSON.parse(JSON.stringify(value));
const draft=()=>({document:blankProfile('11111111-1111-4111-8111-111111111111','Desktop inputs'),assignments:[]});

test('default device blocks expose every supported key, mouse button, pointer axis and scroll direction',()=>{
 const keyboard=desktopBlock('keyboard','keyboard'),mouse=desktopBlock('mouse','mouse');
 assert.deepEqual(plain(keyboard.actions.map(item=>item.action.key)),Array.from(DESKTOP_KEYS,key=>key.usage));
 assert.equal(keyboard.actions.length,105);assert.equal(mouse.actions.length,11);
 assert.deepEqual(plain(mouse.actions.filter(item=>'button' in item.action).map(item=>item.action.button)),['left','right','middle','x1','x2']);
 assert.deepEqual(plain(mouse.actions.filter(item=>item.action.kind==='pointer').map(item=>item.action.axis)),['x','y']);
 assert.deepEqual(plain(mouse.actions.filter(item=>item.action.kind==='wheel').map(item=>[item.action.horizontal,item.action.vertical])),[[0,1],[0,-1],[-1,0],[1,0]]);
 for(const output of [keyboard,mouse]){
  assert.equal(new Set(output.ports.map(port=>port.id)).size,output.actions.length);
  for(const {portId,action} of output.actions){assert.equal(desktopActionValid(action),true);const port=output.ports.find(port=>port.id===portId);assert.equal(port.direction,'input');assert.deepEqual(port.contract,desktopActionContract(action));assert.equal(port.name,desktopInputLabel(action));assert.ok(desktopKeycap(action).length>0);if('key' in action)assert.ok(desktopKeycap(action).length<=7,desktopKeycap(action));}
 }
 assert.equal(desktopInputLabel({kind:'chord',keys:[224,6]}),'Left Ctrl + C');assert.equal(desktopKeycap({kind:'chord',keys:[224,6]}),'Left Ctrl + C');
 assert.equal(desktopBlockLabel({actions:[...keyboard.actions,...mouse.actions]}),'Keyboard and mouse');
 assert.equal(desktopBlockLabel(mouse),'Mouse');assert.equal(desktopBlockLabel(keyboard),'Keyboard');
 keyboard.actions[0].action.key=5;assert.equal(desktopBlock('keyboard','fresh').actions[0].action.key,4);
});

test('separate blocks coexist, roundtrip and retain their own canvas categories and ports',()=>{
 let value=graphSetDesktop(draft(),desktopBlock('keyboard','keyboard'));
 value=graphSetDesktop(value,desktopBlock('mouse','mouse'));
 assert.equal(value.document.desktopOutputs[0].ports[0].name,'A');
 assert.equal(profileDocumentError(value.document),'');assert.deepEqual(plain(parseProfileDocument(exportProfileDocument(value.document))),plain(value.document));
 assert.deepEqual(plain(modules.get('./profile-canvas').canvasNodes(value.document,[],[]).map(node=>node.category)),['Keyboard','Mouse']);
 assert.throws(()=>graphSetDesktop(value,desktopBlock('keyboard','duplicate')),/one active Keyboard/);
 const updated=graphSetDesktop(value,{...plain(value.document.desktopOutputs[1]),scope:{kind:'desktop-wide'}});
 assert.ok(updated.document.desktopOutputs.every(output=>output.scope.kind==='desktop-wide'));
 assert.equal(value.document.desktopOutputs[0].scope.kind,'selected-game');
 assert.deepEqual(plain(graphRemoveNode(updated,'keyboard').document.desktopOutputs.map(output=>output.id)),['mouse']);
 const conflicting=plain(value.document);conflicting.desktopOutputs[1].scope={kind:'desktop-wide'};assert.match(profileDocumentError(conflicting),/shared output scope/);
 assert.equal(desktopOutputsValid([]),true);assert.equal(desktopOutputsValid([...value.document.desktopOutputs,desktopBlock('mouse','third')]),false);
 assert.equal(desktopOutputsValid([{...desktopBlock('keyboard','k'),scope:{kind:'application',applicationReference:'ref'}},{...desktopBlock('mouse','m'),scope:{applicationReference:'ref',kind:'application'}}]),true);
 assert.equal(desktopOutputsValid([{...desktopBlock('keyboard','k'),scope:undefined},{...desktopBlock('mouse','m'),scope:undefined}]),true);
 assert.equal(desktopOutputsValid([{...desktopBlock('keyboard','k'),scope:{kind:'application'}},{...desktopBlock('mouse','m'),scope:{kind:'application'}}]),true);
});

test('palette admission keeps device types independent, including exclusive vehicle tabs and the shared budget',()=>{
 const keyboard={kind:'desktop',device:'keyboard'},mouse={kind:'desktop',device:'mouse'};
 let value=graphSetDesktop(draft(),desktopBlock('keyboard','keyboard'));
 assert.equal(profilePaletteCanAdd(keyboard,value.document,[],[]),false);assert.equal(profilePaletteCanAdd(mouse,value.document,[],[]),true);
 value=graphSetDesktop(value,desktopBlock('mouse','mouse'));assert.equal(profilePaletteCanAdd(mouse,value.document,[],[]),false);
 const vehicle=draft();vehicle.document.eligibility.gameIds=['dcs'];vehicle.document.vehicleTabs=[{id:'a',gameId:'dcs',vehicleId:'one',vehicleName:'One',nodeIds:[]},{id:'b',gameId:'dcs',vehicleId:'two',vehicleName:'Two',nodeIds:[]}];
 const scoped=graphSetDesktop(vehicle,desktopBlock('keyboard','keyboard'),'a');
 assert.equal(profilePaletteCanAdd(keyboard,scoped.document,[],[],'a'),false);assert.equal(profilePaletteCanAdd(keyboard,scoped.document,[],[],'b'),true);assert.equal(profilePaletteCanAdd(keyboard,scoped.document,[],[]),false);assert.equal(profilePaletteCanAdd(mouse,scoped.document,[],[],'a'),true);
 const full=draft();full.document.processingNodes=Array.from({length:152},(_,i)=>({id:`transform-${i}`}));assert.equal(profilePaletteCanAdd(keyboard,full.document,[],[]),false);
 assert.deepEqual(plain(desktopDevices(desktopBlock('mouse','mouse'))),['mouse']);
});
