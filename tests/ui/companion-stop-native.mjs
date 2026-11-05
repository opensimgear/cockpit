// Actual native listener; raw loopback peers deliberately never start TLS.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer, createConnection} from 'node:net';
import {mkdir, writeFile} from 'node:fs/promises';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

test('native companion Stop interrupts sixteen stalled TLS peers', {
  skip:process.env.OSG_COMPANION_STOP_NATIVE!=='1',timeout:30000,
},async()=>{
  const page=await nativePage(),peers=[],closed=[];
  const directory=`.research/reactive/companion-stop-native/${Date.now()}`;
  await mkdir(directory,{recursive:true});
  const evidence={actualNativeCockpit:true,loopbackOnly:true,tlsStarted:false,cleanupErrors:[]};
  let configured=false;
  const native=async(command,companion={})=>{
    const request={command,clientId:'companion-stop-native',...(command.startsWith('companion_')?{companion}:{})};
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(request)}})`);
    assert.equal(reply.ok,true,`${command}: ${reply.error}`);return reply.state;
  };
  const inventory=async()=>(await native('companion_inventory')).companions.listener;
  const disable=async()=>native('companion_disable',{expectedSettingsRevision:(await inventory()).settingsRevision});
  try {
    const initial=await inventory();assert.equal(initial.configuredEnabled,false,'Start with a disabled dev listener');
    assert.equal((await native('profile_status')).profiles.profileSession.owner,null);
    const reservation=createServer();await new Promise(resolve=>reservation.listen(0,'127.0.0.1',resolve));
    const port=reservation.address().port;await new Promise(resolve=>reservation.close(resolve));
    const enabled=await native('companion_enable',{address:'127.0.0.1',advertisedHost:'127.0.0.1',port,
      discovery:false,expectedSettingsRevision:initial.settingsRevision});
    configured=true;assert.equal(enabled.companions.listener.enabled,true);
    await Promise.all(Array.from({length:16},()=>new Promise((resolve,reject)=>{
      const peer=createConnection({host:'127.0.0.1',port});peers.push(peer);
      closed.push(new Promise(done=>peer.once('close',done)));
      peer.once('connect',resolve);peer.once('error',reject);
    })));
    evidence.before=await until(inventory,listener=>listener.activeConnections===16,2000);
    const started=performance.now();
    const stopped=await native('companion_disable',{expectedSettingsRevision:evidence.before.settingsRevision});
    evidence.stopMs=performance.now()-started;
    configured=false;
    assert.ok(evidence.stopMs<1000,`Stop took ${evidence.stopMs.toFixed(1)} ms`);
    assert.equal(stopped.companions.listener.enabled,false);
    evidence.after=await until(inventory,listener=>listener.activeConnections===0,1000);
    await Promise.race([Promise.all(closed),new Promise((_,reject)=>setTimeout(()=>reject(Error('Stalled peers did not close')),1000))]);
    evidence.closedPeers=closed.length;
    assert.equal((await native('profile_status')).profiles.profileSession.owner,null);
  } finally {
    if(configured)await disable().catch(error=>evidence.cleanupErrors.push(String(error)));
    for(const peer of peers)peer.destroy();
    await writeFile(`${directory}/evidence.json`,JSON.stringify(evidence,null,2));
    page.close();
    assert.deepEqual(evidence.cleanupErrors,[]);
  }
});
