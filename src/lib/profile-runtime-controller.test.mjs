import assert from'node:assert/strict';import test from'node:test';import{loadModules}from'./test-module-loader.mjs';
const modules=await loadModules(['./profiles','./profile-runtime','./profile-runtime-controller']);const{ProfileRuntimeController}=modules.get('./profile-runtime-controller');
const id='11111111-1111-4111-8111-111111111111',run='22222222-2222-4222-8222-222222222222',other='33333333-3333-4333-8333-333333333333',hash=`sha256:${'1'.repeat(64)}`;
const stopped=()=>({state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null});const running=()=>({...stopped(),state:'running',appliedSnapshot:{profileId:id,revision:3,semanticHash:hash},owner:{session:run,generation:1,profileId:id,profileRevision:3,gameId:null}});
const editor=()=>({id,revision:3,baseRevision:3,dirty:false,readOnly:false});const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};};
function harness(){const calls=[],client={async status(){calls.push(['status']);return stopped();},async apply(id,revision,game,expectedOwner){calls.push(['apply',id,revision,game,expectedOwner]);return running();},async stop(){calls.push(['stop']);return stopped();},async recover(){calls.push(['recover']);return stopped();}};return{client,calls,controller:new ProfileRuntimeController(client)};}

test('construction, observation and disposal cannot select, apply or stop a native run',async()=>{
  const h=harness();assert.deepEqual(h.calls,[]);const observed=[];const unsubscribe=h.controller.subscribe(value=>observed.push(value));assert.equal(h.controller.state.status,null);await h.controller.refresh();assert.deepEqual(h.calls,[['status']]);assert.equal(h.controller.state.status.state,'stopped');assert.equal(h.controller.state.stale,false);assert.ok(observed.length>=3);unsubscribe();h.controller.dispose();assert.equal(await h.controller.refresh(),false);assert.deepEqual(h.calls,[['status']]);
});
test('activation admits only an exact saved editor identity/revision and captures independent arguments with an inactive-owner guard',async()=>{
  const h=harness();for(const patch of [{dirty:true},{readOnly:true},{baseRevision:null},{revision:4}])assert.equal(await h.controller.apply({...editor(),...patch}),false);assert.deepEqual(h.calls,[]);assert.match(h.controller.state.error,/Save your changes before activating/);
  const pending=deferred(),target=editor();h.client.apply=async(...args)=>{h.calls.push(['apply',...args]);return pending.promise;};const applying=h.controller.apply(target,'dcs');target.id=run;target.revision=8;assert.equal(await h.controller.apply(editor()),false);assert.deepEqual(h.calls,[['apply',id,3,'dcs',null]]);const native=running();pending.resolve(native);assert.equal(await applying,true);native.owner.generation=9;assert.equal(h.controller.state.status.owner.generation,1);assert.ok(Object.isFrozen(h.controller.state.status.owner));
});

test('choosing another profile captures a passive switch confirmation; Cancel never deactivates or activates anything',async()=>{
  const h=harness(),native=running();h.client.status=async()=>{h.calls.push(['status']);return native;};await h.controller.refresh();
  const target={...editor(),id:other};assert.equal(await h.controller.apply(target,'dcs'),false);target.id=run;target.revision=20;native.owner.generation=90;
  const confirmation=h.controller.state.activationConfirmation;assert.equal(h.controller.state.pending,null);assert.equal(confirmation.target.id,other);assert.equal(confirmation.target.revision,3);assert.equal(confirmation.gameId,'dcs');assert.equal(confirmation.previousOwner.generation,1);assert.ok(Object.isFrozen(confirmation));assert.ok(Object.isFrozen(confirmation.target));assert.ok(Object.isFrozen(confirmation.previousOwner));assert.deepEqual(h.calls,[['status']]);
  h.controller.cancelActivation();assert.equal(h.controller.state.activationConfirmation,null);assert.equal(await h.controller.confirmActivation(),false);assert.equal(h.controller.state.status.owner.profileId,id);assert.deepEqual(h.calls,[['status']]);
});

