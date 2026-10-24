import {progressSettingsInvoke} from './native-progress-settings.mjs';
// Actual running Cockpit + verified native process, with authored RREF UDP.
// This exercises production IPC/transports; it does not qualify a real simulator.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createSocket} from 'node:dgram';
import {spawn} from 'node:child_process';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {dirname,resolve} from 'node:path';
import {randomUUID} from 'node:crypto';

const executable=resolve(process.env.OSG_CONTEXT_GAME_EXE??'.research/profile-context-native-game/X-Plane.exe');
const artifacts=resolve('.research/profile-vehicle-switch-native'),gameId='x-plane-12';
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(read,label,timeout=30_000){const end=Date.now()+timeout;let last;while(Date.now()<end){try{last=await read();if(last)return last;}catch(error){last=String(error);}await delay(100);}throw Error(`${label}: ${JSON.stringify(last)}`);}
const targets=async()=>(await fetch(`${process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222'}/json/list`,{signal:AbortSignal.timeout(5000)})).json();
async function connect(target){
 target??=(await targets()).find(target=>target.type==='page'&&/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url)&&!target.url.includes('profile-activation-window'));assert.ok(target,'Start the native dev Cockpit with CDP enabled.');
 const socket=new WebSocket(target.webSocketDebuggerUrl),pending=new Map();let sequence=0;
 await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});
 socket.addEventListener('message',event=>{const message=JSON.parse(event.data),entry=pending.get(message.id);if(!entry)return;clearTimeout(entry.timer);pending.delete(message.id);message.error?entry.reject(Error(message.error.message)):entry.resolve(message.result);});
 socket.addEventListener('close',()=>{for(const entry of pending.values()){clearTimeout(entry.timer);entry.reject(Error('Native Cockpit window closed.'));}pending.clear();});
 const send=(method,params={})=>new Promise((resolve,reject)=>{if(socket.readyState!==1){reject(Error('Native Cockpit window is closed.'));return;}const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error(`CDP ${method} timed out`));},method==='Page.captureScreenshot'?5000:60_000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
 return{send,async evaluate(expression){const reply=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(reply.exceptionDetails)throw Error(reply.exceptionDetails.exception?.description??reply.exceptionDetails.text);return reply.result.value;},close(){socket.close();}};
}
async function emulator(){
 const socket=createSocket('udp4'),clients=new Map(),started=performance.now();let aircraft='Aircraft/Test/Hornet.acf',enabled=true;
 socket.on('message',(bytes,remote)=>{
  if(bytes.length!==413||bytes.subarray(0,5).toString()!=='RREF\0')return;
  const hz=bytes.readInt32LE(5),index=bytes.readInt32LE(9),path=bytes.subarray(13).toString().split('\0')[0],key=`${remote.address}:${remote.port}`;
  let client=clients.get(key);if(!client){client={remote,subscriptions:new Map()};clients.set(key,client);}
  if(hz===0)client.subscriptions.delete(index);else client.subscriptions.set(index,path);
 });
 await new Promise(resolve=>socket.bind(0,'127.0.0.1',resolve));
 const value=path=>{
  const index=/^sim\/aircraft\/view\/acf_relative_path\[(\d+)\]$/.exec(path);if(index)return aircraft.charCodeAt(Number(index[1]))||0;
  return({'sim/version/xplane_internal_version':120000,'sim/time/paused':0,'sim/time/is_in_replay':0,'sim/time/total_running_time_sec':1+(performance.now()-started)/1000,'sim/aircraft/engine/acf_num_engines':1,'sim/flightmodel/position/indicated_airspeed':123.5})[path]??0;
 };
 const timer=setInterval(()=>{if(!enabled)return;for(const {remote,subscriptions} of clients.values()){
  if(!subscriptions.size)continue;const bytes=Buffer.alloc(5+8*subscriptions.size);bytes.write('RREF,');let offset=5;
  for(const [index,path] of subscriptions){bytes.writeInt32LE(index,offset);bytes.writeFloatLE(value(path),offset+4);offset+=8;}
  socket.send(bytes,remote.port,remote.address);
 }},50);
 return{port:socket.address().port,select(id){aircraft=id;},pause(){enabled=false;},resume(){enabled=true;},close(){clearInterval(timer);socket.close();},clients};
}
function identity(id,contract){return{id,name:id,operation:{kind:'identity'},inputs:[{id:'in',name:'Input',direction:'input',contract}],outputs:[{id:'out',name:'Output',direction:'output',contract}]};}

