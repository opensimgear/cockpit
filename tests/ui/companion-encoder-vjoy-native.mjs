import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {once} from 'node:events';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {createServer} from 'node:http';
import {resolve} from 'node:path';
import {createInterface} from 'node:readline';
import {setTimeout as delay} from 'node:timers/promises';
import test from 'node:test';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';
import {mcpVjoyObservation} from './mcp-vjoy-observation.mjs';
import {summarizeMilliseconds} from './vjoy-rate-transport.mjs';

const enabled = process.env.OSG_ENCODER_VJOY_NATIVE === '1';
const outputNode = 'burst-vjoy';
const debugNodes = ['burst-fast','burst-peer'];
const encoderNode = 'panel-encoder';
const control = 'flight-input-5';

test('A real companion encoder burst and native debug sources preserve exact Windows vJoy edges', {
  skip:!enabled, timeout:90000,
}, async () => {
  const probePath=process.env.OSG_COMPANION_RECOVERY_VJOY_PROBE;
  assert.ok(probePath,'Set OSG_COMPANION_RECOVERY_VJOY_PROBE to the compiled Raw Input probe');
  const page=await nativePage();
  const token=randomUUID(), instance='encoder-burst-'+randomUUID();
  const invoke=request=>page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{
    request:${JSON.stringify(request.command==='qualification_debug_burst'?request:{...request,clientId:'encoder-vjoy-qualification'})}
  })`);
  const request=async value=>{
    const reply=await invoke(value);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);
    return reply.state;
  };
  const companion=async(command,params={})=>(await request({command,companion:params})).companions;
  const inventory=async()=>(await request({command:'system_inventory'})).systemInventory.virtualDevices;
  const allowed=new Set(['companion_pair_offer','companion_approve']);
  const server=createServer(async(req,res)=>{
    try {
      assert.ok(req.method==='POST'&&req.url==='/native'&&req.headers.authorization===`Bearer ${token}`);
      let bytes=0;const chunks=[];
      for await(const chunk of req){bytes+=chunk.length;assert.ok(bytes<=4096);chunks.push(chunk);}
      const data=JSON.parse(Buffer.concat(chunks).toString('utf8'));
      assert.ok(allowed.has(data.command));
      if(data.command==='companion_approve')assert.equal(data.companion?.companionId,instance);
      const reply=await invoke(data);
      res.writeHead(200,{'content-type':'application/json'}).end(JSON.stringify(reply));
    } catch(error) {
      res.writeHead(400,{'content-type':'application/json'}).end(JSON.stringify({ok:false,error:String(error)}));
    }
  });
  let peer,peerDone,probe,probeDone,profile,observation,stopped=false,slot,peerError='',failure;
  try {
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    assert.equal((await companion('companion_status')).listener?.enabled,true,'Enable the private companion listener on loopback');
    server.listen(0,'127.0.0.1');await once(server,'listening');
    peer=spawn(process.env.OSG_PYTHON??'python',['scripts/companion-encoder-burst.py',
      '--bridge',`http://127.0.0.1:${server.address().port}/native`,'--instance',instance],
    {windowsHide:true,env:{...process.env,OSG_NATIVE_BRIDGE_TOKEN:token},stdio:['pipe','pipe','pipe']});
    peer.stderr.on('data',chunk=>{peerError+=String(chunk);});
    peerDone=once(peer,'close');void peerDone.catch(()=>{});
    const messages=createInterface({input:peer.stdout})[Symbol.asyncIterator]();
    const fromPeer=async phase=>{
      const message=await messages.next();
      assert.equal(message.done,false,`Encoder peer exited waiting for ${phase}: ${peerError}`);
      const value=JSON.parse(message.value);
      assert.equal(value.phase,phase,JSON.stringify(value));
      return value;
    };
    await fromPeer('paired');
    const bytes=await readFile(new URL('../../scripts/fixtures/compiled-flight-publication.json',import.meta.url));
    const {publicationHash}=JSON.parse(bytes);
    const {uploadId}=await companion('companion_publish_begin',{size:bytes.length});
    for(let offset=0;offset<bytes.length;offset+=8192){
      await companion('companion_publish_chunk',{uploadId,offset,bytes:[...bytes.subarray(offset,offset+8192)]});
    }
    await companion('companion_publish_commit',{uploadId,saveAsLatest:false});
    await companion('companion_assign',{companionId:instance,instanceId:instance,publicationHash});
    peer.stdin.write(JSON.stringify({publicationHash})+'\n');
    await fromPeer('ready');

    const resource=await until(async()=>(await request({command:'profile_resources_list',limit:128})).profileResources.resources
      .find(item=>item.attributes?.companionId===instance&&item.attributes?.componentKind!=='paired-companion'));
    const {ports}=(await request({command:'profile_resources_inspect',resourceId:resource.id,limit:64})).profileResources;
    const observed=ports.find(port=>port.current&&port.descriptor.id===control);
    assert.ok(observed,JSON.stringify(ports));
    slot=await until(async()=>(await inventory()).find(device=>device.status==='free'&&device.buttons>=2
      &&Array.isArray(device.axes)&&!device.discreteHats&&!device.forceFeedback
      &&device.buttons+device.axes.length+device.continuousHats<=128));
    const layout={buttons:Array.from({length:slot.buttons},(_,index)=>'button-'+(index+1)),
      axes:slot.axes.map((_,index)=>({portId:'axis-'+(index+1),range:'bipolar',fallback:0})),
      hats:Array.from({length:slot.continuousHats},(_,index)=>'hat-'+(index+1))};
    const document=(await request({command:'profile_create',name:'Encoder and native burst '+randomUUID()})).profiles.document;
    profile=document;
    const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
    document.resources.push({id:encoderNode,name:'WSS encoder',kind:'companion',required:true,
      ports:[{...observed.descriptor,id:'pulse'}]});
    document.companionAssignments.push({resourceId:encoderNode,publicationHash});
    for(const id of debugNodes)document.resources.push({id,name:id,kind:'debug-controller',required:false,
      ports:[{id:'button-1',name:'Button',direction:'output',contract}]});
    document.processingNodes.push({id:'peer-sink',name:'125 Hz native peer',operation:{kind:'identity'},
      inputs:[{id:'in',name:'Input',direction:'input',contract}],
      outputs:[{id:'out',name:'Output',direction:'output',contract}]});
    document.bindings.push({id:'peer-edge',from:{nodeId:debugNodes[1],portId:'button-1'},to:{nodeId:'peer-sink',portId:'in'}});
    const assignments=[{resourceId:encoderNode,sourceId:resource.id,ports:[{
      profilePortId:'pulse',observedPortId:observed.descriptor.id,expectedContractHash:observed.contractHash,
    }]}];
    const draft=await page.evaluate(`(async()=>{
      const {graphSetVirtual,graphConnect}=await import('/src/lib/profile-graph.ts');
      let draft=graphSetVirtual(${JSON.stringify({document,assignments})},${JSON.stringify({
        id:outputNode,name:'Burst test output',required:true,layout,
      })});
      draft=graphConnect(draft,{nodeId:'${debugNodes[0]}',portId:'button-1'},{nodeId:'${outputNode}',portId:'button-1'});
      return graphConnect(draft,{nodeId:'${encoderNode}',portId:'pulse'},{nodeId:'${outputNode}',portId:'button-2'});
    })()`);
    profile=(await request({command:'profile_editor_save',document:draft.document,assignments,
      expectedRevision:profile.revision})).profiles.document;
    await request({command:'profile_virtual_slot_set',id:profile.id,expectedRevision:profile.revision,
      nodeId:outputNode,slot:slot.slot});
    const {owner,state}=(await request({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,
      expectedActiveOwner:null})).profiles.profileSession;
    assert.equal(state,'running');
    assert.equal(owner.profileId,profile.id);
    await until(async()=>(await companion('companion_status')).host.instances
      .find(row=>row.companionId===instance&&row.profileOwned&&row.armed&&row.values[control]===false));
    observation=await mcpVjoyObservation(page,owner,{nodeId:outputNode,capturePhysical:false});

    const probeStarted=process.hrtime.bigint();
    probe=spawn(probePath,['--vjoy-buttons'],{windowsHide:true,stdio:['ignore','pipe','pipe']});
    probeDone=once(probe,'close');void probeDone.catch(()=>{});
    const lines=createInterface({input:probe.stdout})[Symbol.asyncIterator]();
    assert.equal((await lines.next()).value,'ready');
    const probeReady=process.hrtime.bigint();
    const startNs=String(process.hrtime.bigint()+450_000_000n), detents=24;
    const qualification=request({command:'qualification_debug_burst',owner,nodes:debugNodes,startNs});
    peer.stdin.write(JSON.stringify({startNs,detents})+'\n');
    const [native,{frames},mcp]=await Promise.all([
      qualification,fromPeer('burst'),observation.sample(true,'button-2'),
    ]);
    const [fast,slow]=native.qualificationBurst;
    assert.deepEqual([fast.length,slow.length,frames.length],[193,25,2*detents]);
    for(const sequence of [fast,slow,frames]){
      assert.ok(sequence.every((row,index)=>row.value===(index%2===0)));
    }
    await delay(150);
    assert.deepEqual((await request({command:'profile_status'})).profiles.profileSession.owner,owner);
    const stopNs=process.hrtime.bigint(), stopReplyMs=await observation.stop();
    stopped=true;
    const {unreadMs}=await observation.resumeSlow();
    await until(inventory,devices=>devices.find(device=>device.slot===slot.slot)?.status==='free');
    const report=JSON.parse((await lines.next()).value);
    assert.equal((await probeDone)[0],0);
    const events=report.buttonEvents.filter(event=>event.report===slot.slot);
    assert.ok(events.every(event=>event.status===0x00110000));
    assert.equal(new Set(events.map(event=>event.device)).size,1);
    const hz=BigInt(report.receiverTickHz), origin=BigInt(report.receiverStartedTicks);
    const toNs=ticks=>ticks*1_000_000_000n/hz;
    assert.ok(toNs(origin)>=probeStarted&&toNs(origin)<=probeReady,'Raw Input and Node must share the native monotonic clock');
    const receive=event=>toNs(origin+BigInt(event.receivedTicks));
    const edges=field=>events.filter((event,index)=>event[field]!== (events[index-1]?.[field]??false));
    const rawFast=edges('pressed'),rawEncoder=edges('peerPressed');
    assert.deepEqual(rawFast.map(row=>row.pressed),[...fast.map(row=>row.value),false]);
    assert.deepEqual(rawEncoder.map(row=>row.peerPressed),frames.map(row=>row.value));
    const stopReleaseMs=Number(receive(rawFast.at(-1))-stopNs)/1e6;
    assert.ok(stopReleaseMs>=0);
    const offer=row=>BigInt(row.atNs);
    const rate=rows=>(rows.length-1)*1e9/Number(offer(rows.at(-1))-offer(rows[0]));
    const detentHz=rate(frames.filter(row=>row.value)),fastHz=rate(fast),slowHz=rate(slow);
    for(const [measured,target] of [[detentHz,125],[fastHz,1000],[slowHz,125]]) {
      assert.ok(Math.abs(measured/target-1)<=.05,`${measured} Hz against ${target} Hz`);
    }
    const schedules=[fast,slow,frames];
    const overlapStart=schedules.map(rows=>offer(rows[0])).reduce((a,b)=>a>b?a:b);
    const overlapEnd=schedules.map(rows=>offer(rows.at(-1))).reduce((a,b)=>a<b?a:b);
    const overlapMs=Number(overlapEnd-overlapStart)/1e6;
    assert.ok(overlapMs>=180,`All three sources must overlap for at least 180 ms, observed ${overlapMs} ms`);
    const latency=(offers,receipts)=>summarizeMilliseconds(offers.map((row,index)=>{
      const ms=Number(receive(receipts[index])-offer(row))/1e6;
      assert.ok(ms>=0,'Windows receipt preceded its source offer');
      return ms;
    }));
    const evidence={detents,fastHz,slowHz,detentHz,overlapMs,fastEdges:rawFast.length,
      encoderEdges:rawEncoder.length,encoderOfferToReceiptMs:latency(frames,rawEncoder),
      debugOfferToReceiptMs:latency(fast,rawFast),
      nativePendingPeak:Math.max(...[...fast,...slow].map(row=>row.pendingFrames)),
      stopReleaseMs,stopReplyMs,unreadMs,mcp,slot:slot.slot};
    const root=resolve('.research/reactive/encoder-vjoy');await mkdir(root,{recursive:true});
    await writeFile(resolve(root,'latest.json'),JSON.stringify({...evidence,frames,fast,slow,events},null,2)+'\n');
    console.log(JSON.stringify(evidence));
  } catch(error) {
    failure=error;
    throw error;
  } finally {
    const cleanupErrors=[];
    const cleanup=async work=>{try{await work();}catch(error){cleanupErrors.push(error);}};
    if(profile&&!stopped)await cleanup(()=>request({command:'profile_stop'}));
    if(observation)await cleanup(()=>observation.close());
    if(profile)await cleanup(()=>request({command:'profile_delete',id:profile.id,expectedRevision:profile.revision}));
    if(peer&&peer.exitCode===null&&peer.signalCode===null){
      await cleanup(async()=>{peer.stdin.end('close\n');const [code]=await peerDone;assert.equal(code,0,peerError);});
    }
    await cleanup(()=>companion('companion_unassign',{instanceId:instance}));
    await cleanup(()=>companion('companion_revoke',{companionId:instance}));
    if(probe&&probe.exitCode===null&&probe.signalCode===null){probe.kill();await cleanup(()=>probeDone);}
    await new Promise(resolve=>server.close(resolve));
    page.close();
    if(cleanupErrors.length)throw new AggregateError(failure?[failure,...cleanupErrors]:cleanupErrors,'Encoder qualification cleanup failed');
  }
});
