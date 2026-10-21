import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';
const {ActivationProgressPresenter,ApplicationProfileProgress,activationWindowProgress}=(await loadModules(['./activation-progress-presenter'])).get('./activation-progress-presenter');
const progress=(id='first')=>({activationId:id,profileId:'profile',profileName:'Rig',phase:'activating',message:'Configuring profile mappings',step:'configuring-mappings',canCancel:true,cancelRequested:false,companions:[],outcome:null,error:null});
test('only current companion work supplies asset byte status; later mappings/hiding/launching replace it',()=>{
 const value=progress();value.companions=[{name:'Screen',operation:{phase:'downloading',completedBytes:1024,totalBytes:4096}}];
 value.step='companion-baseline';assert.match(activationWindowProgress(value).message,/Screen: Downloading assets.*25%.*1 \/ 4 KiB/);
 for(const [step,message] of [['configuring-mappings','Configuring mappings'],['hiding-peripherals','Hiding peripherals'],['launching','Launching DCS World…']]){Object.assign(value,{step,message,phase:step==='launching'?'launching':'activating'});const payload=activationWindowProgress(value);assert.equal(payload.message,message);assert.equal(payload.phase,value.phase);}
 Object.assign(value,{phase:'cancelling',message:'Restoring safe outputs',step:'companion-baseline',cancelRequested:true});assert.equal(activationWindowProgress(value).message,'Restoring safe outputs');assert.equal(activationWindowProgress(value).canCancel,false);
});

test('vehicle switching preserves its aircraft context and disables Cancel through real preparation and completion',()=>{
 for(const phase of ['preparing','activating','succeeded','failed']){
  const payload=activationWindowProgress({...progress(),origin:'vehicle-switch',vehicleName:'AH-6J',phase});
  assert.equal(payload.title,'Switching vehicle settings');assert.equal(payload.phase,'switching');
  assert.equal(payload.profileName,'Rig · AH-6J');assert.equal(payload.canCancel,false);
 }
 const payload=activationWindowProgress({...progress(),origin:'vehicle-switch',profileName:'🚁'.repeat(100),vehicleName:'飛'.repeat(200)});
 assert.ok(new TextEncoder().encode(payload.profileName).length<=240);
 assert.match(payload.profileName,/🚁 · 飛/);assert.ok(!payload.profileName.includes('\uFFFD'));
});

test('a short vehicle confirmation cannot cancel or close a newer manual activation',async()=>{
 const calls=[],callbacks=[];
 const presenter=new ActivationProgressPresenter({async show(payload,cancel){calls.push(['show',payload.operationId,payload.phase]);callbacks.push(cancel);return true;},async update(){return true;},async close(id){calls.push(['close',id]);},dispose(){}},async id=>{calls.push(['cancel',id]);return true;});
 await presenter.sync({...progress('switch'),origin:'vehicle-switch',vehicleName:'Hornet',phase:'succeeded',canCancel:false});
 assert.equal(await callbacks[0](),false);
 await presenter.sync(progress('manual'));assert.equal(await callbacks[0](),false);assert.equal(await callbacks[1](),true);
 assert.deepEqual(calls.filter(call=>call[0]==='close'),[['close','switch']]);
 assert.deepEqual(calls.filter(call=>call[0]==='cancel'),[['cancel','manual']]);presenter.dispose();
});
test('one application owner preserves the same native window through game launch and fences a stale Cancel',async()=>{
 const calls=[],callbacks=[];const window={async show(payload,cancel){calls.push(['show',payload.operationId,payload.message]);callbacks.push(cancel);return true;},async update(payload){calls.push(['update',payload.operationId,payload.message]);return true;},async close(id){calls.push(['close',id]);},dispose(){calls.push(['dispose']);}};
 const presenter=new ActivationProgressPresenter(window,async id=>{calls.push(['cancel',id]);return true;});
 await presenter.sync(progress());await presenter.sync({...progress(),phase:'launching',step:'launching',message:'Launching X-Plane 12…'});assert.equal(calls.filter(call=>call[0]==='show').length,1);assert.match(calls[1][2],/Launching X-Plane/);
 await presenter.sync(progress('second'));assert.equal(await callbacks[0](),false);assert.equal(await callbacks[1](),true);assert.deepEqual(calls.filter(call=>call[0]==='cancel'),[['cancel','second']]);await presenter.sync(null);assert.deepEqual(calls.at(-1),['close','second']);presenter.dispose();await presenter.sync(progress());assert.deepEqual(calls.at(-1),['dispose']);
});
test('native window failures expose current compact fallback and completion clears it',async()=>{
 const fallback=[];const presenter=new ActivationProgressPresenter({show:async()=>false,update:async()=>false,close:async()=>{},dispose(){}},async()=>true,value=>fallback.push(value?.message??null));
 await presenter.sync(progress());await presenter.sync({...progress(),message:'Hiding peripherals'});await presenter.sync(null);assert.deepEqual(fallback,['Configuring profile mappings','Hiding peripherals',null]);
});

test('cleanup preserves each native step despite stale companion downloads and cannot be cancelled',()=>{
 for(const phase of ['deactivating','recovering']){
  for(const [step,message] of [['releasing-bindings','Releasing keyboard and mouse bindings'],['removing-virtual-devices','Removing virtual devices'],['unhiding-peripherals','Unhiding peripherals'],['companion-restore','Restoring companion panels']]){
   const payload=activationWindowProgress({...progress(),phase,step,message,canCancel:true,companions:[{name:'Screen',operation:{phase:'downloading',completedBytes:1024,totalBytes:4096}}]});
   assert.equal(payload.phase,phase);assert.equal(payload.message,message);assert.equal(payload.canCancel,false);
  }
 }
});

