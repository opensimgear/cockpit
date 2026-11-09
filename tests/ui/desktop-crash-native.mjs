// Explicitly opt-in: interrupt one verified running development app tree while
// its private native worker holds only KeyA, then restart and explicitly recover.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'node:http';
import {spawn} from 'node:child_process';
import {readFile, writeFile, rm, realpath} from 'node:fs/promises';
import {isAbsolute, relative, resolve, sep} from 'node:path';
import {createHash, randomUUID} from 'node:crypto';
import {recoveryRecord} from './sqlite-recovery-record.mjs';
const enabled=process.env.OSG_DESKTOP_CRASH_NATIVE==='1'&&process.env.OSG_DESKTOP_NATIVE_INJECT==='1';
const root=resolve('.');
const allowed=new Set(['companion_status','companion_enable','companion_disable','companion_pair_offer','companion_approve','companion_revoke','companion_assign','companion_unassign','companion_publish_begin','companion_publish_chunk','companion_publish_commit','profile_status','profile_resources_list','profile_resources_inspect','profile_editor_save','profile_compile','profile_apply','profile_stop','profile_recover','profile_delete','profile_desktop_status','profile_desktop_configure','profile_desktop_capture_application']);

async function privatePath(path, parent) {
 assert.ok(path && isAbsolute(path), 'Use an absolute isolated acceptance path');
 const actual=await realpath(path), child=relative(parent,actual);
 assert.ok(child && child!=='..' && !child.startsWith('..'+sep) && !isAbsolute(child), 'Refusing a path outside the isolated acceptance directory');
 return actual;
}

