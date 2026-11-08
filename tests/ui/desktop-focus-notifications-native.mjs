// Authored input and cooperative receive-only windows in the running Cockpit.
import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdir, readFile, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

test('native scope notifications release on away and require fresh neutral after return', {
  skip:process.platform!=='win32'||process.env.OSG_DESKTOP_FOCUS_NATIVE!=='1'||process.env.OSG_DESKTOP_NATIVE_INJECT!=='1',
  timeout:60000,
}, async()=>{
  const page=await nativePage(), directory=resolve('.research/reactive/desktop-focus-native',String(Date.now()));
  await mkdir(directory,{recursive:true});
  const receivers=[];
  let saved, preferences, stopped=false;
  const request=async value=>{
    const response=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'desktop-focus-native'})}})`);
    assert.equal(response.ok,true,`${value.command}: ${response.error}`);return response.state;
  };
  const session=async()=>(await request({command:'profile_status'})).profiles.profileSession;
  const desktop=async()=>(await request({command:'profile_desktop_status'})).desktop;
  const launch=async name=>{
    const file=resolve(directory,name+'.json');
    const executable=process.env.OSG_DESKTOP_RECEIVER??resolve('crates/desktop-output/target/debug/examples/native_receiver.exe');
    const child=spawn(executable,['--keyboard-receiver-only',file],{windowsHide:true,stdio:['ignore','pipe','pipe']});
    let errors='';child.stderr.on('data',bytes=>errors+=bytes);child.stdout.resume();
    child.on('error',error=>{errors+=String(error);});
    const read=async()=>{try{return JSON.parse(await readFile(file,'utf8'));}catch{return null;}};
    const receiver={child,file,read,name,errors:()=>errors};receivers.push(receiver);
    try { receiver.initial=await until(read,value=>!!value?.identity,10000); }
    catch(error){ throw new Error(`${name} receiver failed (exit ${child.exitCode}): ${errors}`,{cause:error}); }
    return receiver;
  };
  const focus=async receiver=>{
    const previous=(await receiver.read()).focusRequests;
    await writeFile(receiver.file.replace(/\.json$/,'.focus'),'focus');
    try { return await until(receiver.read,value=>value?.focusRequests>previous&&value.foregroundWindow===value.focusAfter,10000); }
    catch(error){ throw new Error(`${receiver.name} receiver focus failed (exit ${receiver.child.exitCode}): ${receiver.errors()}`,{cause:error}); }
  };
  const keys=value=>value.observations.filter(item=>item.injectedMarker&&item.scanCode===30&&[256,257].includes(item.message));
  try {
    await until(()=>page.evaluate('typeof window.__TAURI_INTERNALS__?.invoke === "function"'));
    assert.equal((await session()).owner,null);stopped=true;
    preferences=await desktop();
    const first=await launch('first'), second=await launch('second');
    await focus(first);
    await until(desktop,value=>value.recentApplication?.pid===first.initial.identity.pid);
    const reference=(await request({command:'profile_desktop_capture_application'})).desktop.capturedApplicationReference;
    assert.ok(reference);
    await request({command:'profile_desktop_configure',enabled:true,scope:preferences.scope});
    saved=(await request({command:'profile_create',name:'Native focus notifications '+randomUUID()})).profiles.document;
    const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
    const port=(id,direction)=>({id,name:id,direction,contract});
    const document={...saved,
      resources:[{id:'debug',name:'Authored input',kind:'debug-controller',required:false,ports:[port('button-1','output')]}],
      desktopOutputs:[{id:'keyboard',name:'Keyboard',required:true,scope:{kind:'application',applicationReference:reference},
        ports:[port('hold','input')],actions:[{portId:'hold',action:{kind:'key-hold',key:4}}]}],
      bindings:[{id:'hold',from:{nodeId:'debug',portId:'button-1'},to:{nodeId:'keyboard',portId:'hold'}}],
    };
    saved=(await request({command:'profile_editor_save',document,assignments:[],expectedRevision:saved.revision})).profiles.document;
    const applied=(await request({command:'profile_apply',id:saved.id,expectedRevision:saved.revision,expectedActiveOwner:null})).profiles.profileSession;
    assert.equal(applied.state,'running',JSON.stringify(applied));stopped=false;
    const streamId=randomUUID();let sequence=0;
    const input=value=>request({command:'profile_debug_controller_input',owner:applied.owner,nodeId:'debug',streamId,
      sequence:String(++sequence),changes:[{portId:'button-1',value}]});
    await input(true);
    await until(first.read,value=>keys(value??{observations:[]}).some(item=>item.message===256));
    const initial=(await desktop()).runtime;
    assert.equal(initial.scopeNotifications.nativeHealthy,true);
    assert.equal(initial.helperScope.live,true);
    await focus(second);
    const away=await until(desktop,value=>value.runtime?.state==='paused'&&value.runtime.backend.held.length===0
      &&BigInt(value.runtime.helperScope?.admission.epoch??'0')>BigInt(initial.helperScope.admission.epoch));
    assert.ok(away.runtime.scopeNotifications.lossEpoch>initial.scopeNotifications.lossEpoch);
    assert.equal((await session()).state,'running');
    // Keep the real receiver away longer than the helper's 250 ms lease.
    // Healthy paused registrations must survive without rearming any input.
    await new Promise(resolve=>setTimeout(resolve,350));
    assert.equal((await session()).state,'running');
    assert.deepEqual((await desktop()).runtime.backend.held,[]);
    await focus(first);
    const returned=await until(desktop,value=>value.runtime?.state==='waiting'&&value.runtime.scopeNotifications.eligible);
    assert.deepEqual(returned.runtime.backend.held,[]);
    assert.equal(keys(await first.read()).filter(item=>item.message===256).length,1,'Held pre-return input must not replay');
    await input(false);
    await until(desktop,value=>value.runtime?.waitingNeutral.length===0);
    await input(true);
    await until(first.read,value=>keys(value??{observations:[]}).filter(item=>item.message===256).length===2);
    await input(false);
    await until(desktop,value=>value.runtime?.backend.held.length===0);
    await request({command:'profile_stop'});stopped=(await session()).owner===null;
    const final=(await desktop()).runtime;
    assert.ok(stopped&&final.backend.cleanupComplete&&!final.backend.uncertain);
    assert.deepEqual(final.backend.held,[]);assert.deepEqual(final.backend.possibleHeld,[]);
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify({actualNativeCockpit:true,authoredInput:true,
      physicalLatencyQualified:false,helperFocusPollingRemoved:true,initial,away:away.runtime,returned:returned.runtime,
      final,receivers:await Promise.all(receivers.map(async receiver=>({name:receiver.name,...await receiver.read()})))},null,2));
  } finally {
    try {
      if(saved){
        if((await session()).owner?.profileId===saved.id)await request({command:'profile_stop'});
        stopped=(await session()).owner===null;
        if(stopped)await request({command:'profile_delete',id:saved.id,expectedRevision:saved.revision});
      }
      if(preferences&&stopped)await request({command:'profile_desktop_configure',enabled:preferences.enabled,scope:preferences.scope});
    } finally {
      for(const receiver of receivers)if(stopped){
        await writeFile(receiver.file.replace(/\.json$/,'.stop'),'stop');
        receiver.child.stdout.destroy();receiver.child.stderr.destroy();receiver.child.unref();
      }
      page.close();
    }
  }
});
