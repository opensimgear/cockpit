import test from 'node:test';
import assert from 'node:assert/strict';
import {game,summary,harness,frame,change,blobs,deferred,until,models} from './test-fixture.mjs';

test('exact game topics share immutable projections, independent subscribers and final transport disposal',async()=>{
  const h=harness(),a=[],b=[];const one=h.client.observeGamesState(state=>a.push(state)),two=h.client.observeGamesState(state=>b.push(state));assert.equal(h.opens.length,1);
  h.send({value:summary()});await until(()=>a.at(-1).status==='ready');assert.equal(a.at(-1).value,b.at(-1).value);assert.equal(Object.isFrozen(a.at(-1).value.catalogue[0]),true);
  one();one();assert.equal(h.opens[0].signal.aborted,false);two();assert.equal(h.opens[0].signal.aborted,true);
  const stop=h.client.observeGamesState(()=>{});assert.equal(h.opens.length,2);stop();
});

test('new frames replace pending pages atomically and old resolutions cannot overwrite current values',async()=>{
  const f=blobs(),old=f.add({type:'leaf',items:[game('old','Old')]},'array'),latest=f.add({type:'leaf',items:[game('new','New')]},'array'),gate=deferred();let oldSignal;
  const h=harness(async(...args)=>{if(args[0]===old.$gamePage.id){oldSignal=args[2];await gate.promise;}return f.get(...args);}),states=[];
  const off=h.client.observeGamesState(state=>states.push(state));h.send({value:summary()});await until(()=>states.at(-1).status==='ready');const previous=states.at(-1).value;
  h.send({value:{...summary(),catalogue:old}},change(2));await until(()=>oldSignal);assert.equal(states.at(-1).status,'lost');assert.equal(states.at(-1).value,previous);
  h.send({value:{...summary(),catalogue:latest}},change(3));await until(()=>states.at(-1).status==='ready');assert.equal(states.at(-1).value.catalogue[0].id,'new');assert.equal(oldSignal.aborted,true);
  gate.resolve();await new Promise(resolve=>setImmediate(resolve));assert.equal(states.at(-1).revision,'3');off();
});

test('same pending payload deduplicates pages and publishes the newest frame metadata',async()=>{
  const f=blobs(),root=f.add({type:'leaf',items:[game()]},'array'),gate=deferred();let calls=0;
  const h=harness(async(...args)=>{calls++;await gate.promise;return f.get(...args);}),states=[],off=h.client.observeGamesState(state=>states.push(state));
  const payload={value:{catalogue:root}};h.send(payload);await until(()=>calls===1);h.send(payload,change(2));gate.resolve();await until(()=>states.at(-1).status==='ready');assert.equal(calls,1);assert.equal(states.at(-1).revision,'2');off();
});

test('loss preserves cached last value, requires a fresh baseline and rejects retired generation work',async()=>{
  const h=harness(),states=[],off=h.client.observeGamesState(state=>states.push(state));h.send({value:summary()});await until(()=>states.at(-1).status==='ready');const cached=states.at(-1).value;
  h.send(undefined,{kind:'loss',continuity:'lost',generation:'2',revision:'2',sequence:'2',reason:'reset'});assert.equal(states.at(-1).status,'lost');assert.equal(states.at(-1).value,cached);
  h.send({value:summary()},change(3));assert.equal(states.at(-1).status,'lost');
  h.send({value:summary([game('next')])},{generation:'2',revision:'3',sequence:'3'});await until(()=>states.at(-1).status==='ready');assert.equal(states.at(-1).generation,'2');off();
});

test('page failures retain the prior complete value and snapshots can retry the same revision',async()=>{
  const f=blobs(),root=f.add({type:'leaf',items:[game('next')]},'array');let fail=true;
  const h=harness(async(...args)=>{if(fail)throw Error('expired page');return f.get(...args);}),states=[],off=h.client.observeGamesState(state=>states.push(state));
  h.send({value:summary()});await until(()=>states.at(-1).status==='ready');const cached=states.at(-1).value;
  h.send({value:{catalogue:root}},change(2));await until(()=>states.at(-1).status==='error');assert.equal(states.at(-1).value,cached);assert.match(states.at(-1).error,/expired/);
  fail=false;h.send({value:{catalogue:root}},{revision:'2',sequence:'2'});await until(()=>states.at(-1).status==='ready');assert.equal(states.at(-1).value.catalogue[0].id,'next');off();
});