test('actual whole dev app interruption requires explicit owned KeyA recovery',{skip:!enabled,timeout:300_000},async()=>{
 assert.equal(process.platform,'win32');
 const research=await realpath(resolve('.research'));
 const artifacts=await privatePath(process.env.OSG_DESKTOP_CRASH_ARTIFACTS,research);
 const database=await privatePath(process.env.OSG_SQLITE_DATABASE,artifacts);
 await privatePath(process.env.WEBVIEW2_USER_DATA_FOLDER,artifacts);
 const executable=await privatePath(process.env.OSG_DESKTOP_NATIVE_APP,artifacts);
 const executableSha256=createHash('sha256').update(await readFile(executable)).digest('hex');
 const require=createRequire(import.meta.url),{chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');
 const file=resolve(artifacts,'receiver.json'),evidenceFile=resolve(artifacts,'evidence.json');
 for(const path of [file,file.replace(/\.json$/,'.focus'),file.replace(/\.json$/,'.stop'),evidenceFile])await rm(path,{force:true});
 const receiver=spawn(process.env.OSG_DESKTOP_RECEIVER??resolve('crates/desktop-output/target/debug/examples/native_receiver.exe'),['--pointer-receiver-only',file],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe']});
 let receiverError='',browser,page,server,pythonChild,output='';const commands=[],browsers=new Set();
 receiver.stderr.on('data',chunk=>{receiverError+=chunk.toString();});
 const token=randomUUID();
 let recoveryKey;
 async function nativePage(){
  if(page&&!page.isClosed()&&browser?.isConnected())return page;
  if(browser)try{await browser.close();}catch{}
  const deadline=Date.now()+60_000;let error;
  while(Date.now()<deadline){
   try{
    browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222',{timeout:3000});
    browsers.add(browser);
    page=browser.contexts().flatMap(context=>context.pages()).find(value=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(value.url())&&!value.url().includes('profile-activation-window'));
    if(page){await page.waitForFunction(()=>Boolean(window.__TAURI_INTERNALS__?.invoke),undefined,{timeout:5000});return page;}
   }catch(value){error=value;}
   if(browser)try{await browser.close();browsers.delete(browser);}catch{}
   await new Promise(resolve=>setTimeout(resolve,250));
  }
  throw Error('Native CDP restart did not become ready: '+String(error));
 }
 try{
  const target=await new Promise((resolve,reject)=>{let stdout='';const timer=setTimeout(()=>reject(Error('Receiver not ready: '+receiverError)),10_000);receiver.once('error',reject);receiver.stdout.on('data',chunk=>{stdout+=chunk.toString();if(stdout.includes('\n')){try{const value=JSON.parse(stdout.split('\n')[0]);assert.equal(value.identity.pid,receiver.pid);assert.match(value.window,/^[1-9][0-9]*$/);clearTimeout(timer);resolve(value);}catch(error){clearTimeout(timer);reject(error);}}});});
  await nativePage();
  server=createServer(async(req,res)=>{try{
   if(req.method!=='POST'||req.url!=='/native'||req.headers.authorization!==`Bearer ${token}`)throw Error('Unauthorized bounded acceptance bridge');
   const chunks=[];let bytes=0;for await(const chunk of req){bytes+=chunk.length;if(bytes>2*1024*1024)throw Error('Acceptance request too large');chunks.push(chunk);}
   const request=JSON.parse(Buffer.concat(chunks).toString('utf8'));
   let result;
   if(request.command==='crash_recovery_record'){
    assert.ok(recoveryKey && request.key===recoveryKey,'Only the exact test owner journal may be inspected');
    result={ok:true,state:{latency:await recoveryRecord(database,'recovery-desktop',recoveryKey)}};
   }else if(request.command==='crash_storage'){
    const activePage=await nativePage();
    const storage=await activePage.evaluate(()=>window.__TAURI_INTERNALS__.invoke('persistence_status'));
    assert.equal((await realpath(storage.path)).toLowerCase(),database.toLowerCase(),'Refusing an unexpected Cockpit database');
    result={ok:true,state:{latency:storage}};
   }else{
    assert.ok(allowed.has(request.command),'Command outside crash acceptance');
    commands.push(request.command);
    const activePage=await nativePage();
    result=await activePage.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'desktop-crash-native-check'}}),request);
    if(result.ok&&request.command==='profile_apply'){
     const owner=result.state.profiles.profileSession.owner;
     assert.equal(owner.profileId,request.id);
     recoveryKey=`profile-desktop-${owner.session}-${owner.generation}.json`;
    }
   }
   res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(result));
  }catch(error){page=undefined;res.writeHead(400,{'content-type':'application/json'});res.end(JSON.stringify({ok:false,error:String(error)}));}});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  pythonChild=spawn(process.env.OSG_PYTHON??'python',['scripts/desktop-crash-native.py','--bridge',`http://127.0.0.1:${server.address().port}/native`,'--output',evidenceFile,'--receiver',file,'--receiver-window',target.window,'--executable',executable,'--interrupt'],{cwd:root,windowsHide:true,timeout:240_000,env:{...process.env,OSG_NATIVE_BRIDGE_TOKEN:token},stdio:['ignore','pipe','pipe']});
  pythonChild.stdout.on('data',chunk=>{output+=chunk.toString();});pythonChild.stderr.on('data',chunk=>{output+=chunk.toString();});
  const code=await new Promise((resolve,reject)=>{pythonChild.once('error',reject);pythonChild.once('close',resolve);});
  await writeFile(resolve(artifacts,'run.log'),output);assert.equal(code,0,output);
  const evidence=JSON.parse(await readFile(evidenceFile,'utf8'));
  assert.ok(evidence.actualNativeCockpit&&evidence.actualWholeAppInterruption&&evidence.injected&&evidence.explicitRecovery);
  assert.equal(evidence.virtualControllers,0);assert.equal(evidence.receiverEvents.length,2);assert.deepEqual(evidence.cleanupErrors,[]);
  assert.equal(evidence.cleanupProven,true);
  await writeFile(resolve(artifacts,'run-manifest.json'),JSON.stringify({database,executable,executableSha256,receiver:target},null,2)+'\n');
 }finally{
  await writeFile(resolve(artifacts,'commands.json'),JSON.stringify(commands,null,2)+'\n');
  // An attempted recovery command is not proof that a native hold was cleared.
  // Retain the receive-only target unless persisted evidence proves cleanup.
  let unsafe=commands.includes('profile_apply');
  try{
   const evidence=JSON.parse(await readFile(evidenceFile,'utf8'));
   unsafe=unsafe&&evidence.cleanupProven!==true;
  }catch{}
  const cleanup={receiverPid:receiver.pid,receiverRetainedForRootCleanup:unsafe};
  if(unsafe){receiver.stdout.destroy();receiver.stderr.destroy();receiver.unref();}
  if(!unsafe&&receiver.exitCode===null&&receiver.signalCode===null){
   const exited=new Promise(resolve=>receiver.once('close',resolve));await writeFile(file.replace(/\.json$/,'.stop'),'stop');
   await Promise.race([exited,new Promise(resolve=>setTimeout(resolve,2500))]);
   if(receiver.exitCode===null&&receiver.signalCode===null){receiver.kill();await exited;}
  }
  cleanup.receiverExitCode=receiver.exitCode;cleanup.receiverSignalCode=receiver.signalCode;
  cleanup.receiverError=receiverError;
  await writeFile(resolve(artifacts,'process-cleanup.json'),JSON.stringify(cleanup,null,2)+'\n');
  if(server)await new Promise(resolve=>server.close(resolve));
  await Promise.allSettled([...browsers].map(value=>value.close()));
 }
});
