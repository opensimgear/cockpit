import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-palette','./profile-editor']);
const {profilePaletteCanAdd,profilePlaceComponent}=modules.get('./profile-palette');
const {blankProfile}=modules.get('./profile-editor');
const document=()=>({...blankProfile('11111111-1111-4111-8111-111111111111','Vehicles'),eligibility:{gameIds:['dcs']},resources:[{id:'a-stick',name:'Stick A',kind:'device',required:false,ports:[]},{id:'b-stick',name:'Stick B',kind:'device',required:false,ports:[]}],vehicleTabs:[{id:'a',gameId:'dcs',vehicleId:'F-16C_50',vehicleName:'F-16C',nodeIds:['a-stick']},{id:'b',gameId:'dcs',vehicleId:'FA-18C_hornet',vehicleName:'F/A-18C',nodeIds:['b-stick']}]});
const source={id:'native-stick',name:'Stick',resourceType:'host-controller',attributes:{}};

test('palette duplicate admission belongs to the current tab and preserves physical identity across exclusive vehicles',()=>{
 const value=document(),assignments=[{resourceId:'a-stick',sourceId:'native-stick',ports:[]}],component={kind:'resource',sourceId:'native-stick'};
 assert.equal(profilePaletteCanAdd(component,value,assignments,[source],'a'),false);
 assert.equal(profilePaletteCanAdd(component,value,assignments,[source],'b'),true);
 assert.equal(profilePaletteCanAdd(component,value,assignments,[source]),true);
 assignments.push({resourceId:'b-stick',sourceId:'native-stick',ports:[]});assert.equal(profilePaletteCanAdd(component,value,assignments,[source],'b'),false);
});

test('vehicle placement preserves other tab positions and Base group membership at identical coordinates',()=>{
 const before=document();before.editor.positions=[{nodeId:'a-stick',x:600,y:60},{nodeId:'b-stick',x:20,y:60}];before.editor.groups=[{id:'base',name:'Base group',members:[],bounds:{x:0,y:0,width:440,height:360}}];
 const after=JSON.parse(JSON.stringify(before));after.resources.push({id:'pedals',name:'Pedals',kind:'device',required:false,ports:[]});after.vehicleTabs[0].nodeIds.push('pedals');
 const next=profilePlaceComponent(before,after,'pedals',{x:40,y:60},'a');
 assert.deepEqual(JSON.parse(JSON.stringify(next.editor.groups)),before.editor.groups);assert.deepEqual(JSON.parse(JSON.stringify(next.editor.positions.find(row=>row.nodeId==='b-stick'))),before.editor.positions[1]);assert.deepEqual(JSON.parse(JSON.stringify(next.editor.positions.find(row=>row.nodeId==='pedals'))),{nodeId:'pedals',x:40,y:60});
});
