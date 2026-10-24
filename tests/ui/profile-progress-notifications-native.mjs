import {progressSettingsInvoke} from './native-progress-settings.mjs';
// Presentation acceptance against the running native Cockpit. These explicitly
// named display probes author no simulator telemetry or profile ownership.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';

const artifacts=resolve('.research/profile-progress-notifications-native');
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const targets=async()=>(await fetch(`${process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222'}/json/list`,{signal:AbortSignal.timeout(5000)})).json();
async function until(read,label,timeout=10_000){const end=Date.now()+timeout;while(Date.now()<end){const value=await read();if(value)return value;await delay(100);}throw Error(label);}
async function connect(target){
 const socket=new WebSocket(target.webSocketDebuggerUrl),pending=new Map();let serial=0;
 try{await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});}
 catch(error){socket.close();throw error;}
 socket.addEventListener('message',event=>{const value=JSON.parse(event.data),entry=pending.get(value.id);if(!entry)return;clearTimeout(entry.timer);pending.delete(value.id);value.error?entry.reject(Error(value.error.message)):entry.resolve(value.result);});
 socket.addEventListener('close',()=>{for(const entry of pending.values()){clearTimeout(entry.timer);entry.reject(Error('Native Cockpit window closed.'));}pending.clear();});
 const send=(method,params={})=>new Promise((resolve,reject)=>{if(socket.readyState!==1){reject(Error('Native Cockpit window is closed.'));return;}const id=++serial,timer=setTimeout(()=>{pending.delete(id);reject(Error(`${method} timed out`));},10_000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
 return {send,async evaluate(expression){const reply=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(reply.exceptionDetails)throw Error(reply.exceptionDetails.exception?.description??reply.exceptionDetails.text);return reply.result.value;},close(){socket.close();}};
}
function assertGeometry(snapshot,position){
 assert.equal(snapshot.delivery,'window');assert.equal(snapshot.windowFocused,false,'Progress windows must not acquire native keyboard focus');
 const geometry=snapshot.nativeGeometry;assert.ok(geometry,'Native notification geometry is unavailable');
 const {scale,workArea}=geometry;assert.ok(Number.isFinite(scale)&&scale>0);assert.ok(workArea.width>0&&workArea.height>0);
 const width=Math.min(Math.round(420*scale),workArea.width),height=Math.min(Math.round(96*scale),workArea.height),workInset=Math.max(1,Math.round(16*scale));
 const insetX=Math.min(workInset,Math.floor((workArea.width-width)/2)),insetY=Math.min(workInset,Math.floor((workArea.height-height)/2));
 assert.equal(geometry.width,width);assert.equal(geometry.height,height);
 assert.equal(geometry.x,workArea.x+(position.endsWith('right')?workArea.width-width-insetX:insetX));
 assert.equal(geometry.y,workArea.y+(position.startsWith('bottom')?workArea.height-height-insetY:insetY));
 return geometry;
}

test('native progress uses system delivery or fallback, preserves controls and repositions visible windows',{
 skip:process.env.OSG_PROGRESS_NOTIFICATIONS_NATIVE!=='1',timeout:90_000,
},async()=>{
 assert.equal(process.platform,'win32');await mkdir(artifacts,{recursive:true});
 const origin=process.env.OSG_NATIVE_DEV_ORIGIN??'http://127.0.0.1:1420';
 const target=(await targets()).find(target=>target.type==='page'&&(target.url.startsWith(`${origin}/`)||target.url.startsWith('http://tauri.localhost/'))&&!target.url.includes('profile-activation-window'));
 assert.ok(target,'Start the native dev Cockpit with CDP enabled.');
 const main=await connect(target),cleanupErrors=[];
 const invoke=progressSettingsInvoke((command,args={})=>main.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`));
 const evidence={actualNativeCockpit:true,presentationProbes:true,actualSimulatorQualified:false,simulatorTransitionQualified:false,checks:[],cleanupErrors};
 let original,operation,child;
 const cleanup=async(label,action)=>{try{await action();}catch(error){cleanupErrors.push(`${label}: ${error}`);}};
 const screenshot=async name=>{const value=await child.send('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(resolve(artifacts,name),Buffer.from(value.data,'base64'));};
 const childWindow=async()=>connect(await until(async()=>(await targets()).find(target=>target.type==='page'&&target.url.includes('profile-activation-window')),'Native fallback window is unavailable'));
 try{
  original=await invoke('profile_progress_settings_get');evidence.originalSettings=original;
  await until(async()=>{const reply=await invoke('gateway_request',{request:{command:'profile_activation_current',clientId:'notification-delivery-probe'}});assert.equal(reply.ok,true,reply.error);return !reply.state.profileActivation;},'A real profile operation must finish before presentation probes');

  operation=`system-probe-${randomUUID()}`;
  const progress={operationId:operation,title:'Switching vehicle settings',profileName:'Notification delivery check',phase:'switching',message:'Checking operating system notification delivery',details:[],canCancel:false};
  await invoke('profile_progress_settings_save',{settings:{delivery:'system',position:'bottom-right'}});
  await invoke('profile_activation_window_open',{progress});evidence.system=[];
  for(let index=0;index<6;index++){
   const update={...progress,message:`Checking operating system progress update ${index+1}`};
   assert.equal(await invoke('profile_activation_window_update',{progress:update}),true);
   const snapshot=await invoke('profile_activation_window_state');evidence.system.push(snapshot);
   assert.equal(snapshot.progress.operationId,operation);assert.equal(snapshot.progress.message,update.message);assert.ok(['system','window'].includes(snapshot.delivery));
   await delay(250);
  }
  const system=evidence.system.at(-1);
  if(system.delivery==='window'){
   assertGeometry(system,'bottom-right');child=await childWindow();
   evidence.systemFallbackView=await child.evaluate('({text:document.body.innerText,documentFocused:document.hasFocus(),width:innerWidth,height:innerHeight})');
   await screenshot('system-fallback.png');child.close();child=undefined;
  }else assert.equal((await targets()).some(target=>target.url.includes('profile-activation-window')),false,'Successful system delivery must not duplicate a Cockpit window');
  evidence.checks.push(`System preference used actual ${system.delivery} delivery after six native updates`);
  assert.equal(await invoke('profile_activation_window_close',{operationId:'stale-probe'}),false);
  assert.equal((await invoke('profile_activation_window_state')).progress.operationId,operation);
  assert.equal(await invoke('profile_activation_window_close',{operationId:operation}),true);operation=undefined;

  await invoke('profile_progress_settings_save',{settings:{delivery:'window',position:'bottom-right'}});
  operation=`window-probe-${randomUUID()}`;
  const manual={...progress,operationId:operation,title:'Profile activation',profileName:'Notification controls check',phase:'preparing',message:'Checking the native Cancel action',canCancel:true};
  await invoke('profile_activation_window_open',{progress:manual});child=await childWindow();
  await until(()=>child.evaluate(`document.body.innerText.includes('Checking the native Cancel action')`),'The native activation probe did not render');
  evidence.window=await invoke('profile_activation_window_state');assertGeometry(evidence.window,'bottom-right');
  const cancel=await child.evaluate(`(()=>{const button=document.querySelector('button');if(!button||button.disabled)return null;const label=button.textContent.trim();button.click();return label;})()`);assert.equal(cancel,'Cancel');
  await until(async()=>(await invoke('profile_activation_window_state')).cancelRequested,'The actual Cancel action did not submit cancellation intent');
  await invoke('profile_activation_window_update',{progress:{...manual,phase:'deactivating',canCancel:false,message:'Checking cleanup progress'},resetCancelRequest:true});
  await until(()=>child.evaluate(`document.querySelectorAll('button').length===0&&document.body.innerText.includes('Checking cleanup progress')`),'Cleanup did not remove the Cancel action');
  evidence.cleanup=await invoke('profile_activation_window_state');assert.equal(evidence.cleanup.cancelRequested,false);assert.equal(evidence.cleanup.progress.canCancel,false);
  evidence.positions=[];
  for(const position of ['bottom-right','bottom-left','top-right','top-left']){
   const settings={delivery:'window',position};assert.deepEqual(await invoke('profile_progress_settings_save',{settings}),settings);assert.deepEqual(await invoke('profile_progress_settings_get'),settings);
   const snapshot=await invoke('profile_activation_window_state'),view=await child.evaluate('({visibility:document.visibilityState,width:innerWidth,height:innerHeight,text:document.body.innerText})');
   evidence.positions.push({position,snapshot,view});
   const geometry=assertGeometry(snapshot,position);assert.equal(view.visibility,'visible');assert.ok(view.text.includes('Checking cleanup progress'));
   assert.ok(Math.abs(view.width-geometry.width/geometry.scale)<=1);assert.ok(Math.abs(view.height-geometry.height/geometry.scale)<=1);
   await screenshot(`cleanup-${position}.png`);
  }
  evidence.checks.push('Configured Cockpit windows support actual Cancel intent, noncancellable cleanup and stale-operation protection without native focus');
  evidence.checks.push('Settings reposition an already visible notification at all four corners with exact physical dimensions');
 }catch(error){evidence.error=String(error);throw error;}
 finally{
  try{
   if(operation)await cleanup('Close display probe',async()=>{assert.equal(await invoke('profile_activation_window_close',{operationId:operation}),true);});
   if(original)await cleanup('Restore notification preferences',async()=>{assert.deepEqual(await invoke('profile_progress_settings_save',{settings:original}),original);assert.deepEqual(await invoke('profile_progress_settings_get'),original);});
   await cleanup('Verify display probe closed',async()=>{evidence.final=await invoke('profile_activation_window_state');assert.equal(evidence.final,null);});
   await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');
  }finally{child?.close();main.close();}
  assert.deepEqual(cleanupErrors,[]);
 }
});
