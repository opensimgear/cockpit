// Real scope subscriptions and private workers, with no authored non-neutral input.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('native scope subscriptions retire across profile replacement and Stop without input',{
  skip:process.platform!=='win32'||process.env.OSG_DESKTOP_FOCUS_NATIVE!=='1',timeout:60000,
},async()=>{
  const page=await nativePage();let saved,preferences,stopped=false;
  const request=async value=>{
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'desktop-focus-lifecycle-native'})}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;
  };
  const session=async()=>(await request({command:'profile_status'})).profiles.profileSession;
  const desktop=async()=>(await request({command:'profile_desktop_status'})).desktop;
  try{
    assert.equal((await session()).owner,null);stopped=true;
    preferences=await desktop();
    await request({command:'profile_desktop_configure',enabled:true,scope:preferences.scope});
    saved=(await request({command:'profile_create',name:'Native scope lifecycle '+randomUUID()})).profiles.document;
    const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
    const port=(id,direction)=>({id,name:id,direction,contract});
    const document={...saved,
      processingNodes:[{id:'neutral',name:'Neutral',operation:{kind:'constant',value:false},inputs:[],outputs:[port('out','output')]}],
      desktopOutputs:[{id:'keyboard',name:'Keyboard',required:true,scope:{kind:'desktop-wide'},
        ports:[port('hold','input')],actions:[{portId:'hold',action:{kind:'key-hold',key:4}}]}],
      bindings:[{id:'neutral',from:{nodeId:'neutral',portId:'out'},to:{nodeId:'keyboard',portId:'hold'}}],
    };
    saved=(await request({command:'profile_editor_save',document,assignments:[],expectedRevision:saved.revision})).profiles.document;
    const apply=owner=>request({command:'profile_apply',id:saved.id,expectedRevision:saved.revision,expectedActiveOwner:owner});
    const first=(await apply(null)).profiles.profileSession;stopped=false;
    assert.equal(first.state,'running',JSON.stringify(first));
    const firstState=await until(desktop,value=>value.runtime?.scopeNotifications.nativeHealthy);
    assert.ok(firstState.runtime.scopeNotifications.watchId>0);
    assert.deepEqual(firstState.runtime.backend.held,[]);
    const second=(await apply(first.owner)).profiles.profileSession;
    assert.equal(second.state,'running',JSON.stringify(second));
    assert.notDeepEqual(second.owner,first.owner);
    const secondState=await until(desktop,value=>value.runtime?.scopeNotifications.nativeHealthy);
    assert.notEqual(secondState.runtime.scopeNotifications.watchId,firstState.runtime.scopeNotifications.watchId);
    assert.deepEqual(secondState.runtime.backend.held,[]);
    assert.equal(secondState.runtime.backend.uncertain,false);
    await request({command:'profile_stop'});stopped=(await session()).owner===null;
    const final=(await desktop()).runtime;
    assert.ok(stopped&&final.backend.cleanupComplete&&!final.backend.uncertain);
    assert.deepEqual(final.backend.held,[]);assert.deepEqual(final.backend.possibleHeld,[]);
    assert.equal(final.scopeNotifications.nativeHealthy,false);
    assert.equal(final.scopeNotifications.watchId,null);
    const directory=`.research/reactive/desktop-focus-lifecycle-native/${Date.now()}`;
    await mkdir(directory,{recursive:true});
    await writeFile(directory+'/evidence.json',JSON.stringify({actualNativeCockpit:true,
      authoredNonNeutralInput:false,physicalLatencyQualified:false,first:firstState.runtime,second:secondState.runtime,final},null,2));
  }finally{
    try{
      if(saved){
        if((await session()).owner?.profileId===saved.id)await request({command:'profile_stop'});
        stopped=(await session()).owner===null;
        if(stopped)await request({command:'profile_delete',id:saved.id,expectedRevision:saved.revision});
      }
      if(preferences&&stopped)await request({command:'profile_desktop_configure',enabled:preferences.enabled,scope:preferences.scope});
    }finally{page.close();}
  }
});
