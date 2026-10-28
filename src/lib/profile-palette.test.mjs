import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-palette','./profile-editor']);
const {profilePaletteCanAdd,profilePlaceComponent,profilePaletteComponentValid,profilePalettePanelReason}=modules.get('./profile-palette');
const {blankProfile}=modules.get('./profile-editor');
const document=()=>({...blankProfile('11111111-1111-4111-8111-111111111111','Vehicles'),eligibility:{gameIds:['dcs']},resources:[{id:'a-stick',name:'Stick A',kind:'device',required:false,ports:[]},{id:'b-stick',name:'Stick B',kind:'device',required:false,ports:[]}],vehicleTabs:[{id:'a',gameId:'dcs',vehicleId:'F-16C_50',vehicleName:'F-16C',nodeIds:['a-stick']},{id:'b',gameId:'dcs',vehicleId:'FA-18C_hornet',vehicleName:'F/A-18C',nodeIds:['b-stick']}]});
const source={id:'native-stick',name:'Stick',resourceType:'host-controller',attributes:{}};

test('palette component payloads retain publication identities and reject legacy transformation payloads and injected fields',()=>{
 const hash=`sha256:${'1'.repeat(64)}`;
 for(const value of [{kind:'virtual'},{kind:'desktop'},{kind:'resource',sourceId:'tablet'},{kind:'companion-panel',sourceId:'tablet',publicationHash:hash}])assert.equal(profilePaletteComponentValid(value),true,JSON.stringify(value));
 for(const value of [null,[],{kind:'transformation',preset:'scale'},{kind:'transformation',preset:'arbitrary-script'},{kind:'transformation',preset:'scale',factor:Infinity},{kind:'companion-panel',sourceId:'tablet',publicationHash:'latest'},{kind:'companion-panel',sourceId:'tablet',publicationHash:hash,panelId:'fake'},{kind:'resource',sourceId:'bad\nidentity'},{kind:'desktop',sourceId:'extra'}])assert.equal(profilePaletteComponentValid(value),false,JSON.stringify(value));
});

test('panel admission enforces paired identity, global capture limits and compatible publication targets',()=>{
 const value=document(),paired={...source,id:'tablet',resourceType:'companion',attributes:{componentKind:'paired-companion',companionId:'physical-tablet'}},panel={publicationHash:`sha256:${'1'.repeat(64)}`,compatibility:[{companionId:'physical-tablet',compatible:true}]},component={kind:'companion-panel',sourceId:'tablet',publicationHash:panel.publicationHash};
 assert.equal(profilePaletteCanAdd(component,value,[],[paired],'a'),true);assert.equal(profilePalettePanelReason(panel,paired),'');
 assert.match(profilePalettePanelReason({...panel,compatibility:[{companionId:'other',compatible:true}]},paired),/not compatible/);
 assert.equal(profilePalettePanelReason({...panel,compatibility:[{companionId:'physical-tablet',compatible:false,reason:'Needs a larger viewport.'}]},paired),'Needs a larger viewport.');
 assert.equal(profilePaletteCanAdd({...component,sourceId:source.id},value,[],[source],'a'),false);
 value.companionAssignments=[{resourceId:'a-stick',publicationHash:panel.publicationHash}];assert.equal(profilePaletteCanAdd(component,value,[{resourceId:'a-stick',sourceId:'tablet',ports:[]}],[paired],'b'),false,'A captured physical companion cannot be captured in a second exclusive tab.');
 value.companionAssignments=Array.from({length:8},(_,i)=>({resourceId:`panel-${i}`,publicationHash:panel.publicationHash}));assert.equal(profilePaletteCanAdd(component,value,[],[paired],'a'),false);
});

test('legacy transformation payloads are rejected by palette admission',()=>{
 const value=document();assert.equal(profilePaletteCanAdd({kind:'transformation',preset:'scale'},value,[],[],'a'),false);
});

test('palette duplicate admission preserves physical identity across the shared canvas',()=>{
 const value=document(),assignments=[{resourceId:'a-stick',sourceId:'native-stick',ports:[]}],component={kind:'resource',sourceId:'native-stick'};
 assert.equal(profilePaletteCanAdd(component,value,assignments,[source],'a'),false);
 assert.equal(profilePaletteCanAdd(component,value,assignments,[source],'b'),false);
 assert.equal(profilePaletteCanAdd(component,value,assignments,[source]),false);
 assignments.push({resourceId:'b-stick',sourceId:'native-stick',ports:[]});assert.equal(profilePaletteCanAdd(component,value,assignments,[source],'b'),false);
});

