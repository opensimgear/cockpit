import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-virtual-slots','./profile-graph','./profile-editor']);
const {virtualSlotAssignments,virtualBackendSlots,virtualSlotSummary,virtualSlotLayoutIssue,virtualLayoutForSlot}=modules.get('./profile-virtual-slots');
const plain=value=>JSON.parse(JSON.stringify(value));
const layout=()=>({buttons:['button-1','fire'],axes:[{portId:'steer',range:'unipolar',fallback:12}],hats:['look']});
const slot=(patch={})=>({slot:2,name:'vJoy Device 2',status:'free',buttons:2,axes:[{usage:48,min:0,max:32767}],hats:1,discreteHats:0,forceFeedback:false,...patch});
const settings=()=>({profileId:'test',profileRevision:3,readOnly:false,virtualOutputIds:['controller'],virtualSlots:[{nodeId:'controller',slot:2}]});

test('local slot settings require the exact profile, saved nodes, unique pins and bounded slots',()=>{
 const original=settings(),result=virtualSlotAssignments(original,'test');result.virtualSlots[0].slot=3;
 assert.equal(original.virtualSlots[0].slot,2);
 for(const value of [null,[],{...settings(),profileId:'other'},{...settings(),profileRevision:0},{...settings(),readOnly:0},
  {...settings(),virtualOutputIds:null},{...settings(),virtualOutputIds:Array(17).fill('controller')},{...settings(),virtualOutputIds:[3]},
  {...settings(),virtualSlots:null},{...settings(),virtualSlots:Array(17).fill({nodeId:'controller',slot:2})},
  ...[null,{}, {nodeId:'missing',slot:2},{nodeId:3,slot:2},...[0,17,1.5].map(slot=>({nodeId:'controller',slot}))].map(pin=>({...settings(),virtualSlots:[pin]})),
  {...settings(),virtualSlots:[{nodeId:'controller',slot:1},{nodeId:'controller',slot:2}]}])assert.throws(()=>virtualSlotAssignments(value,'test'),/settings are unavailable/);
 assert.deepEqual(plain(virtualSlotAssignments({...settings(),virtualSlots:[]},'test').virtualSlots),[]);
 assert.equal(virtualSlotAssignments({...settings(),virtualSlots:[{nodeId:'controller',slot:null}]},'test').virtualSlots[0].slot,null);
});

test('inventory uses observed capabilities, sorts distinct slots, and preserves unavailable layouts',()=>{
 const raw={virtualDevices:[{slot:3,name:'Third',status:'busy',buttons:4,axes:[],continuousHats:0,discreteHats:0,forceFeedback:false},
  null,{slot:0},{slot:3},{slot:1},{slot:2,name:'Second',status:'free',buttons:2,axes:[{usage:48,min:0,max:32767}],continuousHats:1,discreteHats:0,forceFeedback:false}]};
 const slots=virtualBackendSlots(raw);
 assert.deepEqual(plain(slots.map(slot=>slot.slot)),[1,2,3]);assert.equal(slots[0].name,'Slot 1');assert.equal(slots[0].status,'unknown');
 assert.equal(virtualSlotSummary(slots[0]),'Layout unavailable');assert.equal(virtualSlotSummary(slots[1]),'2 buttons · 1 axes · 1 hats');
 for(const value of [null,[],{}, {virtualDevices:1}])assert.throws(()=>virtualBackendSlots(value),/inventory is unavailable/);
 for(const axes of [[{}],Array(17).fill({usage:48,min:0,max:32767}),[{usage:-1,min:0,max:32767}],[{usage:48,min:NaN,max:32767}],[{usage:48,min:0,max:Infinity}]]){
  assert.equal(virtualBackendSlots({virtualDevices:[{slot:1,axes}]})[0].axes,null);
 }
 assert.deepEqual(plain(virtualBackendSlots({virtualDevices:[]})),[]);
});

test('matching requires the complete native layout and supported ranges, hats and feedback',()=>{
 assert.equal(virtualSlotLayoutIssue(slot(),layout()),'');
 for(const patch of [{buttons:3},{axes:[]},{hats:0}])assert.match(virtualSlotLayoutIssue(slot(patch),layout()),/Match its layout/);
 for(const patch of [{buttons:null},{axes:null},{hats:null},{discreteHats:null},{forceFeedback:null}])assert.match(virtualSlotLayoutIssue(slot(patch),layout()),/unavailable/);
 for(const patch of [{forceFeedback:true},{discreteHats:1},{axes:[{usage:48,min:-1,max:32767}]},{axes:[{usage:48,min:0,max:100}]},{axes:[{usage:99,min:0,max:32767}]},{axes:[{usage:48,min:0,max:32767},{usage:48,min:0,max:32767}]}])assert.match(virtualSlotLayoutIssue(slot(patch),layout()),/unsupported/);
});

test('Use layout keeps existing IDs and axis metadata while adding distinct controls',()=>{
 const before=layout(),expanded=virtualLayoutForSlot(slot({buttons:4,axes:[{usage:48,min:0,max:32767},{usage:49,min:0,max:32767}],hats:2}),before);
 assert.deepEqual(plain(expanded.buttons),['button-1','fire','button-2','button-3']);
 assert.deepEqual(plain(expanded.axes),[{portId:'steer',range:'unipolar',fallback:12},{portId:'axis-1',range:'bipolar',fallback:0}]);
 assert.deepEqual(plain(expanded.hats),['look','hat-1']);assert.deepEqual(before,layout());
 const reduced=virtualLayoutForSlot(slot({buttons:1,axes:[],hats:0}),before);assert.deepEqual(plain(reduced),{buttons:['button-1'],axes:[],hats:[]});
 const collision={buttons:['axis-1','hat-1','button-2'],axes:[],hats:[]};
 const next=virtualLayoutForSlot(slot({buttons:4,axes:[{usage:48,min:0,max:32767}],hats:1}),collision);
 assert.equal(next.axes[0].portId,'axis-2');assert.equal(next.hats[0],'hat-2');assert.equal(next.buttons[3],'button-1');
 for(const patch of [{buttons:null},{axes:null},{hats:null},{buttons:0,axes:[],hats:0},{buttons:128},{forceFeedback:true}])assert.throws(()=>virtualLayoutForSlot(slot(patch),before));
});

test('Use layout cannot remove inputs that still have graph connections',()=>{
 const {blankProfile}=modules.get('./profile-editor'),{graphSetVirtual,graphSetProcessing,graphConnect}=modules.get('./profile-graph');
 let graph={document:blankProfile('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'),assignments:[]};
 graph=graphSetVirtual(graph,{id:'virtual',name:'Virtual',required:false,layout:layout()});
 const contract=graph.document.virtualOutputs[0].ports.find(port=>port.id==='fire').contract;
 graph=graphSetProcessing(graph,{id:'source',name:'Source',operation:{kind:'constant',value:false},inputs:[],outputs:[{id:'out',name:'Out',direction:'output',contract}]});
 graph=graphConnect(graph,{nodeId:'source',portId:'out'},{nodeId:'virtual',portId:'fire'});
 const before=plain(graph),next=virtualLayoutForSlot(slot({buttons:1}),graph.document.virtualOutputs[0].layout);
 assert.throws(()=>graphSetVirtual(graph,{...graph.document.virtualOutputs[0],layout:next}),/port|input/i);
 assert.deepEqual(plain(graph),before);
});
