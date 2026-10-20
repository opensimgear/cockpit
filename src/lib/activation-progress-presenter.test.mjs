import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';
const {ActivationProgressPresenter,activationWindowProgress}=(await loadModules(['./activation-progress-presenter'])).get('./activation-progress-presenter');
const progress=(id='first')=>({activationId:id,profileId:'profile',profileName:'Rig',phase:'activating',message:'Configuring profile mappings',step:'configuring-mappings',canCancel:true,cancelRequested:false,companions:[],outcome:null,error:null});
test('only current companion work supplies asset byte status; later mappings/hiding/launching replace it',()=>{
 const value=progress();value.companions=[{name:'Screen',operation:{phase:'downloading',completedBytes:1024,totalBytes:4096}}];
 value.step='companion-baseline';assert.match(activationWindowProgress(value).message,/Screen: Downloading assets.*25%.*1 \/ 4 KiB/);
 for(const [step,message] of [['configuring-mappings','Configuring mappings'],['hiding-peripherals','Hiding peripherals'],['launching','Launching DCS World…']]){Object.assign(value,{step,message,phase:step==='launching'?'launching':'activating'});const payload=activationWindowProgress(value);assert.equal(payload.message,message);assert.equal(payload.phase,value.phase);}
 Object.assign(value,{phase:'cancelling',message:'Restoring safe outputs',step:'companion-baseline',cancelRequested:true});assert.equal(activationWindowProgress(value).message,'Restoring safe outputs');assert.equal(activationWindowProgress(value).canCancel,false);
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