test('confirmed switching sends the exact captured owner once and relies on native replacement without a separate deactivation',async()=>{
  const h=harness(),native=running(),pending=deferred();h.client.status=async()=>native;await h.controller.refresh();
  assert.equal(await h.controller.apply({...editor(),id:other},'dcs'),false);const owner=h.controller.state.activationConfirmation.previousOwner;
  h.client.apply=async(...args)=>{h.calls.push(['apply',...args]);return pending.promise;};const confirming=h.controller.confirmActivation();assert.equal(h.controller.state.activationConfirmation,null);assert.equal(h.controller.state.pending,'apply');assert.equal(await h.controller.confirmActivation(),false);assert.deepEqual(h.calls,[['apply',other,3,'dcs',owner]]);
  pending.resolve({...running(),appliedSnapshot:{...running().appliedSnapshot,profileId:other},owner:{...running().owner,profileId:other,generation:2,gameId:'dcs'}});assert.equal(await confirming,true);assert.equal(h.controller.state.status.owner.profileId,other);assert.equal(h.controller.state.status.owner.generation,2);assert.equal(h.calls.some(call=>call[0]==='stop'),false);
});

test('confirmation rejects every changed owner field, disappeared ownership, recovery and stale reads without sending effects',async()=>{
  const replacements=[
    value=>{value.owner.session=other;},value=>{value.owner.generation=2;},value=>{value.owner.profileId=other;value.appliedSnapshot.profileId=other;},value=>{value.owner.profileRevision=4;value.appliedSnapshot.revision=4;},value=>{value.owner.gameId='dcs';},
    ()=>stopped(),()=>({...stopped(),state:'recovery-needed',fault:'Release retained output ownership.'}),
  ];
  for(const replace of replacements){const h=harness();let native=running();h.client.status=async()=>{h.calls.push(['status']);return native;};await h.controller.refresh();await h.controller.apply({...editor(),id:other});const replacement=running();native=replace(replacement)??replacement;await h.controller.refresh();const before=h.calls.length;assert.equal(await h.controller.confirmActivation(),false);assert.equal(h.calls.length,before);assert.equal(h.controller.state.activationConfirmation,null);assert.match(h.controller.state.error,/active profile changed/);assert.equal(h.calls.some(call=>['apply','stop','recover'].includes(call[0])),false);}
  const h=harness();h.client.status=async()=>running();await h.controller.refresh();await h.controller.apply({...editor(),id:other});h.client.status=async()=>{throw new Error('Disconnected.');};await h.controller.refresh();assert.equal(h.controller.state.stale,true);assert.equal(await h.controller.confirmActivation(),false);assert.match(h.controller.state.error,/active profile changed/);assert.deepEqual(h.calls,[]);
});

test('reactivating the current profile uses its exact owner directly and deactivation cancels a pending switch',async()=>{
  const h=harness();h.client.status=async()=>running();await h.controller.refresh();const owner=h.controller.state.status.owner;assert.equal(await h.controller.apply(editor()),true);assert.equal(h.controller.state.activationConfirmation,null);assert.deepEqual(h.calls,[['apply',id,3,undefined,owner]]);
  await h.controller.apply({...editor(),id:other});assert.ok(h.controller.state.activationConfirmation);assert.equal(await h.controller.stop(),true);assert.equal(h.controller.state.activationConfirmation,null);assert.equal(h.controller.state.status.owner,null);assert.equal(await h.controller.confirmActivation(),false);assert.deepEqual(h.calls,[['apply',id,3,undefined,owner],['stop']]);
});
test('Stop and recovery interrupt in-flight Apply and late acknowledgements cannot resurrect the displayed run',async()=>{
  for(const operation of ['stop','recover']){const h=harness(),pending=deferred();h.client.apply=()=>{h.calls.push(['apply']);return pending.promise;};const applying=h.controller.apply(editor());assert.equal(h.controller.state.pending,'apply');assert.equal(await h.controller[operation](),true);assert.equal(h.controller.state.status.owner,null);pending.resolve(running());assert.equal(await applying,false);assert.equal(h.controller.state.status.owner,null);assert.equal(h.controller.state.pending,null);assert.equal(h.calls.filter(call=>call[0]==='apply').length,1);}
});