test('integration identities and admission enforce the selected context, unique scopes and graph limits',()=>{
 const value=document(),component={kind:'integration',gameId:'dcs',vehicleId:'A-10C',vehicleName:'A-10C'};
 for(const vehicleId of ['',component.vehicleId])assert.equal(profilePaletteComponentValid({...component,vehicleId}),true);
 for(const invalid of [{...component,extra:true},{...component,gameId:'bad game'},{...component,vehicleId:' bad'},{...component,vehicleId:'a'.repeat(257)},{...component,vehicleName:''},{...component,vehicleName:'bad\nname'}])assert.equal(profilePaletteComponentValid(invalid),false);
 assert.equal(profilePaletteCanAdd(component,value,[],[]),true);
 assert.equal(profilePaletteCanAdd({...component,gameId:'other'},value,[],[]),false);
 assert.equal(profilePaletteCanAdd({...component,vehicleId:'F-16C_50'},value,[],[]),true);
 assert.equal(profilePaletteCanAdd({...component,vehicleId:''},value,[],[]),true);
 value.resources.push({id:'common',kind:'game'});
 assert.equal(profilePaletteCanAdd({...component,vehicleId:''},value,[],[]),false);
 value.vehicleTabs=Array.from({length:32},(_,index)=>({id:`scope-${index}`,gameId:'dcs',vehicleId:`vehicle-${index}`,nodeIds:[]}));
 assert.equal(profilePaletteCanAdd(component,value,[],[]),false);
 value.vehicleTabs=[];value.resources=Array.from({length:64},(_,index)=>({id:`resource-${index}`,kind:'device'}));
 assert.equal(profilePaletteCanAdd(component,value,[],[]),false);
 value.resources=[];value.eligibility.gameIds=[];assert.equal(profilePaletteCanAdd(component,value,[],[]),false);
});

test('a removed vehicle integration can reuse its scope even at the scope limit',()=>{
 const value=document(),component={kind:'integration',gameId:'dcs',vehicleId:'F-16C_50',vehicleName:'Viper'};
 value.vehicleTabs.push(...Array.from({length:30},(_,index)=>({id:`scope-${index}`,gameId:'dcs',vehicleId:`vehicle-${index}`,vehicleName:`Vehicle ${index}`,nodeIds:[]})));
 const before=JSON.stringify(value.vehicleTabs);
 assert.equal(profilePaletteCanAdd(component,value,[],[]),true);
 value.resources.push({id:'viper-game',kind:'game'});value.vehicleTabs[0].nodeIds.push('viper-game');
 assert.equal(profilePaletteCanAdd(component,value,[],[]),false);
 value.resources=value.resources.filter(resource=>resource.id!=='viper-game');value.vehicleTabs[0].nodeIds=value.vehicleTabs[0].nodeIds.filter(id=>id!=='viper-game');
 assert.equal(profilePaletteCanAdd(component,value,[],[]),true);
 assert.equal(JSON.stringify(value.vehicleTabs),before,'Admission and removal preserve the existing scope identity.');
 assert.equal(profilePaletteCanAdd({...component,vehicleId:'new-vehicle'},value,[],[]),false);
 value.resources=Array.from({length:64},(_,index)=>({id:`resource-${index}`,kind:'device'}));
 assert.equal(profilePaletteCanAdd(component,value,[],[]),false,'Reusing a scope still respects the resource limit.');
});

test('local output admission preserves controller and desktop budgets',()=>{
 const value=document();
 assert.equal(profilePaletteCanAdd({kind:'virtual'},value,[],[]),true);
 value.virtualOutputs=Array.from({length:8},(_,index)=>({id:`virtual-${index}`}));
 assert.equal(profilePaletteCanAdd({kind:'virtual'},value,[],[]),false);
 assert.equal(profilePaletteCanAdd({kind:'desktop',device:'mouse'},value,[],[]),true);
 value.desktopOutputs=[{id:'mouse',actions:[{portId:'left',action:{kind:'mouse-button',button:'left'}}]}];
 assert.equal(profilePaletteCanAdd({kind:'desktop',device:'mouse'},value,[],[]),false);
 value.processingNodes=Array.from({length:256},(_,index)=>({id:`process-${index}`}));
 assert.equal(profilePaletteCanAdd({kind:'desktop'},value,[],[]),false);
});

test('vehicle placement preserves other tab positions and Base group membership at identical coordinates',()=>{
 const before=document();before.editor.positions=[{nodeId:'a-stick',x:600,y:60},{nodeId:'b-stick',x:20,y:60}];before.editor.groups=[{id:'base',name:'Base group',members:[],bounds:{x:0,y:0,width:440,height:360}}];
 const after=JSON.parse(JSON.stringify(before));after.resources.push({id:'pedals',name:'Pedals',kind:'device',required:false,ports:[]});after.vehicleTabs[0].nodeIds.push('pedals');
 const next=profilePlaceComponent(before,after,'pedals',{x:40,y:60},'a');
 assert.deepEqual(JSON.parse(JSON.stringify(next.editor.groups)),before.editor.groups);assert.deepEqual(JSON.parse(JSON.stringify(next.editor.positions.find(row=>row.nodeId==='b-stick'))),before.editor.positions[1]);assert.deepEqual(JSON.parse(JSON.stringify(next.editor.positions.find(row=>row.nodeId==='pedals'))),{nodeId:'pedals',x:40,y:60});
});
