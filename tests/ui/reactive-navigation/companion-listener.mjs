import assert from 'node:assert/strict';
import {createServer} from 'node:net';
import {setTimeout as delay} from 'node:timers/promises';
import {until} from '../../../scripts/reactive/cdp.mjs';

async function privateLoopbackPort() {
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const {port} = server.address();
  await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  return port;
}

export async function checkCompanionListenerVisibility({page,request,clickTab,pending,active,baselines,listenerOnlyReads}){
  const setHidden=hidden=>page.evaluate(`(() => {
    if (${hidden}) Object.defineProperty(document,'hidden',{configurable:true,value:true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const reader=()=>[...active.entries()].find(([,value])=>value.topic==='companion.listener');
  const switchDisabled=()=>page.evaluate(`document.querySelector('[aria-label="Companion listener settings"] [role="switch"]')?.disabled`);
  const switchState=()=>page.evaluate(`document.querySelector('[aria-label="Companion listener settings"] [role="switch"]')?.getAttribute('aria-checked')`);
  const listenerStatus=async()=>(await request({command:'companion_status'})).companions.listener;
  const privateListener=process.env.OSG_NAVIGATION_PRIVATE_LISTENER==='1';
  let firstReader, restoreDisabled=false;
  try{
    const before=privateListener ? await listenerStatus() : undefined;
    if(before)assert.equal(before.configuredEnabled,false,'Use a fresh private listener database');
    const beforeOpen=listenerOnlyReads();
    await clickTab('Companions');
    await until(()=>reader()&&baselines.get(reader()[1].token)===1);
    await until(()=>listenerOnlyReads()>beforeOpen);
    await until(switchDisabled,value=>value===false);
    firstReader=reader()[0];
    const beforeHide=listenerOnlyReads();

    await setHidden(true);
    await until(()=>pending.size===0&&!reader()).catch(cause=>{
      throw new Error('Hidden Companion listener settings retained their observation reader',{cause});
    });
    assert.ok(firstReader,'Companion settings must have opened its listener reader');
    assert.equal(await switchDisabled(),true,'Hidden listener controls must be disabled');
    if(before){
      const port=await privateLoopbackPort();
      restoreDisabled=true;
      const {listener}=(await request({command:'companion_enable',companion:{
        expectedSettingsRevision:before.settingsRevision,address:'127.0.0.1',
        advertisedHost:'127.0.0.1',port,discovery:false,
      }})).companions;
      assert.equal(listener.enabled,true);
      assert.equal(listener.config.bind,'127.0.0.1:'+port);
      assert.equal(await switchState(),'false','The hidden view should await its new baseline');
    }
    await delay(300);
    assert.equal(listenerOnlyReads(),beforeHide,'Hidden listener settings must not read status');

    await setHidden(false);
    await until(()=>reader()&&reader()[0]!==firstReader&&baselines.get(reader()[1].token)===1);
    await until(()=>listenerOnlyReads()===beforeHide+1);
    await until(switchDisabled,value=>value===false);
    assert.notEqual(reader()[0],firstReader,'Showing settings must create a fresh reader');
    assert.equal(baselines.get(reader()[1].token),1,'The reopened reader must receive one current baseline');
    assert.equal(listenerOnlyReads(),beforeHide+1,'The fresh baseline must own one listener-only read');
    if(before)assert.equal(await switchState(),'true','Reveal must reflect the committed loopback listener');
    console.log(JSON.stringify({companionListenerDocumentHideAndReopen:'passed',
      hiddenReads:0,reopenReads:1,backgroundCommit:!!before}));
  }finally{
    try{
      await clickTab('General');
      await setHidden(false);
      await until(()=>pending.size===0&&!reader());
    }finally{
      if(restoreDisabled){
        const listener=await listenerStatus();
        if(listener.configuredEnabled)await request({command:'companion_disable',companion:{
          expectedSettingsRevision:listener.settingsRevision,
        }});
        assert.equal((await listenerStatus()).enabled,false);
      }
    }
  }
}