test('details use exact game and scope envelopes, and switching selected topics aborts pending work',async()=>{
  const f=blobs(),gate=deferred(),root=f.add({type:'leaf',entries:Object.entries(game())},'object');let signal;
  const h=harness(async(...args)=>{signal=args[2];await gate.promise;return f.get(...args);}),a=[],b=[];
  const scope=`sha256:${'a'.repeat(64)}`,stop=h.client.observeGameDetails('dcs',state=>a.push(state));h.send({gameId:'dcs',scope,value:root});await until(()=>signal);stop();assert.equal(signal.aborted,true);assert.equal(h.opens[0].signal.aborted,true);
  const next=h.client.observeGameDetails('next',state=>b.push(state));assert.equal(h.opens[1].topic,'games.detail.next');h.send({gameId:'next',scope,value:game('next')});await until(()=>b.at(-1).status==='ready');assert.equal(b.at(-1).value.scope,scope);assert.equal(b.at(-1).value.value.id,'next');gate.resolve();next();
  const invalid=[];h.client.observeGameDetails('../bad',state=>invalid.push(state));assert.equal(invalid.at(-1).status,'error');
});

test('lazy assets share one request, validate image data, and dispose their last page consumer',async()=>{
  const f=blobs(),image=f.add('data:image/jpeg;base64,/9j/','asset'),h=harness(f.get),a=[],b=[];
  const reference=`game-asset:${image.$gamePage.id.slice(7)}:${image.$gamePage.bytes}`,one=h.client.observeGameAsset(reference,state=>a.push(state)),two=h.client.observeGameAsset(reference,state=>b.push(state));
  await until(()=>a.at(-1).status==='ready');assert.equal(a.at(-1).value,'data:image/jpeg;base64,/9j/');assert.equal(f.calls.length,1);assert.equal(b.at(-1).value,a.at(-1).value);assert.equal(h.opens.length,0);one();two();
  const invalid=[];h.client.observeGameAsset('bad',state=>invalid.push(state));assert.equal(invalid.at(-1).status,'error');
});

test('jobs are a bounded strict ordered table and consumers cannot block each other',()=>{
  const h=harness(),states=[];const one=h.client.observeGameJobs(()=>{throw Error('consumer');}),two=h.client.observeGameJobs(state=>states.push(state));
  const jobs=[{key:'discovery',run:'run-1',phase:'scanning',message:'Steam',terminal:false,percent:25}];h.send({jobs});assert.equal(states.at(-1).status,'ready');assert.equal(states.at(-1).value.jobs[0].phase,'scanning');
  h.send({jobs:[{...jobs[0],message:'x'.repeat(1025)}]},change(2));assert.equal(states.at(-1).status,'error');one();two();
  for(const invalid of [[{...jobs[0],percent:101}],[jobs[0],jobs[0]],Array.from({length:17},(_,index)=>({...jobs[0],key:String(index)})),[{...jobs[0],extra:true}]])assert.throws(()=>models.captureGameJobs({jobs:invalid}));
});

test('native empty add receipts accept null while malformed game identities remain rejected',async()=>{
  const h=harness(),states=[],off=h.client.observeGamesState(state=>states.push(state));
  h.send({value:{...summary(),addedGameId:null,error:null}});
  await until(()=>states.at(-1).status==='ready');
  assert.equal(states.at(-1).value.addedGameId,null);
  for(const addedGameId of ['',17,{},'wrong/id'])assert.throws(()=>models.captureGamesState({...summary(),addedGameId}));
  off();
});

test('source, scope and summary fields are validated before partial results become ready',async()=>{
  const h=harness(),states=[],off=h.client.observeGamesState(state=>states.push(state));
  h.send({value:{catalogue:[{...game(),vehicleSupport:{supported:true,kind:'aircraft',vehicles:[]}}]}});await until(()=>states.at(-1).status==='error');
  h.send({value:{catalogue:[{...game(),detailsRevision:`sha256:${'a'.repeat(64)}`,vehicleSupport:{supported:true,kind:'aircraft',vehicleCount:20000}}]}},{revision:'2',sequence:'2'});await until(()=>states.at(-1).status==='ready');
  h.send({value:summary()},change(3,{clockId:'games:22222222-2222-4222-8222-222222222222'}));assert.equal(states.at(-1).status,'error');off();
  assert.throws(()=>models.captureEnvelope(frame({value:summary()},'games.state',{source:'profiles'}),'games.state'));
  assert.throws(()=>models.captureEnvelope(frame({gameId:'wrong',scope:'bad',value:game()},'games.detail.dcs'),'games.detail.dcs'));
});
