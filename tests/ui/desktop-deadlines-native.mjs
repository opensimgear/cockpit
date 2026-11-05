// Opt-in authored input -> native desktop worker -> exact receive-only window.
import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdir, readFile, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

test('native desktop tap completes through the receiver without another source update', {
  skip:process.platform!=='win32'||process.env.OSG_DESKTOP_DEADLINES_NATIVE!=='1'||process.env.OSG_DESKTOP_NATIVE_INJECT!=='1', timeout:60000,
}, async()=>{
  const page=await nativePage(), directory=resolve('.research/reactive/desktop-deadlines-native',String(Date.now()));
  await mkdir(directory,{recursive:true});
  const file=resolve(directory,'receiver.json');
  let receiver, saved, preferences, stopped=false;
  const request=async value=>{
    const response=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'desktop-deadlines-native'})}})`);
    assert.equal(response.ok,true,`${value.command}: ${response.error}`);return response.state;
  };
  const status=async()=>(await request({command:'profile_status'})).profiles.profileSession;
  const received=async()=>{try{return JSON.parse(await readFile(file,'utf8'));}catch{return null;}};
  try {
    assert.equal((await status()).owner,null);
    stopped=true;
    preferences=(await request({command:'profile_desktop_status'})).desktop;
    const executable=process.env.OSG_DESKTOP_RECEIVER??resolve('crates/desktop-output/target/debug/examples/native_receiver.exe');
    receiver=spawn(executable,['--receiver-only',file],{windowsHide:true,stdio:['ignore','pipe','pipe']});
    let errors='';receiver.stderr.on('data',bytes=>errors+=bytes);
    receiver.stdout.resume();
    receiver.on('error',error=>{errors+=String(error);});
    const target=await until(received,value=>!!value?.identity,10000);
    await writeFile(resolve(directory,'receiver.focus'),'focus');
    await until(received,value=>value?.focusRequests>0&&value.foregroundWindow===value.focusAfter,10000);
    const recent=await until(async()=>(await request({command:'profile_desktop_status'})).desktop,
      value=>value.recentApplication?.pid===target.identity.pid&&value.recentApplication.created===target.identity.created,10000);
    assert.equal(recent.recentApplication.executable.toLowerCase(),target.identity.executable.toLowerCase(),errors);
    const captured=(await request({command:'profile_desktop_capture_application'})).desktop;
    const reference=captured.capturedApplicationReference;
    assert.ok(reference);
    await request({command:'profile_desktop_configure',enabled:true,scope:preferences.scope});
    saved=(await request({command:'profile_create',name:'Native desktop deadline '+randomUUID()})).profiles.document;
    const state={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
    const event={delivery:'event',schema:{type:'boolean'}};
    const port=(id,direction,contract)=>({id,name:id,direction,contract});
    const document={...saved,
      resources:[{id:'debug',name:'Authored input',kind:'debug-controller',required:false,ports:[port('button-1','output',state)]}],
      processingNodes:[{id:'edge',name:'Edge',operation:{kind:'event-edge',edge:'rising'},
        inputs:[port('in','input',state)],outputs:[port('out','output',event)]}],
      desktopOutputs:[{id:'keyboard',name:'Keyboard',required:true,scope:{kind:'application',applicationReference:reference},
        ports:[port('tap','input',event)],actions:[{portId:'tap',action:{kind:'key-tap',key:4,downMs:1,upMs:1}}]}],
      bindings:[{id:'input',from:{nodeId:'debug',portId:'button-1'},to:{nodeId:'edge',portId:'in'}},
        {id:'output',from:{nodeId:'edge',portId:'out'},to:{nodeId:'keyboard',portId:'tap'}}],
    };
    saved=(await request({command:'profile_editor_save',document,assignments:[],expectedRevision:saved.revision})).profiles.document;
    const applied=(await request({command:'profile_apply',id:saved.id,expectedRevision:saved.revision,expectedActiveOwner:null})).profiles.profileSession;
    assert.equal(applied.state,'running',JSON.stringify(applied));
    await request({command:'profile_debug_controller_input',owner:applied.owner,nodeId:'debug',streamId:randomUUID(),
      sequence:'1',changes:[{portId:'button-1',value:true}]});
    const result=await until(received,value=>{
      const keys=value?.observations.filter(item=>item.injectedMarker&&item.scanCode===30&&[256,257].includes(item.message));
      return keys?.length>=2&&value;
    },10000);
    const keys=result.observations.filter(item=>item.injectedMarker&&item.scanCode===30&&[256,257].includes(item.message));
    assert.deepEqual(keys.map(item=>item.message),[256,257]);
    const observedDwellUs=(BigInt(keys[1].atUs)-BigInt(keys[0].atUs)).toString();
    await request({command:'profile_stop'});
    stopped=(await status()).owner===null;
    assert.ok(stopped);
    const desktop=(await request({command:'profile_desktop_status'})).desktop;
    assert.equal(desktop.runtime.backend.cleanupComplete,true);
    assert.equal(desktop.runtime.backend.uncertain,false);
    assert.deepEqual(desktop.runtime.backend.held,[]);
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify({actualNativeCockpit:true,authoredInput:true,
      physicalLatencyQualified:false,requestedDownMs:1,requestedUpMs:1,profile:saved.id,target:target.identity,
      sourceUpdates:1,keys,observedDwellUs,desktop,stopped},null,2));
  } finally {
    try {
      if(saved){
        const owner=(await status()).owner;
        if(owner?.profileId===saved.id)await request({command:'profile_stop'});
        stopped=(await status()).owner===null;
        if(stopped)await request({command:'profile_delete',id:saved.id,expectedRevision:saved.revision});
      }
      if(preferences&&stopped)await request({command:'profile_desktop_configure',enabled:preferences.enabled,scope:preferences.scope});
    } finally {
      if(receiver&&stopped){await writeFile(resolve(directory,'receiver.stop'),'stop');receiver.stdout.destroy();receiver.stderr.destroy();receiver.unref();}
      page.close();
    }
  }
});
