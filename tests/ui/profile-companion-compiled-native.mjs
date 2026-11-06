// Use an isolated running Cockpit with its companion listener enabled on loopback.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {createInterface} from 'node:readline';
import {randomUUID,createHash,X509Certificate} from 'node:crypto';
import {connect} from 'node:tls';
import {readFile} from 'node:fs/promises';
import {setTimeout as delay} from 'node:timers/promises';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';
test('compiled browser input reaches the profile and returns host feedback', {timeout:90000}, async(t)=>{
  const qualifyHostWake=process.env.OSG_COMPANION_HOST_WAKE_NATIVE==='1';
  const qualifyFeedback=process.env.OSG_COMPANION_FEEDBACK_NATIVE==='1';
  const recoveryProbe=process.env.OSG_COMPANION_RECOVERY_VJOY_PROBE;
  const slowFeedback=process.env.OSG_COMPANION_SLOW_FEEDBACK_NATIVE==='1';
  if(slowFeedback)assert.ok(recoveryProbe,'Slow feedback qualification requires the vJoy probe');
  const cockpit=await nativePage();
  const clientId='compiled-companion-qualification';
  const request=async(command,params={})=>{
   const reply=await cockpit.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({command,clientId,...params})}})`);
   assert.equal(reply.ok,true,reply.error??command);
   return reply.state;
  };
  const companion=async(command,params={})=>(await request(command,{companion:params})).companions;
  const id='reactive-'+randomUUID();
  const publication=JSON.parse(await readFile(new URL('../../scripts/fixtures/compiled-flight-publication.json',import.meta.url)));
  const {chromium}=createRequire(import.meta.url)(process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
  let browser, peer, profile, desktopPreferences, slot, probe;
  const inventory=async()=>(await request('system_inventory')).systemInventory.virtualDevices;
  const desktop=async()=>(await request('profile_desktop_status')).desktop;
  const assertHostWake=async phase=>{
   const status=await desktop();
   assert.equal(status.runtime.serviceIntervalMs,null,`${phase}: companion routing must use retained wakes and deadlines`);
   assert.deepEqual(status.runtime.backend.held,[],`${phase}: neutral desktop binding must hold no keys`);
   assert.equal(status.runtime.backend.uncertain,false,`${phase}: desktop delivery must remain certain`);
   assert.equal(status.runtime.helperScope.live,true,`${phase}: quiet host authorization must remain live`);
   t.diagnostic(JSON.stringify({qualification:'companion-host-wake',phase,
    appHash:process.env.OSG_COMPANION_HOST_WAKE_APP_HASH??null,authoredNonNeutralDesktopInput:false,
    latencyPercentilesQualified:false,runtime:status.runtime}));
  };
  try {
   const initial=await companion('companion_status');
   assert.equal(initial.listener?.enabled,true,'Enable the isolated companion listener first');
   assert.equal((await request('profile_status')).profiles.profileSession.state,'stopped');
   if(qualifyHostWake){
    desktopPreferences=await desktop();
    await request('profile_desktop_configure',{enabled:true,scope:desktopPreferences.scope});
   }
   const existing=new Set(initial.host.clients.map(row=>row.companionId));
   const offer=await companion('companion_pair_offer',{kind:'browser'});
   const url=new URL(offer.pairing.browserUrl);
   assert.equal(url.hostname,'127.0.0.1');
   // Pin only this test listener in the disposable browser process.
   const pin=await new Promise((resolve,reject)=>{
    const socket=connect({host:url.hostname,port:Number(url.port),rejectUnauthorized:false},()=>{
     try{resolve(createHash('sha256').update(new X509Certificate(socket.getPeerCertificate().raw).publicKey.export({type:"spki",format:"der"})).digest('base64'));}catch(error){reject(error);}finally{socket.destroy();}
    });socket.on('error',reject);
   });
   browser=await chromium.launch({channel:process.env.OSG_BROWSER_CHANNEL??'msedge',headless:true,args:['--ignore-certificate-errors-spki-list='+pin]});
   const page=await browser.newPage();
   await page.addInitScript(slowFeedback=>{
    window.__feedbackProbe=[];
    window.__feedbackTrace=[];
    const Original=window.SharedWorker;
    window.SharedWorker=class extends Original {
     constructor(...args){
      super(...args);
      if(slowFeedback){
       const send=this.port.postMessage.bind(this.port);
       const stall={enabled:false,received:0,held:[]};
       window.__feedbackStall=stall;
       stall.release=()=>{
        stall.enabled=false;
        for(const ack of stall.held.splice(0))send(ack);
       };
       this.port.postMessage=(message,...rest)=>{
        if(stall.enabled&&message.type==='feedback-ack')stall.held.push(message);
        else send(message,...rest);
       };
       this.port.addEventListener('message',({data})=>{
        if(data.type==='feedback')stall.received++;
       });
      }
      this.port.addEventListener('message',({data})=>{
       const frame=data?.reply?.feedback;
       if(frame){
        window.__feedbackProbe.push(frame);
        if(window.__feedbackProbe.length>64)window.__feedbackProbe.shift();
        window.__feedbackTrace.push({frame,receivedAt:performance.now()});
        if(window.__feedbackTrace.length>64)window.__feedbackTrace.shift();
       }
      });
     }
    };
   },slowFeedback);
   await page.goto(url.href,{waitUntil:'domcontentloaded'});
   peer=await until(async()=>(await companion('companion_status')).host.clients.find(row=>!existing.has(row.companionId)),Boolean,15000)
    .catch(async error=>{throw new Error('Browser pairing did not finish: '+await page.locator('body').innerText(),{cause:error});});
   const bytes=Buffer.from(JSON.stringify(publication));
   const {uploadId}=await companion('companion_publish_begin',{size:bytes.length});
   for(let offset=0;offset<bytes.length;offset+=8192)await companion('companion_publish_chunk',{uploadId,offset,bytes:[...bytes.subarray(offset,offset+8192)]});
   await companion('companion_publish_commit',{uploadId,saveAsLatest:false});
   await companion('companion_assign',{companionId:peer.companionId,instanceId:id,publicationHash:publication.publicationHash});
   await page.getByText('Connected · panel active',{exact:true}).waitFor({timeout:15000});
   const row=await until(async()=>(await request('profile_resources_list')).profileResources.resources.find(row=>row.attributes?.companionId===peer.companionId),Boolean,10000);
   const details=await request('profile_resources_inspect',{resourceId:row.id});
   const selectedIds=['flight-input-0','feedback:flight-channel-0:flight-output-0'];
   if(qualifyFeedback)selectedIds.push('feedback:flight-channel-1:flight-output-1');
   const selected=details.profileResources.ports.filter(p=>selectedIds.includes(p.descriptor.id));
   assert.equal(selected.length,selectedIds.length);
   const panelValues=pressed=>qualifyFeedback?[pressed,pressed,'local']:[pressed,pressed];
   const document=(await request('profile_create',{name:'Compiled companion activation '+Date.now()})).profiles.document;
   profile=document;
   const ports=selected.map(p=>({...p.descriptor,id:['button','lamp','text'][selectedIds.indexOf(p.descriptor.id)]}));
   document.resources.push({id:'browser-panel',name:'Compiled browser panel',kind:'companion',required:true,ports});
   document.companionAssignments.push({resourceId:'browser-panel',publicationHash:publication.publicationHash});
   document.bindings.push({id:'button-lamp',from:{nodeId:'browser-panel',portId:'button'},to:{nodeId:'browser-panel',portId:'lamp'}});
   if(qualifyFeedback){
    const {contract}=ports.find(port=>port.id==='text');
    document.processingNodes.push({id:'text-constant',name:'Text constant',operation:{kind:'constant',value:'local'},
     inputs:[],outputs:[{id:'out',name:'Text',direction:'output',contract}]});
    document.bindings.push({id:'constant-text',from:{nodeId:'text-constant',portId:'out'},to:{nodeId:'browser-panel',portId:'text'}});
   }
   if(recoveryProbe){
    slot=(await inventory()).find(device=>device.status==='free'&&device.buttons>0&&Array.isArray(device.axes)
     &&!device.discreteHats&&!device.forceFeedback&&device.buttons+device.axes.length+device.continuousHats<=128);
    assert.ok(slot,'A compatible free vJoy slot is required');
    const layout={
     buttons:Array.from({length:slot.buttons},(_,index)=>'button-'+(index+1)),
     axes:slot.axes.map((_,index)=>({portId:'axis-'+(index+1),range:'bipolar',fallback:0})),
     hats:Array.from({length:slot.continuousHats},(_,index)=>'hat-'+(index+1)),
    };
    const draft=await cockpit.evaluate(`(async()=>{
     const {graphSetVirtual,graphConnect}=await import('/src/lib/profile-graph.ts');
     const document=${JSON.stringify(document)};
     const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
     document.resources.push({id:'independent',name:'Independent debug source',kind:'debug-controller',
      required:false,ports:[{id:'button-1',name:'Button',direction:'output',contract}]});
     return graphConnect(graphSetVirtual({document,assignments:[]},
      {id:'recovery-vjoy',name:'Recovery test output',required:true,layout:${JSON.stringify(layout)}}),
      {nodeId:'independent',portId:'button-1'},{nodeId:'recovery-vjoy',portId:'button-1'}).document;
    })()`);
    Object.assign(document,draft);
   }
   if(qualifyHostWake){
    const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
    const port=(id,direction)=>({id,name:id,direction,contract});
    (document.processingNodes??=[]).push({id:'neutral',name:'Neutral',operation:{kind:'constant',value:false},inputs:[],outputs:[port('out','output')]});
    (document.desktopOutputs??=[]).push({id:'keyboard',name:'Keyboard',required:true,scope:{kind:'desktop-wide'},
     ports:[port('hold','input')],actions:[{portId:'hold',action:{kind:'key-hold',key:4}}]});
    document.bindings.push({id:'neutral',from:{nodeId:'neutral',portId:'out'},to:{nodeId:'keyboard',portId:'hold'}});
   }
   const assignments=[{resourceId:'browser-panel',sourceId:row.id,ports:selected.map((p,i)=>({profilePortId:ports[i].id,observedPortId:p.descriptor.id,expectedContractHash:p.contractHash}))}];
   profile=(await request('profile_editor_save',{document,assignments,expectedRevision:document.revision})).profiles.document;
   if(slot)await request('profile_virtual_slot_set',{id:profile.id,expectedRevision:profile.revision,nodeId:'recovery-vjoy',slot:slot.slot});
   const applied=await request('profile_apply',{id:profile.id,expectedRevision:profile.revision,expectedActiveOwner:null,activationId:randomUUID()});
   assert.equal(applied.profiles.profileSession.state,'running');
   if(slot)await until(inventory,devices=>devices.find(device=>device.slot===slot.slot)?.status==='busy',5000);
   await page.getByText('Connected · panel active',{exact:true}).waitFor({timeout:15000})
    .catch(async error=>{throw new Error('Applied panel did not reconnect: '+await page.locator('body').innerText(),{cause:error});});
   const owner=applied.profiles.profileSession.owner;
   if(qualifyHostWake)await assertHostWake('applied');
   let quietStart;
   if(qualifyFeedback){
    await until(()=>page.evaluate(()=>['flight-channel-0','flight-channel-1'].every(channel=>
     window.__feedbackProbe.some(frame=>frame.values.some(value=>value.channelId===channel&&value.status==='fresh')))),Boolean,7000);
    quietStart=await page.evaluate(()=>({frame:window.__feedbackProbe.at(-1),at:performance.now()}));
    assert.match(quietStart.frame.sequence,/^\d+$/,'Feedback must expose its publisher sequence');
    for(const key of ['clientId','sessionId','generation','controllerInstanceId','publicationHash','bindingGeneration']){
     assert.ok(quietStart.frame[key]!=null,`Quiet feedback must pin ${key}`);
    }
   }
   if(qualifyHostWake||qualifyFeedback){
    // Browser heartbeats continue; no gateway reads or authored input during
    // this interval, which spans several desktop output lease periods.
    await delay(1100);
    if(qualifyFeedback){
     const quiet=await page.evaluate(start=>{
      const current=['flight-channel-0','flight-channel-1'].map(channel=>{
       const latest=window.__feedbackTrace.findLast(item=>item.frame.values.some(value=>value.channelId===channel));
       return latest&&{sample:latest.frame.values.find(value=>value.channelId===channel),receivedAt:latest.receivedAt};
      });
      return {frames:window.__feedbackTrace.filter(item=>item.receivedAt>=start),at:performance.now(),current};
     },quietStart.at);
     if(!quiet.frames.length){
      const status=(await request('profile_status')).profiles.profileSession;
      const host=(await companion('companion_status')).host.instances.find(row=>row.instanceId===id);
      assert.fail(`Quiet feedback did not renew: ${JSON.stringify({quiet,status,connected:host?.connected,armed:host?.armed,panel:await page.locator('body').innerText()})}`);
     }
     const pinned=quietStart.frame;
     for(const {frame} of quiet.frames){
      for(const key of ['clientId','sessionId','generation','controllerInstanceId','publicationHash','bindingGeneration']){
       assert.equal(frame[key],pinned[key],`Quiet feedback changed its pinned ${key}`);
      }
      for(const sample of frame.values)assert.equal(sample.status,'fresh',`Quiet feedback withdrew ${sample.channelId}`);
     }
     const after=quiet.frames.at(-1).frame.sequence;
     const difference=BigInt(after)-BigInt(pinned.sequence);
     const quietMs=Math.round(quiet.at-quietStart.at);
     t.diagnostic(JSON.stringify({qualification:'companion-feedback',before:pinned.sequence,after,
      difference:difference.toString(),eligibleFrames:quiet.frames.length,quietMs,
      appHash:process.env.OSG_COMPANION_FEEDBACK_APP_HASH??process.env.OSG_COMPANION_HOST_WAKE_APP_HASH??null}));
     assert.ok(difference>=0n&&difference<=12n,
      `Quiet publisher sequence advanced ${difference} times in ${quietMs}ms; expected at most 12`);
     for(const current of quiet.current){
      assert.equal(current?.sample.status,'fresh','A quiet connection must retain current widget feedback');
      assert.ok(current.sample.sourceAgeMs+quiet.at-current.receivedAt<1000,
       'A low publication count must not hide an expired widget');
     }
    }
    if(qualifyHostWake)await assertHostWake('quiet-1100ms');
   }
   await page.getByRole('button',{name:'probe: press',exact:true}).click();
   await until(()=>page.evaluate(()=>window.__feedbackProbe.some(frame=>frame.values.some(value=>
    value.channelId==='flight-channel-0'&&value.outputId==='flight-output-0'
    &&value.value===true&&value.status==='fresh'&&value.provenance==='local'&&value.sourceKind==='host'))),Boolean,7000);
   const inspection=(await request('profile_runtime_inspect',{owner,nodeId:'browser-panel'})).profileInspection;
   assert.deepEqual(inspection.ports.map(port=>port.sample.value.value),panelValues(true));
   const readInstance=async()=>(await companion('companion_status')).host.instances.find(row=>row.instanceId===id);
   const before=await readInstance();
   assert.equal(before.values['flight-input-0'],true);
   assert.equal(before.profileOwned,true);
   const streamId=randomUUID();
   const independent=(sequence,value)=>request('profile_debug_controller_input',{owner,nodeId:'independent',
    streamId,sequence:String(sequence),changes:[{portId:'button-1',value}]});
   const probeLines=[];
   let probeDone;
   const assertRawTransitions=async expected=>{
    assert.equal((await probeDone)[0],0);
    const events=JSON.parse(probeLines.at(-1)).buttonEvents.filter(event=>event.report===slot.slot);
    assert.ok(events.every(event=>event.status===0x00110000));
    assert.equal(new Set(events.map(event=>event.device)).size,1);
    assert.deepEqual(events.map(event=>event.pressed),expected);
   };
   if(slot){
    probe=spawn(recoveryProbe,['--vjoy-button'],{windowsHide:true});
    const lines=createInterface({input:probe.stdout});
    probeDone=once(probe,'close');
    const [line]=await Promise.race([once(lines,'line'),probeDone.then(()=>{throw Error('vJoy probe exited before registration');})]);
    assert.equal(line,'ready');
    lines.on('line',value=>probeLines.push(value));
    await independent(1,true);
   }
   if(slot){
    const lossStarted=performance.now();
    await page.goto('about:blank',{waitUntil:'domcontentloaded'});
    await until(readInstance,row=>row&&!row.connected&&!row.armed,3000);
    await until(async()=>{
     const {profileInspection}=await request('profile_runtime_inspect',{owner,nodeId:'browser-panel'});
     return profileInspection.ports.length===selectedIds.length&&profileInspection.ports.every(port=>port.sample.value?.value!==true);
    },Boolean,1000);
    assert.equal((await inventory()).find(device=>device.slot===slot.slot)?.status,'busy');
    t.diagnostic(`Browser loss and affected contribution withdrawal observed within ${Math.round(performance.now()-lossStarted)} ms`);
    await page.goto(url.href,{waitUntil:'domcontentloaded'});
   }else{
    await page.reload({waitUntil:'domcontentloaded'});
   }
   await page.getByText('Connected · panel active',{exact:true}).waitFor({timeout:15000});
   await until(readInstance,row=>row?.connected&&row.armed&&row.values['flight-input-0']===false,15000);
   await until(async()=>{
    const {profileInspection}=await request('profile_runtime_inspect',{owner,nodeId:'browser-panel'});
    const expected=panelValues(false);
    return profileInspection.ports.length===expected.length&&profileInspection.ports.every((port,index)=>port.sample.value?.value===expected[index]);
   },Boolean,10000);
   if(qualifyHostWake)await assertHostWake('reloaded-neutral-recovery');
   assert.deepEqual((await request('profile_status')).profiles.profileSession.owner,owner,
    'Reloading the browser must retain the applied owner');
   assert.equal(await page.evaluate(()=>window.__feedbackProbe.some(frame=>frame.values.some(value=>
    value.channelId==='flight-channel-0'&&value.outputId==='flight-output-0'&&value.status==='fresh'&&value.value===true))),false,
    'The reloaded browser must not receive the previous held feedback as fresh');
   let stalledAt;
   if(slowFeedback){
    await page.evaluate(()=>{window.__feedbackStall.enabled=true;});
    stalledAt=await until(()=>page.evaluate(()=>{
     const stall=window.__feedbackStall;
     return stall.held.length===1?stall.received:null;
    }),Boolean,2500);
   }
   await page.getByRole('button',{name:'probe: press',exact:true}).click();
   if(slowFeedback){
    await until(async()=>{
     const {profileInspection}=await request('profile_runtime_inspect',{owner,nodeId:'browser-panel'});
     return profileInspection.ports[0]?.sample.value?.value===true;
    },Boolean,1000);
    await independent(2,false);
    await delay(100);
    await independent(3,true);
    await delay(1500);
    // The probe must finish before feedback resumes, so the Windows reports
    // prove native dispatch progressed during the stall.
    await assertRawTransitions([true,false,true]);
    const stalled=await page.evaluate(()=>{
     const {held,received}=window.__feedbackStall;
     return {held:held.length,received};
    });
    assert.deepEqual(stalled,{held:1,received:stalledAt},'A held receipt must bound browser feedback to one in-flight event');
    const peerState=await readInstance();
    assert.ok(peerState.connected&&peerState.armed,'Slow feedback must preserve the input lease');
    assert.deepEqual((await request('profile_status')).profiles.profileSession.owner,owner);
    assert.equal((await inventory()).find(device=>device.slot===slot.slot)?.status,'busy');
    await page.evaluate(()=>window.__feedbackStall.release());
    await until(()=>page.evaluate(()=>window.__feedbackProbe.some(frame=>frame.mode==='snapshot'&&
     frame.values.some(value=>value.channelId==='flight-channel-0'&&value.status==='fresh'&&value.value===true))),Boolean,3000);
    t.diagnostic('Feedback acknowledgements stalled for more than one renewal and a full input lease');
   }
   await until(()=>page.evaluate(()=>window.__feedbackProbe.some(frame=>frame.values.some(value=>
    value.channelId==='flight-channel-0'&&value.outputId==='flight-output-0'&&value.status==='fresh'&&value.value===true))),Boolean,7000);
   const rearmed=await request('profile_runtime_inspect',{owner,nodeId:'browser-panel'});
   assert.deepEqual(rearmed.profileInspection.ports.map(port=>port.sample.value.value),panelValues(true));
   if(slot&&!slowFeedback){
    await independent(2,false);
    await delay(100);
    await independent(3,true);
    await delay(100);
   }
   const stopStarted=performance.now();
   const stopped=await request('profile_stop');
   assert.equal(stopped.profiles.profileSession.state,'stopped');
   assert.equal(stopped.profiles.profileSession.owner,null);
   if(slot){
    await until(inventory,devices=>devices.find(device=>device.slot===slot.slot)?.status==='free',1000);
    t.diagnostic(`Stop and vJoy slot release observed within ${Math.round(performance.now()-stopStarted)} ms`);
    if(!slowFeedback)await assertRawTransitions([true,false,true,false]);
   }
  } finally {
   const cleanupErrors=[];
   const cleanup=async work=>{try{await work();}catch(error){if(qualifyHostWake||qualifyFeedback||recoveryProbe)cleanupErrors.push(error);}};
   let stopped=false;
   if(profile||desktopPreferences)await cleanup(async()=>{
    const result=await request('profile_stop');
    assert.equal(result.profiles.profileSession.state,'stopped');
    assert.equal(result.profiles.profileSession.owner,null);
    stopped=true;
   });
   if(profile)await cleanup(()=>request('profile_delete',{id:profile.id,expectedRevision:profile.revision}));
   if(desktopPreferences)await cleanup(async()=>{
    assert.equal(stopped,true,'Stop must finish before restoring desktop settings');
    await request('profile_desktop_configure',{enabled:desktopPreferences.enabled,scope:desktopPreferences.scope});
   });
   if(peer)await cleanup(()=>companion('companion_unassign',{instanceId:id}));
   if(peer)await cleanup(()=>companion('companion_revoke',{companionId:peer.companionId}));
   if(probe&&probe.exitCode===null&&probe.signalCode===null)probe.kill();
   try{await browser?.close();}finally{cockpit.close();}
   if(cleanupErrors.length)throw new AggregateError(cleanupErrors,'Companion native qualification cleanup failed');
  }

});
