// Explicitly opt-in: interrupt one verified running development app tree while
// its private native worker holds only KeyA, then restart and explicitly recover.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'node:http';
import {spawn} from 'node:child_process';
import {mkdir, readFile, writeFile, rm} from 'node:fs/promises';
import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
const enabled=process.env.OSG_DESKTOP_CRASH_NATIVE==='1'&&process.env.OSG_DESKTOP_NATIVE_INJECT==='1';
const root=resolve('.'),artifacts=resolve('.research/desktop-crash-native');
const allowed=new Set(['companion_status','companion_enable','companion_disable','companion_pair_offer','companion_approve','companion_revoke','companion_assign','companion_publish_begin','companion_publish_chunk','companion_publish_commit','profile_status','profile_resources_list','profile_resources_inspect','profile_editor_save','profile_compile','profile_assignments_get','profile_apply','profile_stop','profile_recover','profile_select','profile_selection_mode','profile_desktop_status','profile_desktop_configure','profile_desktop_capture_application']);

test('actual whole dev app interruption requires explicit owned KeyA recovery',{skip:!enabled,timeout:300_000},async()=>{
 const require=createRequire(import.meta.url),{chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');
 await mkdir(artifacts,{recursive:true});
 const file=resolve(artifacts,'receiver.json'),evidenceFile=resolve(artifacts,'evidence.json');
 for(const path of [file,file.replace(/\.json$/,'.focus'),file.replace(/\.json$/,'.stop'),evidenceFile])await rm(path,{force:true});
 const receiver=spawn(resolve('crates/desktop-output/target/debug/examples/native_receiver.exe'),['--receiver-only',file],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe']});
 let receiverError='',browser,page,server,pythonChild,output='';const commands=[],browsers=new Set();
 receiver.stderr.on('data',chunk=>{receiverError+=chunk.toString();});
 const token=randomUUID();
 async function nativePage(){
  if(page&&!page.isClosed()&&browser?.isConnected())return page;
  if(browser)try{await browser.close();}catch{}
  const deadline=Date.now()+60_000;let error;
  while(Date.now()<deadline){
   try{
    browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222',{timeout:3000});
    browsers.add(browser);
    page=browser.contexts().flatMap(context=>context.pages()).find(value=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(value.url()));
    if(page){await page.waitForFunction(()=>Boolean(window.__TAURI_INTERNALS__?.invoke),undefined,{timeout:5000});return page;}
   }catch(value){error=value;}
   if(browser)try{await browser.close();browsers.delete(browser);}catch{}
   await new Promise(resolve=>setTimeout(resolve,250));
  }
  throw Error('Native CDP restart did not become ready: '+String(error));
 }
 try{
  await new Promise((resolve,reject)=>{let stdout='';const timer=setTimeout(()=>reject(Error('Receiver not ready: '+receiverError)),10_000);receiver.once('error',reject);receiver.stdout.on('data',chunk=>{stdout+=chunk.toString();if(stdout.includes('\n')){try{assert.ok(JSON.parse(stdout.split('\n')[0]).identity);clearTimeout(timer);resolve();}catch(error){clearTimeout(timer);reject(error);}}});});
  await nativePage();
  server=createServer(async(req,res)=>{try{
   if(req.method!=='POST'||req.url!=='/native'||req.headers.authorization!==`Bearer ${token}`)throw Error('Unauthorized bounded acceptance bridge');
   const chunks=[];let bytes=0;for await(const chunk of req){bytes+=chunk.length;if(bytes>2*1024*1024)throw Error('Acceptance request too large');chunks.push(chunk);}
   const request=JSON.parse(Buffer.concat(chunks).toString('utf8'));if(!allowed.has(request.command))throw Error('Command outside crash acceptance');commands.push(request.command);
   const activePage=await nativePage();
   const result=await activePage.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'desktop-crash-native-check'}}),request);
   res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(result));
  }catch(error){page=undefined;res.writeHead(400,{'content-type':'application/json'});res.end(JSON.stringify({ok:false,error:String(error)}));}});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  pythonChild=spawn(process.env.OSG_PYTHON??'python',['scripts/desktop-crash-native.py','--bridge',`http://127.0.0.1:${server.address().port}/native`,'--output',evidenceFile,'--receiver',file,'--interrupt'],{cwd:root,windowsHide:true,timeout:240_000,env:{...process.env,OSG_NATIVE_BRIDGE_TOKEN:token},stdio:['ignore','pipe','pipe']});
  pythonChild.stdout.on('data',chunk=>{output+=chunk.toString();});pythonChild.stderr.on('data',chunk=>{output+=chunk.toString();});
  const code=await new Promise((resolve,reject)=>{pythonChild.once('error',reject);pythonChild.once('close',resolve);});
  await writeFile(resolve(artifacts,'run.log'),output);assert.equal(code,0,output);
  const evidence=JSON.parse(await readFile(evidenceFile,'utf8'));
  assert.ok(evidence.actualNativeCockpit&&evidence.actualWholeAppInterruption&&evidence.injected&&evidence.explicitRecovery);
  assert.equal(evidence.virtualControllers,0);assert.equal(evidence.receiverEvents.length,2);assert.deepEqual(evidence.cleanupErrors,[]);
 }finally{
  await writeFile(resolve(artifacts,'commands.json'),JSON.stringify(commands,null,2)+'\n');
  // An attempted recovery command is not proof that a native hold was cleared.
  // Retain the receive-only target unless persisted evidence proves cleanup.
  let unsafe=commands.includes('profile_apply');
  try{
   const evidence=JSON.parse(await readFile(evidenceFile,'utf8')),journal=evidence.recovery?.journal;
   const recovered=evidence.explicitRecovery===true&&evidence.cleanupErrors?.length===0
    &&evidence.finalStatus?.profileSession?.state==='stopped'&&evidence.finalStatus?.desktop?.enabled===false
    &&journal?.cleanupComplete===true&&journal.uncertain===false&&journal.held?.length===0
    &&journal.possibleHeld?.length===0&&journal.inFlight===null;
   unsafe=unsafe&&!recovered;
  }catch{}
  const cleanup={receiverPid:receiver.pid,receiverRetainedForRootCleanup:unsafe};
  if(unsafe){receiver.stdout.destroy();receiver.stderr.destroy();receiver.unref();}
  if(!unsafe&&receiver.exitCode===null&&receiver.signalCode===null){
   const exited=new Promise(resolve=>receiver.once('close',resolve));await writeFile(file.replace(/\.json$/,'.stop'),'stop');
   await Promise.race([exited,new Promise(resolve=>setTimeout(resolve,2500))]);
   if(receiver.exitCode===null&&receiver.signalCode===null){receiver.kill();await exited;}
  }
  cleanup.receiverExitCode=receiver.exitCode;cleanup.receiverSignalCode=receiver.signalCode;
  await writeFile(resolve(artifacts,'process-cleanup.json'),JSON.stringify(cleanup,null,2)+'\n');
  if(server)await new Promise(resolve=>server.close(resolve));
  await Promise.allSettled([...browsers].map(value=>value.close()));
 }
});