test('a queued cleanup update blocks a previously displayed Cancel before native update resolves',async()=>{
 let release;const pending=new Promise(resolve=>release=resolve);let callback,cancelled=0;
 const presenter=new ActivationProgressPresenter({show:async(_,cancel)=>{callback=cancel;return true;},update:async()=>pending,close:async()=>{},dispose(){}},async()=>{cancelled++;return true;});
 await presenter.sync(progress());assert.equal(await callback(),true);
 const updating=presenter.sync({...progress(),phase:'recovering',message:'Releasing bindings',canCancel:true});
 assert.equal(await callback(),false);assert.equal(cancelled,1);release(true);await updating;
 assert.equal(await callback(),false);presenter.dispose();
});

test('presenter serializes replacement and completion while native window creation is pending',async()=>{
 let release;const pending=new Promise(resolve=>release=resolve),calls=[];
 const presenter=new ActivationProgressPresenter({show:async payload=>{calls.push(['show',payload.operationId]);if(payload.operationId==='first')await pending;return true;},update:async()=>true,close:async id=>{calls.push(['close',id]);},dispose(){}},async()=>true);
 const opening=presenter.sync(progress()),replacing=presenter.sync(progress('second')),finishing=presenter.sync(null);
 await Promise.resolve();assert.deepEqual(calls,[['show','first']]);release();await Promise.all([opening,replacing,finishing]);
 assert.deepEqual(calls,[['show','first'],['close','first'],['show','second'],['close','second']]);presenter.dispose();
});

function applicationHarness(readExact=async()=>null){
 const calls=[],timers=new Map();let next=0;
 const app=new ApplicationProfileProgress({async sync(value){calls.push(value?{id:value.activationId,phase:value.phase,message:value.message,canCancel:value.canCancel}:null);},dispose(){calls.push('disposed');}},readExact,(callback,delay)=>{const id=++next;timers.set(id,{callback,delay});return id;},id=>timers.delete(id));
 return {app,calls,timers,async finishConfirmation(){for(const [id,timer]of timers){timers.delete(id);assert.equal(timer.delay,600);timer.callback();}await Promise.resolve();}};
}

test('cleanup dispatch opens immediately, survives pre-admission null polls and shows actual cleanup then its result',async()=>{
 const h=applicationHarness(async id=>({...progress(id),phase:'succeeded',message:'Outputs recovered.',canCancel:false}));
 await h.app.cleanup({type:'start',activationId:'cleanup',phase:'recovering'});
 assert.deepEqual(h.calls,[{id:'cleanup',phase:'recovering',message:'Recovering outputs…',canCancel:false}]);
 await h.app.sync(null);await h.app.sync(progress('previous-activation'));assert.equal(h.calls.length,1);
 await h.app.sync({...progress('cleanup'),phase:'recovering',message:'Removing virtual devices',canCancel:false});assert.equal(h.calls.at(-1).message,'Removing virtual devices');
 await h.app.cleanup({type:'finish',activationId:'cleanup',phase:'recovering'});assert.equal(h.calls.at(-1).message,'Outputs recovered.');assert.equal(h.calls.at(-1).phase,'recovering');
 await h.app.sync(null);assert.notEqual(h.calls.at(-1),null);await h.finishConfirmation();assert.equal(h.calls.at(-1),null);h.app.dispose();
});

test('quick deactivation confirms its real result without inventing cleanup steps',async()=>{
 const h=applicationHarness();
 await h.app.cleanup({type:'start',activationId:'cleanup',phase:'deactivating'});await h.app.cleanup({type:'finish',activationId:'cleanup',phase:'deactivating'});
 assert.deepEqual(h.calls.map(call=>call?.message),['Deactivating profile…','Profile deactivated.']);assert.equal(h.timers.size,1);
 await h.finishConfirmation();assert.equal(h.calls.at(-1),null);h.app.dispose();
});

test('failed cleanup reports native recovery details and stale completion cannot dismiss a newer cleanup',async()=>{
 let release;const pending=new Promise(resolve=>release=resolve);
 const h=applicationHarness(async id=>id==='first'?pending:{...progress(id),phase:'failed',message:'Unhiding peripherals failed. Recover outputs to retry.',canCancel:false});
 await h.app.cleanup({type:'start',activationId:'first',phase:'deactivating'});const finishing=h.app.cleanup({type:'finish',activationId:'first',phase:'deactivating'});
 await h.app.cleanup({type:'start',activationId:'second',phase:'recovering'});release({...progress('first'),phase:'succeeded',message:'Profile deactivated.'});await finishing;
 assert.equal(h.calls.at(-1).id,'second');assert.equal(h.timers.size,0);
 await h.app.cleanup({type:'finish',activationId:'second',phase:'recovering',error:'hidden device receipt failed'});assert.match(h.calls.at(-1).message,/Unhiding peripherals failed/);assert.equal(h.calls.at(-1).canCancel,false);
 h.app.dispose();assert.equal(h.timers.size,0);await h.app.sync(progress());assert.equal(h.calls.at(-1),'disposed');
});

test('a newer activation replaces a finished cleanup confirmation without being closed by its timer',async()=>{
 const h=applicationHarness();await h.app.cleanup({type:'start',activationId:'cleanup',phase:'recovering'});await h.app.cleanup({type:'finish',activationId:'cleanup',phase:'recovering'});
 assert.equal(h.timers.size,1);await h.app.sync(progress('new-activation'));assert.equal(h.timers.size,0);assert.equal(h.calls.at(-1).id,'new-activation');
 await h.finishConfirmation();assert.equal(h.calls.at(-1).id,'new-activation');h.app.dispose();
});