test('native vehicle observation switches scoped settings with positioned progress notifications',{skip:process.env.OSG_VEHICLE_SWITCH_NATIVE!=='1',timeout:180_000},async()=>{
 assert.equal(process.platform,'win32');await readFile(executable);await mkdir(artifacts,{recursive:true});
 const cdp=await connect(),telemetry=await emulator(),commands=[],checks=[],cleanupErrors=[];
 const evidence={actualNativeCockpit:true,verifiedNativeFixture:executable,authoredRrefTelemetry:true,actualSimulatorQualified:false,checks,cleanupErrors};
 const request=async value=>{commands.push(value.command);const reply=await cdp.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'profile-vehicle-switch-native'})}})`);assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;};
 const status=async()=>(await request({command:'profile_status'})).profiles.profileSession;
 const game=async()=>(await request({command:'game_get_state'})).games.catalogue.find(game=>game.id===gameId);
 const datarefs=resolve(dirname(executable),'Resources/plugins/DataRefs.txt'),datarefsBefore=await readFile(datarefs);
 let child,profileId,installationBefore,modeBefore,connectionBefore,selectedBefore,progressSettingsBefore,viewBefore,lastProgressOperation='';
 const cleanup=async(label,action)=>{try{await action();}catch(error){cleanupErrors.push(`${label}: ${error}`);}};
 const invoke=progressSettingsInvoke((command,args={})=>cdp.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`));
 async function notificationPosition(position){
  await until(()=>cdp.evaluate(`(()=>{const button=[...document.querySelectorAll('.sidebar button')].find(button=>button.textContent.trim()==='Settings');if(!button)return false;button.click();return true;})()`),'Settings navigation is unavailable');
  await until(()=>cdp.evaluate(`(()=>{const selects=[...document.querySelectorAll('[aria-label="Progress notifications"] select')];return selects.length===2&&selects.every(select=>!select.disabled);})()`),'Notification settings did not load');
  const deliveryOptions=await cdp.evaluate(`Array.from(document.querySelector('select[aria-label="Progress notification style"]').options,option=>option.value)`);
  assert.deepEqual(deliveryOptions,['system','window']);
  if((await invoke('profile_progress_settings_get')).delivery!=='window'){
   await cdp.evaluate(`(()=>{const select=document.querySelector('select[aria-label="Progress notification style"]');select.value='window';select.dispatchEvent(new Event('change',{bubbles:true}));})()`);
   await until(async()=>!(await cdp.evaluate(`document.querySelector('select[aria-label="Progress notification style"]').disabled`))&&(await invoke('profile_progress_settings_get')).delivery==='window','The Settings UI did not save Cockpit window delivery');
  }
  const options=await cdp.evaluate(`Array.from(document.querySelector('select[aria-label="Progress notification position"]').options,option=>option.value)`);
  assert.deepEqual(options,['bottom-right','bottom-left','top-right','top-left']);
  await cdp.evaluate(`(()=>{const select=document.querySelector('select[aria-label="Progress notification position"]');select.value=${JSON.stringify(position)};select.dispatchEvent(new Event('change',{bubbles:true}));})()`);
  await until(async()=>{
   const ui=await cdp.evaluate(`(()=>{const panel=document.querySelector('[aria-label="Progress notifications"]'),select=panel?.querySelector('select[aria-label="Progress notification position"]');return {value:select?.value,delivery:panel?.querySelector('select[aria-label="Progress notification style"]')?.value,busy:select?.disabled,error:panel?.querySelector('[role="alert"]')?.textContent,message:panel?.querySelector('[role="status"]')?.textContent};})()`);
   if(ui.error)throw Error(ui.error);
   return !ui.busy&&ui.value===position&&ui.delivery==='window'&&ui.message==='Notification settings saved.'&&(await invoke('profile_progress_settings_get')).position===position?ui:false;
  },`The Settings UI did not save ${position}`);
 }
 async function switchWithProgress(label,position,trigger){
  await notificationPosition(position);
  const mainFocusedBefore=await cdp.evaluate('document.hasFocus()');
  let surface,targetId,lastError;
  const watching=until(async()=>{
   try{
    const target=(await targets()).find(target=>target.type==='page'&&target.url.includes('profile-activation-window'));
    if(!target)return false;
    if(target.id!==targetId){surface?.close();surface=await connect(target);targetId=target.id;}
    return await surface.evaluate(`(async()=>{if(!window.__TAURI_INTERNALS__?.invoke)return null;const state=await window.__TAURI_INTERNALS__.invoke('profile_activation_window_state'),main=document.querySelector('main[aria-label="Profile operation progress"]');if(state?.progress.phase!=='switching'||state.progress.operationId===${JSON.stringify(lastProgressOperation)}||!main||main.querySelector('h1')?.textContent!==state.progress.title)return null;return {state,text:main.querySelector('[role="status"]')?.textContent.trim(),title:main.querySelector('h1')?.textContent,profileName:main.querySelector('.profile-name')?.textContent,buttons:main.querySelectorAll('button').length,documentFocused:document.hasFocus(),width:innerWidth,height:innerHeight,x:screenX,y:screenY,outerWidth,outerHeight,devicePixelRatio,available:{left:screen.availLeft,top:screen.availTop,width:screen.availWidth,height:screen.availHeight},overflow:document.documentElement.scrollWidth>innerWidth||document.documentElement.scrollHeight>innerHeight};})()`);
   }catch(error){lastError=String(error);return false;}
  },`No actual ${label} switching progress window was observed`,30_000);
  try{
   await trigger();
   const window=await watching;lastProgressOperation=window.state.progress.operationId;
   const mainFocusedAfter=await cdp.evaluate('document.hasFocus()');
   const observation={label,position,mainFocusedBefore,mainFocusedAfter,window,native:null};(evidence.progressNotifications??=[]).push(observation);
   const image=await surface.send('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(resolve(artifacts,`progress-${label}-${position}.png`),Buffer.from(image.data,'base64'));
   assert.equal(window.title,'Switching vehicle settings');assert.ok(window.text);assert.equal(window.state.progress.canCancel,false);assert.equal(window.buttons,0);assert.equal(window.overflow,false);
   // WebView2 can report internal document focus for a non-activating native
   // window. The native window observation decides OS keyboard focus.
   assert.equal(window.state.windowFocused,false,'A switching notification must not acquire native keyboard focus');
   // Native physical coordinates avoid WebView2's invisible-frame screenX/Y
   // offsets and CSS screen metrics on monitors with scaled Windows displays.
   const geometry=window.state.nativeGeometry;assert.ok(geometry,'The native notification geometry is unavailable');
   const {scale,workArea}=geometry;assert.ok(Number.isFinite(scale)&&scale>0,`Invalid native scale: ${scale}`);assert.ok(workArea&&workArea.width>0&&workArea.height>0,'The native monitor work area is unavailable');
   const width=Math.min(Math.round(420*scale),workArea.width),height=Math.min(Math.round(96*scale),workArea.height),inset=Math.max(1,Math.round(16*scale)),insetX=Math.min(inset,Math.floor((workArea.width-width)/2)),insetY=Math.min(inset,Math.floor((workArea.height-height)/2));
   const x=workArea.x+(position.endsWith('right')?workArea.width-width-insetX:insetX),y=workArea.y+(position.startsWith('bottom')?workArea.height-height-insetY:insetY);
   for(const [name,actual,expected] of [['width',geometry.width,width],['height',geometry.height,height],['x',geometry.x,x],['y',geometry.y,y],['CSS width',window.width,width/scale],['CSS height',window.height,height/scale]])assert.ok(Math.abs(actual-expected)<=1,`${position} ${name}: expected ${expected}, observed ${actual}`);
   const native=(await request({command:'profile_activation_status',activationId:lastProgressOperation})).profileActivation;
   observation.native=native;
   assert.equal(native.activationId,lastProgressOperation);assert.equal(native.origin,'vehicle-switch');assert.equal(native.profileId,profileId);assert.equal(native.canCancel,false);
   assert.equal(native.vehicleName,({'viper':'Viper','stale-base':'Base','hornet-recovered':'Hornet','unconfigured-base':'Base'})[label]);
   assert.equal(window.profileName,`${native.profileName} · ${native.vehicleName}`,'The notification must identify the profile and newly selected tab');
   return observation;
  }catch(error){evidence.progressObservationError={label,error:String(error),lastError};throw error;}
  finally{surface?.close();}
 }
 async function topology(expected){return until(async()=>{
  const state=await status();if(state.owner?.profileId!==profileId||!['running','degraded'].includes(state.state))return false;
  const reply=await request({command:'profile_runtime_topology',owner:state.owner,section:'nodes',offset:0,limit:64});
  const page=reply.profileTopology??reply.profiles?.topology??reply.profileRuntimeTopology;
  assert.ok(page,`Missing topology projection: ${JSON.stringify(reply)}`);
  const ids=page.items.map(node=>node.id).sort();return JSON.stringify(ids)===JSON.stringify([...expected].sort())?{owner:state.owner,nodes:ids}:false;
 },`Active topology did not become ${expected.join(',')}`);}
 async function inventory(){const all=[];let total;do{const page=(await request({command:'profile_resources_list',offset:all.length,limit:128})).profileResources;all.push(...page.resources);total=page.total;}while(all.length<total);return all;}
 async function actualTelemetry(owner,nodeId){return until(async()=>{
  const page=(await request({command:'profile_runtime_inspect',owner,nodeId,offset:0,limit:32})).profileInspection;
  assert.ok(page,'Missing native port inspection');
  (evidence.telemetryObservations??={})[`${owner.generation}/${nodeId}`]=page;
  const value=page.ports.find(port=>port.endpoint.portId==='out')?.sample;
  return value?.status==='known'&&value.value.kind==='scalar'&&value.value.value===123.5?value:false;
 },`Native telemetry did not reach ${nodeId}`);}
 try{
  progressSettingsBefore=await invoke('profile_progress_settings_get');evidence.progressSettingsBefore=progressSettingsBefore;
  viewBefore=await cdp.evaluate(`(()=>{const button=document.querySelector('.sidebar button.current');return button?.title||button?.textContent.trim()||null;})()`);
  const initial=(await request({command:'profile_status'})).profiles;modeBefore=initial.session.selectionMode;selectedBefore=initial.session.selectedProfileId;
  if(['faulted','recovery-needed'].includes(initial.profileSession.state))await request({command:'profile_recover'});
  await request({command:'profile_stop'});await request({command:'profile_selection_mode',mode:'manual'});
  const state=await request({command:'game_get_state'});installationBefore=state.games.manualInstallations?.[gameId]??null;connectionBefore=state.games.catalogue.find(game=>game.id===gameId)?.integrationSetup?.connection;
  const guards=['sim/version/xplane_internal_version','sim/time/paused','sim/time/is_in_replay','sim/time/total_running_time_sec','sim/aircraft/engine/acf_num_engines','sim/aircraft/view/acf_relative_path','sim/flightmodel/position/indicated_airspeed'];
  await writeFile(datarefs,`2 1200 Vehicle acceptance installation\n${guards.map(path=>`${path}\tfloat\tn\tFixture`).join('\n')}\n`);
  await request({command:'game_installation',gameId,executable});
  await request({command:'game_integration_setup',gameId,connection:{host:'127.0.0.1',port:telemetry.port,listenPort:0,updateHz:10,freshnessMs:500}});
  child=spawn(executable,[],{cwd:dirname(executable),windowsHide:true,stdio:'ignore'});child.on('error',error=>{evidence.fixtureError=String(error);});
  const observed=await until(async()=>{const observed=await game();return observed.currentVehicle?.id==='Aircraft/Test/Hornet.acf'&&observed.integrationContext?.processVerified?observed:false;},'Passive observer did not identify a verified vehicle before profile activation');
  assert.equal((await status()).owner,null);evidence.beforeProfile=observed;checks.push('Fresh aircraft identity is observed from real UDP before any applied profile.');
  const source=await until(async()=>(await inventory()).find(resource=>resource.resourceType==='game-integration'&&resource.attributes.gameId===gameId),'Native X-Plane integration inventory is missing');
  const details=(await request({command:'profile_resources_inspect',resourceId:source.id,offset:0,limit:128})).profileResources;
  const observedPort=details.ports.find(port=>port.descriptor.id==='channel:airspeed_kts');assert.ok(observedPort,'Native airspeed descriptor missing');
  const contract=observedPort.descriptor.contract,resourceIds=['base-game','hornet-game','viper-game'];
  const resources=resourceIds.map(id=>({id,name:id,kind:'game',required:false,ports:[{id:'airspeed',name:'Airspeed',direction:'output',contract}]}));
  const assignments=resourceIds.map(resourceId=>({resourceId,sourceId:source.id,ports:[{profilePortId:'airspeed',observedPortId:observedPort.descriptor.id,expectedContractHash:observedPort.contractHash}]}));
  evidence.telemetryDescriptor={resourceId:source.id,port:observedPort};
  const created=(await request({command:'profile_create',name:`Native vehicle switch ${randomUUID()}`})).profiles.document;profileId=created.id;
  const document={...created,eligibility:{gameIds:[gameId]},resources,processingNodes:['base','hornet','viper'].map(id=>identity(`${id}-airspeed`,contract)),bindings:['base','hornet','viper'].map(id=>({id:`${id}-telemetry`,from:{nodeId:`${id}-game`,portId:'airspeed'},to:{nodeId:`${id}-airspeed`,portId:'in'}})),vehicleTabs:[
   {id:'hornet',gameId,vehicleId:'Aircraft/Test/Hornet.acf',vehicleName:'Hornet',nodeIds:['hornet-game','hornet-airspeed']},
   {id:'viper',gameId,vehicleId:'Aircraft/Test/Viper.acf',vehicleName:'Viper',nodeIds:['viper-game','viper-airspeed']},
  ]};
  const compiled=(await request({command:'profile_compile',document,assignments})).profileCompilation;
  assert.equal(compiled.diagnostics.some(diagnostic=>diagnostic.code==='duplicatePhysicalSource'),false,JSON.stringify(compiled.diagnostics));
  assert.notEqual(compiled.readiness,'invalid',JSON.stringify(compiled.diagnostics));evidence.compilation=compiled;checks.push('Native compiler permits shared Base and vehicle game telemetry reads with exact observed port pins.');
  const saved=(await request({command:'profile_editor_save',document,assignments,expectedRevision:created.revision})).profiles.document;
  await request({command:'profile_apply',id:profileId,expectedRevision:saved.revision,gameId});
  const hornet=await topology(['base-game','base-airspeed','hornet-game','hornet-airspeed']);evidence.hornet=hornet;
  evidence.hornet.samples=await Promise.all(['base-airspeed','hornet-airspeed'].map(nodeId=>actualTelemetry(hornet.owner,nodeId)));
  const viperProgress=await switchWithProgress('viper','bottom-right',()=>telemetry.select('Aircraft/Test/Viper.acf'));await until(async()=>(await game()).currentVehicle?.id==='Aircraft/Test/Viper.acf','Observer did not switch to Viper');
  const viper=await topology(['base-game','base-airspeed','viper-game','viper-airspeed']);assert.ok(viper.owner.generation>hornet.owner.generation);evidence.viper=viper;
  evidence.viper.samples=await Promise.all(['base-airspeed','viper-airspeed'].map(nodeId=>actualTelemetry(viper.owner,nodeId)));checks.push('Vehicle switch replaces Hornet nodes with Viper nodes while retaining Base and delivering actual UDP telemetry to both logical blocks.');
  assert.equal((await request({command:'profile_activation_status',activationId:viperProgress.native.activationId})).profileActivation.phase,'succeeded');
  await delay(600);assert.deepEqual((await status()).owner,viper.owner,'Unchanged vehicle must retain ownership without repeated activation.');
  await switchWithProgress('stale-base','bottom-left',()=>telemetry.pause());await until(async()=>(await game()).currentVehicle===null,'Stale telemetry retained the old aircraft');evidence.stale=await topology(['base-game','base-airspeed']);assert.equal(evidence.stale.owner.profileId,profileId);assert.equal(evidence.stale.owner.profileRevision,saved.revision);checks.push('Stale telemetry clears currentVehicle and applies Base alone while retaining the saved profile owner.');
  await switchWithProgress('hornet-recovered','top-right',()=>{telemetry.select('Aircraft/Test/Hornet.acf');telemetry.resume();});await until(async()=>(await game()).currentVehicle?.id==='Aircraft/Test/Hornet.acf','Hornet did not recover after stale telemetry');
  const recovered=await topology(['base-game','base-airspeed','hornet-game','hornet-airspeed']);evidence.recovered=recovered;evidence.recovered.samples=await Promise.all(['base-airspeed','hornet-airspeed'].map(nodeId=>actualTelemetry(recovered.owner,nodeId)));checks.push('Fresh telemetry restores Base plus the matching Hornet tab and both native airspeed samples.');
  await switchWithProgress('unconfigured-base','top-left',()=>telemetry.select('Aircraft/Test/Unconfigured.acf'));await until(async()=>(await game()).currentVehicle?.id==='Aircraft/Test/Unconfigured.acf','Unconfigured aircraft was not observed');
  const unconfigured=await topology(['base-game','base-airspeed']);evidence.unconfigured=unconfigured;evidence.unconfigured.sample=await actualTelemetry(unconfigured.owner,'base-airspeed');checks.push('An unconfigured aircraft applies Base alone and recovers its native telemetry without another tab.');
  checks.push('Cockpit window delivery and all four positions are saved through the actual Settings selectors and applied to non-focusing 420 × 96 native vehicle-switch progress windows, with no cancellation control or layout overflow.');
  await request({command:'profile_stop'});telemetry.select('Aircraft/Test/Hornet.acf');await until(async()=>(await game()).currentVehicle?.id==='Aircraft/Test/Hornet.acf','Passive introspection failed after Stop');await delay(500);assert.equal((await status()).owner,null);checks.push('Stop suppresses vehicle reactivation while passive introspection continues.');
 }catch(error){evidence.failure=String(error);throw error;}
 finally{
  await cleanup('Stop profile',()=>request({command:'profile_stop'}));
  if(child&&child.exitCode===null){child.kill();await new Promise(resolve=>{child.once('exit',resolve);setTimeout(resolve,2000);});}
  telemetry.close();
  if(profileId)await cleanup('Delete fixture profile',async()=>{const doc=(await request({command:'profile_get',id:profileId})).profiles.document;await request({command:'profile_delete',id:profileId,expectedRevision:doc.revision});});
  await cleanup('Restore datarefs',()=>writeFile(datarefs,datarefsBefore));
  if(installationBefore!==undefined)await cleanup('Restore installation',()=>request({command:'game_installation',gameId,executable:installationBefore}));
  if(connectionBefore)await cleanup('Restore provider settings',async()=>{const current=await game();await request({command:'game_integration_settings_save',gameId,expectedSetupRevision:current.integrationSetup.providerSetupRevision,connection:connectionBefore});});
  if(selectedBefore)await cleanup('Restore selected profile',()=>request({command:'profile_select',id:selectedBefore,selectionSource:'manual'}));
  if(modeBefore)await cleanup('Restore selection mode',()=>request({command:'profile_selection_mode',mode:modeBefore}));
  await cleanup('Leave stopped',()=>request({command:'profile_stop'}));
  if(progressSettingsBefore)await cleanup('Restore progress notification preference',async()=>{assert.deepEqual(await invoke('profile_progress_settings_save',{settings:progressSettingsBefore}),progressSettingsBefore);assert.deepEqual(await invoke('profile_progress_settings_get'),progressSettingsBefore);});
  if(viewBefore)await cleanup('Restore navigation',()=>cdp.evaluate(`(()=>{const button=[...document.querySelectorAll('.sidebar button')].find(button=>button.title===${JSON.stringify(viewBefore)}||button.textContent.trim()===${JSON.stringify(viewBefore)});if(!button)throw new Error('The original navigation action is missing.');button.click();})()`));
  evidence.commands=commands;await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');cdp.close();assert.deepEqual(cleanupErrors,[]);
 }
});
