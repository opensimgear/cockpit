import test from 'node:test';
import assert from 'node:assert/strict';
import {panelDestinationWarnings} from './companion-panel-resolution.ts';
const page={id:'main',title:'Main',width:800,height:480};
const client={companionId:'win',name:'WIN Companion',connected:true,displayReports:[{displayWidth:2184,displayHeight:1311}]};
const state={host:{clients:[client]}};
test('new drafts warn before compiling or assigning a publication and clear as page size increases',()=>{
  const warnings=panelDestinationWarnings(page,['win'],state);
  assert.equal(warnings.length,1);assert.match(warnings[0].message,/WIN Companion:.*800 × 480.*2184 × 1311/);
  assert.deepEqual(panelDestinationWarnings({...page,width:2400,height:1440},['win'],state),[]);
  assert.deepEqual(panelDestinationWarnings({...page,width:2400},['win'],state).map(w=>w.companionId),['win']);
});
test('only selected connected measured destinations warn; invalid and missing reports stay quiet',()=>{
  for(const value of [undefined,page]){
    assert.deepEqual(panelDestinationWarnings(value,[],state),[]);
    assert.deepEqual(panelDestinationWarnings(value,['win'],{host:{clients:[{...client,connected:false}]}}),[]);
  }
  for(const displayReports of [undefined,[],[null], [{displayWidth:Infinity,displayHeight:1311}],[{displayWidth:2184,displayHeight:NaN}],[{displayWidth:70000,displayHeight:1311}],[{displayWidth:2184,displayHeight:70000}]])assert.deepEqual(panelDestinationWarnings(page,['win'],{host:{clients:[{...client,displayReports}]}}),[]);
  assert.deepEqual(panelDestinationWarnings(page,['win'],{}),[]);
  assert.deepEqual(panelDestinationWarnings(undefined,['win'],state),[]);
  assert.match(panelDestinationWarnings(page,['win'],{host:{clients:[{...client,name:undefined}]}})[0].message,/^win:/);
});
