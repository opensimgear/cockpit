import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';

const {filterGameList,emptyGameListFilters}=(await loadModules(['./games'])).get('./games');
const games=[
  {id:'racer',name:'Racing game',tagIds:['racing'],source:'steam'},
  {id:'sim',name:'Simulator',tagIds:['simulation'],source:'steam',provider:'dcs-bios'},
  {id:'local',name:'Local game',source:'local'},
];
const profiles=[
  {isDefault:true,gameIds:['local']},
  {isDefault:false,gameIds:[]},
  {isDefault:false},
  {isDefault:false,gameIds:['racer','sim']},
  {isDefault:false,gameIds:['sim']},
];
const ids=(selection,matching=profiles,extra={})=>filterGameList(games,{...emptyGameListFilters(),...extra,profiles:selection},matching).map(game=>game.id).join(',');

test('profile presence matches saved game contexts and excludes the hardware default',()=>{
  assert.equal(ids(['yes']),'racer,sim');
  assert.equal(ids(['no']),'local');
  assert.equal(ids([]),'racer,sim,local');
  assert.equal(ids(['yes','no']),'racer,sim,local');
  assert.equal(ids(['yes'],[]),'');
  assert.equal(ids(['no'],[]),'racer,sim,local');
});

test('profile presence combines with other filters and updates with saved profiles',()=>{
  assert.equal(ids(['yes'],profiles,{name:'sim',tags:['simulation'],libraries:['steam'],integrations:['available']}),'sim');
  assert.equal(ids(['no'],profiles,{tags:['racing']}),'');
  assert.equal(ids(['yes'],[{isDefault:false,gameIds:['local']}]),'local');
  const first=emptyGameListFilters(),second=emptyGameListFilters();
  first.profiles.push('yes');assert.equal(second.profiles.length,0);
  assert.equal(filterGameList(games,emptyGameListFilters(),profiles).length,3);
});
