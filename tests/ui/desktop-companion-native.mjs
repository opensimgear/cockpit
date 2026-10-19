// One actual native Cockpit is the host. Python drives real authenticated WSS
// peers; this bounded local bridge forwards only test operations through CDP IPC.
import test from'node:test';import assert from'node:assert/strict';import{createRequire}from'node:module';import{createServer}from'node:http';import{spawn}from'node:child_process';import{mkdir,readFile,writeFile,rm}from'node:fs/promises';import{resolve}from'node:path';import{randomUUID}from'node:crypto';
const require=createRequire(import.meta.url),playwright=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const{chromium}=require(playwright),inject=process.env.OSG_DESKTOP_NATIVE_INJECT==='1';
const root=resolve('.'),artifacts=resolve('.research/desktop-companion-native');
const python=process.env.OSG_PYTHON??(process.platform==='win32'?'python':'python3');
const allowed=new Set(['companion_status','companion_enable','companion_disable','companion_pair_offer','companion_approve','companion_revoke','companion_assign','companion_publish_begin','companion_publish_chunk','companion_publish_commit','profile_status','profile_resources_list','profile_resources_inspect','profile_editor_save','profile_compile','profile_assignments_get','profile_apply','profile_stop','profile_pause','profile_runtime_inspect','profile_desktop_status','profile_desktop_configure','profile_desktop_capture_application','profile_desktop_stop']);
const live=new Set(['profile_apply','profile_stop','profile_pause','profile_desktop_configure','profile_desktop_capture_application','profile_desktop_stop']);
async function startReceiver(file){
 for(const path of [file,file.replace(/\.json$/,'.focus'),file.replace(/\.json$/,'.stop')])await rm(path,{force:true});
 const executable=process.env.OSG_DESKTOP_RECEIVER??resolve('crates/desktop-output/target/debug/examples/native_receiver.exe');
 const child=spawn(executable,['--receiver-only',file],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe']});
 let stderr='';child.stderr.on('data',chunk=>{stderr+=chunk.toString();});
 try{await new Promise((resolve,reject)=>{let output='';const timer=setTimeout(()=>reject(Error('Receiver readiness timed out: '+stderr)),10_000);child.once('error',error=>{clearTimeout(timer);reject(error);});child.once('close',code=>{clearTimeout(timer);reject(Error(`Receiver exited ${code}: ${stderr}`));});child.stdout.on('data',chunk=>{output+=chunk.toString();if(output.includes('\n')){try{const ready=JSON.parse(output.split('\n')[0]);assert.ok(ready.identity);clearTimeout(timer);resolve();}catch(error){clearTimeout(timer);reject(error);}}});});}
 catch(error){child.kill();throw error;}
 return {child,file};
}
async function closeReceiver({child,file}){
 if(child.exitCode!==null||child.signalCode!==null)return;
 const exited=new Promise(resolve=>child.once('close',resolve));await writeFile(file.replace(/\.json$/,'.stop'),'stop');
 await Promise.race([exited,new Promise(resolve=>setTimeout(resolve,2000))]);
 if(child.exitCode===null&&child.signalCode===null){child.kill();await exited;}
 assert.ok(child.exitCode!==null||child.signalCode!==null,'Receive-only process did not exit');
}
async function nativeWorkers(){
 const child=spawn('powershell.exe',['-NoProfile','-NonInteractive','-Command',"$ErrorActionPreference='Stop'; $workers=@(Get-CimInstance Win32_Process -Filter \"Name = 'osg-cockpit.exe' OR Name = 'cockpit.exe'\" | Where-Object {$_.CommandLine -like '*--osg-desktop-input-worker*'} | ForEach-Object {[pscustomobject]@{pid=$_.ProcessId;name=$_.Name}}); ConvertTo-Json -InputObject $workers -Compress"],{windowsHide:true,timeout:15_000,stdio:['ignore','pipe','pipe']});
 let output='',error='';child.stdout.on('data',chunk=>{output+=chunk.toString();});child.stderr.on('data',chunk=>{error+=chunk.toString();});
 const code=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',resolve);});assert.equal(code,0,error);
 const workers=JSON.parse(output);assert.ok(Array.isArray(workers),'Native worker inspection returned no process list');return workers;
}
async function invoke(page,request){
 return page.evaluate(request=>{if(!window.__TAURI_INTERNALS__?.invoke)throw Error('Existing page is not native Cockpit');return window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'desktop-companion-native-check'}});},request);
}
test('real native companion input reaches a typed desktop-only ProfileSession',{timeout:300_000},async()=>{
 await mkdir(artifacts,{recursive:true});const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
 try{
 const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));assert.ok(page,'An already-running native Cockpit page is required');
 const token=randomUUID(),commands=[],paired=new Set(),bridgeRequests=new Set(),receiverFile=resolve(artifacts,'receiver.json'),focusReceiverFile=resolve(artifacts,'focus-receiver.json'),receivers=[];let server,child,desktopBefore,listenerEnabled,settingsSnapshot,bridgeClosing=false;
 const settingsFile=inject&&process.platform==='win32'?(process.env.OSG_DESKTOP_SETTINGS_FILE??resolve(process.env.APPDATA,'OpenSimGear/Cockpit/desktop-input-v1.json')):null;
 if(inject){
  assert.equal(process.platform,'win32');
  const profile=await invoke(page,{command:'profile_status'}),desktop=await invoke(page,{command:'profile_desktop_status'}),companions=await invoke(page,{command:'companion_status'});
  for(const result of [profile,desktop,companions])assert.equal(result.ok,true);
  assert.equal(profile.state.profiles.profileSession.state,'stopped');assert.equal(profile.state.profiles.profileSession.owner,null);
  assert.deepEqual(await nativeWorkers(),[],'Another native input worker is active');
  desktopBefore=desktop.state.desktop;listenerEnabled=companions.state.companions.listener?.enabled??false;
  try{settingsSnapshot=await readFile(settingsFile);}catch(error){if(error.code!=='ENOENT')throw error;}
 }
 try{
  if(inject){
   receivers.push(await startReceiver(receiverFile));receivers.push(await startReceiver(focusReceiverFile));
  }
  server=createServer(async(req,res)=>{try{
   if(bridgeClosing||req.method!=='POST'||req.url!=='/native'||req.headers.authorization!==`Bearer ${token}`)throw Error('Unauthorized or closed acceptance bridge request');
   let bytes=0;const chunks=[];for await(const chunk of req){bytes+=chunk.length;if(bytes>2*1024*1024)throw Error('Acceptance request exceeds its bound');chunks.push(chunk);}
   const request=JSON.parse(Buffer.concat(chunks).toString('utf8'));if(bridgeClosing||!allowed.has(request.command)||!inject&&live.has(request.command))throw Error('Command is outside this acceptance phase');commands.push(request.command);
   const operation=invoke(page,request);bridgeRequests.add(operation);
   let result;try{result=await operation;}finally{bridgeRequests.delete(operation);}
   if(result.ok&&request.command==='companion_approve')paired.add(request.companion.companionId);
   if(result.ok&&request.command==='companion_revoke')paired.delete(request.companion.companionId);
   res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(result));
  }catch(error){res.writeHead(400,{'content-type':'application/json'});res.end(JSON.stringify({ok:false,error:String(error)}));}});
  server.requestTimeout=30_000;await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const address=server.address();
  const args=['scripts/desktop-companion-native.py','--bridge',`http://127.0.0.1:${address.port}/native`,'--output',resolve(artifacts,'evidence.json')];if(inject)args.push('--inject','--receiver',receiverFile,'--focus-receiver',focusReceiverFile,'--desktop-settings-file',settingsFile);
  child=spawn(python,args,{cwd:root,windowsHide:true,timeout:150_000,env:{...process.env,OSG_NATIVE_BRIDGE_TOKEN:token},stdio:['ignore','pipe','pipe']});
  let output='';child.stdout.on('data',chunk=>{output+=chunk.toString();});child.stderr.on('data',chunk=>{output+=chunk.toString();});
  const code=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',resolve);});await writeFile(resolve(artifacts,'run.log'),output);assert.equal(code,0,output);
  const evidence=JSON.parse(await readFile(resolve(artifacts,'evidence.json'),'utf8'));assert.equal(evidence.actualNativeCockpit,true);assert.equal(evidence.virtualControllers,0);assert.equal(evidence.injected,inject);if(inject){assert.ok(evidence.checks.length>=8);assert.ok(evidence.focusReturn);assert.ok(evidence.macroFocusReturn?.originalGlobalMacroLaterPressObserved);assert.deepEqual(evidence.cleanupErrors,[]);}
 }finally{
  // Independent cleanup still runs if Python times out or never writes evidence.
  bridgeClosing=true;
  if(child&&child.exitCode===null&&child.signalCode===null){const exited=new Promise(resolve=>child.once('close',resolve));child.kill();await exited;}
  const processCleanup={errors:[]};
  const attempt=async(label,operation)=>{try{return await operation();}catch(error){processCleanup.errors.push(`${label}: ${String(error)}`);}};
  const bridgeDrained=await attempt('Drain earlier native IPC before cleanup',async()=>{
   let timer;try{await Promise.race([Promise.allSettled([...bridgeRequests]),new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error('Earlier native IPC is still pending; preserve recovery context')),30_000);})]);return true;}finally{clearTimeout(timer);}
  })===true;
  let ownershipClear=!inject;
  if(inject){
   const stopConfirmed=await attempt('Stop native profile',async()=>{processCleanup.stop=await invoke(page,{command:'profile_stop'});assert.equal(processCleanup.stop.ok,true);return true;})===true;
   ownershipClear=await attempt('Prove native ownership cleanup before restoring preferences or closing receivers',async()=>{
    assert.equal(stopConfirmed,true,'Native Stop did not succeed; preserve recovery settings and receivers');
    assert.equal(bridgeDrained,true,'Earlier native IPC may still mutate ownership; preserve recovery context');
    const profile=await invoke(page,{command:'profile_status'}),desktop=await invoke(page,{command:'profile_desktop_status'});
    assert.equal(profile.ok,true);assert.equal(desktop.ok,true);
    assert.equal(profile.state.profiles.profileSession.state,'stopped');assert.equal(profile.state.profiles.profileSession.owner,null);
    const runtime=desktop.state.desktop.runtime;
    if(runtime==null)assert.ok(!commands.includes('profile_apply'),'No native ledger proves cleanup after attempted Apply');
    else{
     const backend=runtime.backend;assert.equal(runtime.state,'stopped');
     for(const field of ['held','possibleHeld','owners','runs'])assert.deepEqual(backend[field],[]);
     assert.equal(backend.cleanupComplete,true);assert.equal(backend.uncertain,false);assert.equal(backend.inFlight,null);
    }
    const workers=await nativeWorkers();assert.deepEqual(workers,[],'A native input worker still owns the session');
    processCleanup.ownershipProof={profileSession:profile.state.profiles.profileSession,desktop:desktop.state.desktop,nativeInputWorkers:workers};return true;
   })===true;
   processCleanup.ownershipCleanupConfirmed=ownershipClear;processCleanup.needsRootCleanup=!ownershipClear;
   if(ownershipClear){
    await attempt('Restore desktop scope/enablement',async()=>{const result=await invoke(page,{command:'profile_desktop_configure',enabled:desktopBefore.enabled,scope:desktopBefore.scope});assert.equal(result.ok,true);});
    await attempt('Restore exact desktop preference bytes',async()=>{if(settingsSnapshot===undefined)await rm(settingsFile,{force:true});else await writeFile(settingsFile,settingsSnapshot);});
    for(const companionId of paired)await attempt('Revoke exact test WSS identity',async()=>{const result=await invoke(page,{command:'companion_revoke',companion:{companionId}});assert.equal(result.ok,true);});
    if(!listenerEnabled)await attempt('Restore disabled listener',async()=>{const result=await invoke(page,{command:'companion_disable'});assert.equal(result.ok,true);});
    await attempt('Verify restored preferences/listener and stopped profile',async()=>{
     const desktop=await invoke(page,{command:'profile_desktop_status'}),profile=await invoke(page,{command:'profile_status'}),companions=await invoke(page,{command:'companion_status'});
     for(const result of [desktop,profile,companions])assert.equal(result.ok,true);
     assert.equal(profile.state.profiles.profileSession.state,'stopped');assert.equal(profile.state.profiles.profileSession.owner,null);
     assert.equal(desktop.state.desktop.enabled,desktopBefore.enabled);assert.deepEqual(desktop.state.desktop.scope,desktopBefore.scope);assert.deepEqual(desktop.state.desktop.application,desktopBefore.application);
     assert.equal(companions.state.companions.listener?.enabled??false,listenerEnabled);
     if(settingsSnapshot===undefined)await assert.rejects(readFile(settingsFile),{code:'ENOENT'});else assert.deepEqual(await readFile(settingsFile),settingsSnapshot);
     processCleanup.restored={desktop:desktop.state.desktop,profileSession:profile.state.profiles.profileSession,listener:companions.state.companions.listener};
    });
   }
  }
  if(ownershipClear){const closed=await Promise.allSettled(receivers.map(closeReceiver));processCleanup.errors.push(...closed.filter(result=>result.status==='rejected').map(result=>'Close receive-only helper: '+String(result.reason)));}
  else for(const {child}of receivers)if(child.exitCode===null&&child.signalCode===null){child.stdout.destroy();child.stderr.destroy();child.unref();}
  processCleanup.receivers=receivers.map(({child})=>({pid:child.pid,exitCode:child.exitCode,signalCode:child.signalCode,retainedForRootCleanup:!ownershipClear}));
  if(inject)await attempt('Inspect native worker cleanup',async()=>{processCleanup.activeNativeWorkers=await nativeWorkers();if(ownershipClear)assert.deepEqual(processCleanup.activeNativeWorkers,[],'Native desktop worker survived Stop');});
  try{await writeFile(resolve(artifacts,'commands.json'),JSON.stringify(commands,null,2)+'\n');await writeFile(resolve(artifacts,'process-cleanup.json'),JSON.stringify(processCleanup,null,2)+'\n');}
  finally{if(server)await new Promise(resolve=>server.close(resolve));}
  assert.deepEqual(processCleanup.errors,[],'Native companion cleanup failed; inspect process-cleanup.json');
 }
 }finally{await browser.close();} // Disconnect CDP; keep the native application open.
});
