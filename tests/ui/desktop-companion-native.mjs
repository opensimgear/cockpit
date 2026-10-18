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
 const child=spawn('powershell.exe',['-NoProfile','-NonInteractive','-Command',"$ErrorActionPreference='Stop'; @(Get-CimInstance Win32_Process -Filter \"Name = 'osg-cockpit.exe' OR Name = 'cockpit.exe'\" | Where-Object { $_.CommandLine -like '*--osg-desktop-input-worker*' } | Select-Object ProcessId,ParentProcessId,Name,CommandLine) | ConvertTo-Json -Compress"],{windowsHide:true,stdio:['ignore','pipe','pipe']});
 let output='',error='';child.stdout.on('data',chunk=>{output+=chunk.toString();});child.stderr.on('data',chunk=>{error+=chunk.toString();});
 const code=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',resolve);});assert.equal(code,0,error);
 const workers=JSON.parse(output.trim()||'[]');return Array.isArray(workers)?workers:[workers];
}
test('real native companion input reaches a typed desktop-only ProfileSession',{timeout:180_000},async()=>{
 await mkdir(artifacts,{recursive:true});const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
 const pages=browser.contexts().flatMap(context=>context.pages()),page=pages.find(page=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()))??pages[0];assert.ok(page);
 const token=randomUUID(),commands=[],receiverFile=resolve(artifacts,'receiver.json'),focusReceiverFile=resolve(artifacts,'focus-receiver.json'),receivers=[];let server;
 try{
  if(inject){
   receivers.push(await startReceiver(receiverFile));receivers.push(await startReceiver(focusReceiverFile));
  }
  server=createServer(async(req,res)=>{try{
   if(req.method!=='POST'||req.url!=='/native'||req.headers.authorization!==`Bearer ${token}`)throw Error('Unauthorized acceptance bridge request');
   let bytes=0;const chunks=[];for await(const chunk of req){bytes+=chunk.length;if(bytes>2*1024*1024)throw Error('Acceptance request exceeds its bound');chunks.push(chunk);}
   const request=JSON.parse(Buffer.concat(chunks).toString('utf8'));if(!allowed.has(request.command)||!inject&&live.has(request.command))throw Error('Command is outside this acceptance phase');commands.push(request.command);
   const result=await page.evaluate(request=>{if(!window.__TAURI_INTERNALS__?.invoke)throw Error('Existing page is not native Cockpit');return window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'desktop-companion-native-check'}});},request);
   res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(result));
  }catch(error){res.writeHead(400,{'content-type':'application/json'});res.end(JSON.stringify({ok:false,error:String(error)}));}});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const address=server.address();
  const args=['scripts/desktop-companion-native.py','--bridge',`http://127.0.0.1:${address.port}/native`,'--output',resolve(artifacts,'evidence.json')];if(inject){args.push('--inject','--receiver',receiverFile,'--focus-receiver',focusReceiverFile);if(process.platform==='win32')args.push('--desktop-settings-file',process.env.OSG_DESKTOP_SETTINGS_FILE??resolve(process.env.APPDATA,'OpenSimGear/Cockpit/desktop-input-v1.json'));}
  const child=spawn(python,args,{cwd:root,windowsHide:true,timeout:150_000,env:{...process.env,OSG_NATIVE_BRIDGE_TOKEN:token},stdio:['ignore','pipe','pipe']});
  let output='';child.stdout.on('data',chunk=>{output+=chunk.toString();});child.stderr.on('data',chunk=>{output+=chunk.toString();});
  const code=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',resolve);});await writeFile(resolve(artifacts,'run.log'),output);assert.equal(code,0,output);
  const evidence=JSON.parse(await readFile(resolve(artifacts,'evidence.json'),'utf8'));assert.equal(evidence.actualNativeCockpit,true);assert.equal(evidence.virtualControllers,0);assert.equal(evidence.injected,inject);if(inject){assert.ok(evidence.checks.length>=7);assert.ok(evidence.focusReturn);assert.deepEqual(evidence.cleanupErrors,[]);}
 }finally{
  await writeFile(resolve(artifacts,'commands.json'),JSON.stringify(commands,null,2)+'\n');
  const closed=await Promise.allSettled(receivers.map(closeReceiver));
  const processCleanup={receivers:receivers.map(({child})=>({pid:child.pid,exitCode:child.exitCode,signalCode:child.signalCode})),errors:closed.filter(result=>result.status==='rejected').map(result=>String(result.reason))};
  try{if(inject)processCleanup.activeNativeWorkers=await nativeWorkers();}
  catch(error){processCleanup.errors.push('Inspect native worker cleanup: '+String(error));}
  try{await writeFile(resolve(artifacts,'process-cleanup.json'),JSON.stringify(processCleanup,null,2)+'\n');}
  finally{try{if(server)await new Promise(resolve=>server.close(resolve));}finally{await browser.close();}}
  assert.deepEqual(processCleanup.errors,[],'Receive-only process cleanup failed');if(inject)assert.deepEqual(processCleanup.activeNativeWorkers,[],'Native desktop worker survived profile Stop');
 }
});
