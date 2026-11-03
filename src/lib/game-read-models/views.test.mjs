import test from 'node:test';
import assert from 'node:assert/strict';
import {game,summary,load} from './test-fixture.mjs';

const views=await load('./views');
const revision=`sha256:${'a'.repeat(64)}`;
const fullGame={...game(),vehicleSupport:{supported:true,kind:'aircraft',vehicles:[{id:'f-16',name:'F-16'}]}};

test('game detail readiness requires the selected id and exact issued summary revision',()=>{
  const current={...game(),detailsRevision:revision},detail={status:'ready',value:{gameId:'dcs',scope:revision,value:fullGame}};
  assert.equal(views.gameDetailMatches(current,detail),true);
  assert.equal(views.gameDetailMatches({...current,detailsRevision:`sha256:${'b'.repeat(64)}`},detail),false);
  for(const status of ['connecting','lost','error'])assert.equal(views.gameDetailMatches(current,{status,value:detail.value}),false);
  assert.equal(views.gameDetailMatches({...current,id:'other'},detail),false);
  assert.equal(views.gameDetailMatches({...game(),detailsRevision:undefined},detail),false);
  assert.equal(views.gameDetailMatches(undefined,detail),false);
});

test('selected game retains only a same-id cached detail while its newer scope is loading',()=>{
  const current={...game(),detailsRevision:revision},cached={...fullGame,name:'Cached detail'};
  const stale={status:'lost',value:{gameId:'dcs',scope:`sha256:${'b'.repeat(64)}`,value:cached}};
  assert.equal(views.selectedGameView(current,stale),cached);
  assert.equal(views.selectedGameView(current,{status:'connecting',value:stale.value}),cached);
  assert.equal(views.selectedGameView(current,{status:'ready',value:{...stale.value,gameId:'other'}}),current);
  assert.equal(views.selectedGameView(undefined,stale),undefined);
});

test('a new discovery run withdraws prior entries and observations until its own review arrives',()=>{
  const state={...summary(),discovery:{scanId:'run-old',status:'ready',firstStart:true,libraries:[{id:'steam'}],entries:[{id:'old-entry'}],observations:[{id:'old-observation'}],diagnostics:['old']}};
  const result=views.gamesWithJobs(state,{jobs:[{key:'discovery',run:'run-new',phase:'scanning',message:'Scanning',terminal:false}]});
  assert.notEqual(result,state);assert.equal(result.discovery.scanId,'run-new');assert.equal(result.discovery.status,'scanning');
  assert.equal(result.discovery.firstStart,true);assert.equal(result.discovery.libraries.length,0);assert.equal(result.discovery.entries.length,0);
  assert.equal(result.discovery.observations.length,0);assert.equal(result.discovery.diagnostics.length,0);
});

test('same-run terminal discovery receipt leaves the authoritative ready review untouched',()=>{
  const state={...summary(),discovery:{scanId:'run-current',status:'ready',firstStart:false,libraries:[{id:'steam',status:'complete'}],entries:[{id:'review-entry'}],observations:[{id:'evidence'}],diagnostics:[]}};
  const result=views.gamesWithJobs(state,{jobs:[{key:'discovery',run:'run-current',phase:'complete',message:'Done',terminal:true}]});
  assert.equal(result,state);assert.equal(result.discovery.entries[0].id,'review-entry');
});

test('ready discovery remains actionable while its matching job awaits explicit apply',()=>{
  const state={...summary(),discovery:{scanId:'review',status:'ready',firstStart:false,libraries:[],entries:[{id:'candidate'}],diagnostics:[]}};
  for(const phase of ['ready','scanning','preparing']){
    const result=views.gamesWithJobs(state,{jobs:[{key:'discovery',run:'review',phase,message:'Review discovered installations',terminal:false}]});
    assert.equal(result,state);assert.equal(result.discovery.status,'ready');assert.equal(result.discovery.entries[0].id,'candidate');
  }
  const pending=views.gamesWithJobs(state,{jobs:[{key:'discovery',run:'new-review',phase:'ready',message:'Review is loading',terminal:false}]});
  assert.equal(pending.discovery.status,'scanning');assert.equal(pending.discovery.entries.length,0);
});

test('discoveryImage passes an issued opaque game asset reference through unchanged',async()=>{
  const discovery=await load('../game-discovery'),reference=`game-asset:${'c'.repeat(64)}:1024`;
  assert.equal(discovery.discoveryImage({image:reference}),reference);
  assert.equal(discovery.discoveryImage({image:'https://example.test/untrusted.jpg'}),'');
});
