// Opt-in native storage/lifecycle stress. No physical or external output.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('native profile storage survives repeated internal lifecycle commits',{
  skip:process.env.OSG_PERSISTENCE_CONCURRENCY_NATIVE!=='1',timeout:180000,
},async()=>{
  const directory=resolve(`.research/reactive/persistence-concurrency-native/${Date.now()}`);
  await mkdir(directory,{recursive:true});
  const page=await nativePage(),created=new Map(),cleanupErrors=[];
  let owner,completed=0,failure;
  const request=async value=>{
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'persistence-concurrency-native-acceptance'})}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;
  };
  try{
    await until(async()=>{try{return await page.evaluate("document.readyState==='complete'&&document.title==='OpenSimGear Cockpit'&&!!window.__TAURI_INTERNALS__");}catch{return false;}});
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    for(let index=0;index<200;index++){
      let document=(await request({command:'profile_create',name:`Storage lifecycle ${Date.now()} ${index}`})).profiles.document;
      created.set(document.id,document);
      assert.equal(document.virtualOutputs.length,0);
      assert.equal((document.desktopOutputs??[]).length,0);
      const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
      document.resources=[{id:'debug',name:'Owned probe',kind:'debug-controller',required:false,ports:[{id:'button-1',name:'Button',direction:'output',contract}]}];
      document.processingNodes=[{id:'mirror',name:'Identity',operation:{kind:'identity'},inputs:[{id:'in',name:'In',direction:'input',contract}],outputs:[{id:'out',name:'Out',direction:'output',contract}]}];
      document.bindings=[{id:'debug-mirror',from:{nodeId:'debug',portId:'button-1'},to:{nodeId:'mirror',portId:'in'}}];
      document=(await request({command:'profile_editor_save',document,assignments:[],expectedRevision:document.revision})).profiles.document;
      created.set(document.id,document);
      const session=(await request({command:'profile_apply',id:document.id,expectedRevision:document.revision,expectedActiveOwner:null})).profiles.profileSession;
      owner=session.owner;assert.equal(session.state,'running');assert.ok(owner);
      await request({command:'profile_stop'});owner=undefined;
      await request({command:'profile_delete',id:document.id,expectedRevision:document.revision});created.delete(document.id);
      completed++;
    }
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    const recovery=(await request({command:'profile_retired_recovery_status'})).retiredRecovery;
    assert.equal(recovery.required,false);assert.deepEqual(recovery.diagnostics,[]);
  }catch(error){failure=String(error);throw error;
  }finally{
    try{
      if(owner){const active=(await request({command:'profile_status'})).profiles.profileSession.owner;if(active?.session===owner.session&&active?.generation===owner.generation)await request({command:'profile_stop'});}
      for(const document of created.values())try{await request({command:'profile_delete',id:document.id,expectedRevision:document.revision});}catch(error){cleanupErrors.push(String(error));}
      await writeFile(resolve(directory,'evidence.json'),JSON.stringify({at:new Date().toISOString(),actualNativeCockpit:true,completed,
        commandsRetried:0,physicalOutputs:false,externalOutputs:false,failure,cleanupErrors},null,2)+'\n');
    }finally{page.close();}
    assert.deepEqual(cleanupErrors,[]);
  }
});
