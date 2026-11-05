// Background companion presence must commit promptly in the running native Cockpit.
// Pairing is setup only; after the baseline, the remote peer closes its WSS socket
// and the only native calls are committedOnly Activity reads.
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
import test from 'node:test';
import {randomUUID} from 'node:crypto';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

const enabled=process.platform==='win32'&&process.env.OSG_ACTIVITY_SOURCE_NATIVE==='1';
const allowedBridgeCommands=new Set(['companion_pair_offer','companion_approve']);
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));

test('a remote companion disconnect wakes native Activity without a flush',{
  skip:!enabled,timeout:90_000,
},async t=>{
  const page=await nativePage(),bridgeToken=randomUUID(),companionId=`activity-source-${randomUUID()}`;
  const commands=[],timing={appHash:process.env.OSG_ACTIVITY_SOURCE_APP_HASH??null};
  let server,peer,peerLines,listenerEnabledByFixture=false,peerReady=false,fixturePaired=false;
  const native=async request=>{
    commands.push({command:request.command,committedOnly:request.command==='activity_query'?request.committedOnly:undefined,at:Date.now()});
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...request,clientId:'activity-source-wakes-native'})}})`);
    assert.equal(reply.ok,true,`${request.command}: ${reply.error}`);return reply.state;
  };
  const listen=async()=>{
    const {companions}=await native({command:'companion_inventory'});
    return companions.listener;
  };
  const read=async(committedOnly=true)=>{
    const state=await native({command:'activity_query',committedOnly,activityFilter:{entityId:companionId}});
    return state.activity;
  };
  const startBridge=async()=>{
    server=createServer(async(req,res)=>{
      try{
        assert.equal(req.method,'POST');assert.equal(req.url,'/native');
        assert.equal(req.headers.authorization,`Bearer ${bridgeToken}`);
        let size=0;const chunks=[];
        for await(const chunk of req){size+=chunk.length;assert.ok(size<=64*1024,'Bridge request exceeds its limit');chunks.push(chunk);}
        const request=JSON.parse(Buffer.concat(chunks).toString('utf8'));
        assert.ok(allowedBridgeCommands.has(request.command),'Command is outside the pairing fixture allowlist');
        const reply=await native({command:request.command,companion:request.companion??{}});
        if(request.command==='companion_approve')fixturePaired=true;
        res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({ok:true,state:{companions:reply.companions}}));
      }catch(error){res.writeHead(400,{'content-type':'application/json'});res.end(JSON.stringify({ok:false,error:String(error)}));}
    });
    await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});
    return `http://127.0.0.1:${server.address().port}/native`;
  };
  const startPeer=async bridge=>{
    peer=spawn(process.env.OSG_PYTHON??'python',['scripts/reactive/activity-source-wakes-native.py','--bridge',bridge,'--companion-id',companionId],{
      cwd:process.cwd(),windowsHide:true,env:{...process.env,OSG_ACTIVITY_SOURCE_BRIDGE_TOKEN:bridgeToken},stdio:['pipe','pipe','ignore'],
    });
    peerLines=createInterface({input:peer.stdout});
    const message=await new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>reject(Error('Remote pairing fixture did not become ready.')),30_000);
      peerLines.once('line',line=>{clearTimeout(timer);try{resolve(JSON.parse(line));}catch{reject(Error('Remote fixture returned malformed status.'));}});
      peer.once('error',error=>{clearTimeout(timer);reject(error);});
      peer.once('exit',code=>{clearTimeout(timer);if(!peerReady)reject(Error(`Remote pairing fixture exited (${code}).`));});
    });
    assert.equal(message.type,'ready');assert.equal(message.companionId,companionId);peerReady=true;
  };
  const closePeer=async()=>{
    if(!peer||peer.exitCode!==null||peer.signalCode!==null)return;
    peer.stdin.write('close\n');
    const exited=await new Promise(resolve=>{const timer=setTimeout(()=>resolve(false),5_000);peer.once('exit',()=>{clearTimeout(timer);resolve(true);});});
    if(!exited){peer.kill();await new Promise(resolve=>peer.once('exit',resolve));}
    assert.ok(exited,'Remote WSS fixture did not exit after its owned socket closed.');
  };
  try{
    const initial=await listen();
    if(!initial.enabled){
      assert.equal(initial.configuredEnabled,false,'Refusing to replace an existing inactive listener configuration.');
      const portServer=createServer();await new Promise((resolve,reject)=>portServer.listen(0,'127.0.0.1',resolve));
      const port=portServer.address().port;await new Promise(resolve=>portServer.close(resolve));
      const started=await native({command:'companion_enable',companion:{address:'127.0.0.1',advertisedHost:'127.0.0.1',port,discovery:false,expectedSettingsRevision:initial.settingsRevision}});
      listenerEnabledByFixture=true;
      await until(listen,state=>state.enabled===true,15_000);
      timing.listenerEnableCommand='companion_enable';timing.listenerPort=port;
      assert.equal(started.companions.listener.configuredEnabled,true);
    }

    const bridge=await startBridge();await startPeer(bridge);
    timing.pairReadyAt=Date.now();
    // Pairing and authentication are setup. Wait for the committed discovered/online baseline.
    const before=await until(read,activity=>activity.events.some(event=>event.entityId===companionId&&event.eventType.startsWith('companion_')),20_000);
    assert.ok(before.events.some(event=>event.eventType==='companion_discovered'||event.eventType==='companion_online'));
    const flushStarted=Date.now();await read(false);timing.preMeasurementFlush={startedAt:flushStarted,finishedAt:Date.now(),committedOnly:false};
    const settled=await read(true);assert.ok(settled.events.some(event=>event.eventType==='companion_discovered'||event.eventType==='companion_online'));

    const afterBaseline=commands.length;
    const closeRequestedAt=process.hrtime.bigint();timing.disconnectRequestedAt=Date.now();await closePeer();
    let committed;
    const deadline=Date.now()+500;
    do{
      committed=await read(true);
      if(committed.events.some(event=>event.eventType==='companion_offline'))break;
      await pause(10);
    }while(Date.now()<deadline);
    const latencyMs=Number(process.hrtime.bigint()-closeRequestedAt)/1e6;
    timing.offlineCommittedAt=Date.now();timing.disconnectLatencyMs=latencyMs;
    assert.ok(latencyMs<500,`companion_offline took ${latencyMs.toFixed(1)} ms; expected a background wake under 500 ms.`);
    const offline=committed.events.filter(event=>event.eventType==='companion_offline');
    assert.equal(offline.length,1,'The fixture companion must produce exactly one offline event.');
    assert.equal(offline[0].entityId,companionId);
    assert.ok(commands.slice(afterBaseline).every(item=>item.command==='activity_query'&&item.committedOnly===true),
      'Only independent committedOnly reads may run between WSS close and commit confirmation.');
    timing.commands=commands;
    assert.ok(commands.filter(item=>item.command==='activity_query').every(item=>item.committedOnly===true||item.committedOnly===false));
    assert.equal(commands.filter(item=>item.command==='activity_query'&&item.committedOnly===false).length,1,'Only the documented pre-measurement flush is allowed.');
  }finally{
    const cleanupErrors=[];
    for(const [label,operation] of [
      ['close remote peer',closePeer],
      ...(fixturePaired?[['revoke fixture companion',()=>native({command:'companion_revoke',companion:{companionId}})]]:[]),
      ...(listenerEnabledByFixture?[['disable fixture listener',()=>native({command:'companion_disable'})]]:[]),
    ])try{await operation();}catch(error){cleanupErrors.push(`${label}: ${String(error)}`);}
    peerLines?.close();
    if(server)await new Promise(resolve=>server.close(resolve));
    timing.cleanupErrors=cleanupErrors;timing.commands=commands;
    t.diagnostic(`Activity source wake evidence: ${JSON.stringify(timing)}`);
    page.close();assert.deepEqual(cleanupErrors,[],'Fixture-owned resources must be cleaned up.');
  }
});