test('deactivation interrupts a confirmed switch and late success cannot reactivate its replacement',async()=>{
  const h=harness(),pending=deferred();h.client.status=async()=>running();await h.controller.refresh();await h.controller.apply({...editor(),id:other});h.client.apply=()=>{h.calls.push(['apply']);return pending.promise;};const switching=h.controller.confirmActivation();assert.equal(h.controller.state.pending,'apply');assert.equal(await h.controller.stop(),true);pending.resolve({...running(),appliedSnapshot:{...running().appliedSnapshot,profileId:other},owner:{...running().owner,profileId:other,generation:2}});assert.equal(await switching,false);assert.equal(h.controller.state.status.owner,null);assert.equal(h.controller.state.pending,null);assert.deepEqual(h.calls,[['apply'],['stop']]);
});

test('a native owner race during confirmed activation refreshes actual ownership without retrying or deactivating it',async()=>{
  const h=harness();let native=running();h.client.status=async()=>{h.calls.push(['status']);return native;};await h.controller.refresh();await h.controller.apply({...editor(),id:other});const expected=h.controller.state.activationConfirmation.previousOwner;
  h.client.apply=async(...args)=>{h.calls.push(['apply',...args]);native={...running(),owner:{...running().owner,generation:2}};throw new Error('The active profile changed. Refresh and confirm the activation again.');};
  assert.equal(await h.controller.confirmActivation(),false);assert.equal(h.controller.state.activationConfirmation,null);assert.match(h.controller.state.error,/active profile changed/);assert.equal(h.controller.state.stale,false);assert.equal(h.controller.state.status.owner.generation,2);assert.deepEqual(h.calls,[['status'],['apply',other,3,undefined,expected],['status']]);
});
test('late status reads cannot replace an explicit Stop or restart outputs',async()=>{
  const h=harness(),pending=deferred();h.client.status=()=>{h.calls.push(['status']);return pending.promise;};const reading=h.controller.refresh();assert.equal(await h.controller.refresh(),false);await h.controller.stop();pending.resolve(running());assert.equal(await reading,false);assert.equal(h.controller.state.status.state,'stopped');assert.equal(h.controller.state.status.owner,null);assert.equal(h.calls.some(call=>call[0]==='apply'),false);
});
test('native rejection is never retried; a read-only refresh preserves the error and obtains actual remaining ownership',async()=>{
  const h=harness();h.client.apply=async()=>{h.calls.push(['apply']);throw new Error('Required native backend unavailable.');};h.client.status=async()=>{h.calls.push(['status']);return running();};assert.equal(await h.controller.apply(editor()),false);assert.deepEqual(h.calls,[['apply'],['status']]);assert.match(h.controller.state.error,/Required native/);assert.equal(h.controller.state.status.owner.profileRevision,3);h.controller.dismissError();assert.equal(h.controller.state.error,'');h.client.status=async()=>{throw new Error('Service disconnected.');};assert.equal(await h.controller.refresh(),false);assert.equal(h.controller.state.stale,true);assert.match(h.controller.state.statusError,/disconnected/);
});
test('busy emergency commands do not duplicate and disposal ignores late replies without changing native processing',async()=>{
  const h=harness(),pending=deferred();h.client.stop=()=>{h.calls.push(['stop']);return pending.promise;};const stopping=h.controller.stop();assert.equal(await h.controller.stop(),false);assert.equal(await h.controller.refresh(),false);h.controller.dispose();pending.resolve(stopped());assert.equal(await stopping,false);assert.deepEqual(h.calls,[['stop']]);
});
