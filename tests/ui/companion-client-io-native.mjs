// Native service-library client against the actual Cockpit. Mobile shell and
// device qualification remain separate; the probe uses an isolated sealed DB.
import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
import {createServer} from 'node:net';
import {mkdir, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('native client pairing, presence handoff and cancelled TLS use the running Cockpit',{
  skip:process.env.OSG_COMPANION_CLIENT_IO_NATIVE!=='1',timeout:60000,
},async()=>{
  assert.equal(process.platform,'win32','This qualification uses Windows native protection');
  assert.ok(process.env.OSG_NATIVE_CLIENT_PROBE,'Build the isolated native client probe first');
  const directory=resolve('.research/reactive/companion-client-io-native',String(Date.now()));
  await mkdir(directory,{recursive:true});
  const page=await nativePage(),peers=[],pending=[];
  const stalled=createServer(peer=>{peers.push(peer);peer.on('error',()=>{});});
  let child,configured=false,companionId,probeError='';
  const evidence={actualNativeCockpit:true,nativeLibraryClient:true,mobileShellQualified:false,checks:[],cleanupErrors:[]};
  const host=async(command,payload={})=>{
    const request={command,clientId:'native-client-io-qualification',...(command.startsWith('companion_')?{companion:payload}:payload)};
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(request)}})`);
    assert.equal(reply.ok,true,`${command}: ${reply.error}`);return reply.state;
  };
  const companion=async(command,payload={})=>(await host(command,payload)).companions;
  const listener=async()=>(await companion('companion_inventory')).listener;
  const call=request=>new Promise((resolve,reject)=>{
    const task={resolve,reject};task.timer=setTimeout(()=>{reject(Error('Native probe command timed out'));},5000);
    pending.push(task);child.stdin.write(JSON.stringify(request)+'\n');
  }).then(reply=>{assert.equal(reply.ok,true,reply.error);return reply.state;});
  const status=()=>call({type:'status'});
  const connected=async()=> (await companion('companion_status')).host.clients.find(row=>row.companionId===companionId);
  try {
    const initial=await listener();assert.equal(initial.configuredEnabled,false);
    assert.equal((await host('profile_status')).profiles.profileSession.owner,null);
    const reservation=createServer();await new Promise(r=>reservation.listen(0,'127.0.0.1',r));
    const port=reservation.address().port;await new Promise(r=>reservation.close(r));
    await companion('companion_enable',{address:'127.0.0.1',advertisedHost:'127.0.0.1',port,discovery:false,expectedSettingsRevision:initial.settingsRevision});
    configured=true;
    const offer=await companion('companion_pair_offer');
    const payload=JSON.parse(offer.pairing.manualPayload);
    child=spawn(process.env.OSG_NATIVE_CLIENT_PROBE,[resolve(directory,'client')],{windowsHide:true,stdio:['pipe','pipe','pipe']});
    child.stderr.on('data',bytes=>{probeError+=bytes.toString();});
    createInterface({input:child.stdout}).on('line',line=>{
      const task=pending.shift();if(!task)return;clearTimeout(task.timer);
      try{task.resolve(JSON.parse(line));}catch(error){task.reject(error);}
    });
    child.on('error',error=>{for(const task of pending.splice(0)){clearTimeout(task.timer);task.reject(error);}});
    child.on('exit',code=>{for(const task of pending.splice(0)){clearTimeout(task.timer);task.reject(Error(`Native probe exited ${code}: ${probeError}`));}});
    assert.equal((await status()).authenticationAvailable,true);
    const name=`Native I/O qualification ${Date.now()}`;
    await call({type:'pair-start',payload,name});
    evidence.paired=await until(status,row=>row.phase==='paired'&&!row.busy);
    const row=await until(async()=>(await companion('companion_status')).host.clients.find(row=>row.name===name),row=>!!row?.connected);
    companionId=row.companionId;
    evidence.checks.push('Pinned native pairing and passive presence');
    let epoch=evidence.paired.epoch;
    for(let pass=0;pass<3;pass++){
      await call({type:'refresh-assignments'});
      const next=await until(status,row=>row.phase==='paired'&&!row.busy);
      assert.ok(next.epoch>epoch);epoch=next.epoch;await until(connected,row=>row?.connected);
    }
    evidence.checks.push('Three presence-to-worker-to-presence epoch handoffs');
    const quietUntil=Date.now()+6500;
    while(Date.now()<quietUntil){assert.equal((await connected()).connected,true);await new Promise(r=>setTimeout(r,150));}
    evidence.checks.push('Quiet presence remains authenticated through three keepalive deadlines');
    await new Promise(r=>stalled.listen(0,'127.0.0.1',r));
    const accepted=new Promise(r=>stalled.once('connection',r));
    await call({type:'pair-start',name,payload:{...payload,endpoint:`wss://127.0.0.1:${stalled.address().port}/companion`}});
    await accepted;await until(status,row=>row.busy);
    const began=performance.now();await call({type:'disconnect'});
    const stopped=await until(status,row=>!row.busy,1000);evidence.stopMs=performance.now()-began;
    assert.ok(evidence.stopMs<1000,`TLS-stalled native client Stop took ${evidence.stopMs} ms`);
    assert.equal(stopped.phase,'disconnected');
    evidence.checks.push('Stalled native TLS is cancelled before its three-second deadline');
    await until(connected,row=>row?.connected);
    await call({type:'forget'});assert.equal((await status()).phase,'unpaired');
    await until(connected,row=>!row?.connected,1000);
    evidence.checks.push('Committed Forget closes passive presence');
  }finally{
    if(child?.exitCode===null){await call({type:'forget'}).catch(error=>evidence.cleanupErrors.push(String(error)));child.stdin.end();
      await new Promise(resolve=>{const timer=setTimeout(()=>{child.kill();resolve();},2000);child.once('exit',()=>{clearTimeout(timer);resolve();});});}
    if(companionId)await companion('companion_revoke',{companionId}).catch(error=>evidence.cleanupErrors.push(String(error)));
    if(configured)await companion('companion_disable',{expectedSettingsRevision:(await listener()).settingsRevision}).catch(error=>evidence.cleanupErrors.push(String(error)));
    for(const peer of peers)peer.destroy();if(stalled.listening)await new Promise(r=>stalled.close(r));
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify(evidence,null,2));page.close();
    assert.deepEqual(evidence.cleanupErrors,[]);
  }
});
