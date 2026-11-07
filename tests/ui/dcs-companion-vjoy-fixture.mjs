// One applied profile owns authored DCS telemetry, a real browser panel and vJoy.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHash, randomUUID, X509Certificate} from 'node:crypto';
import {once} from 'node:events';
import {readFile} from 'node:fs/promises';
import {createServer} from 'node:net';
import {createRequire} from 'node:module';
import {createInterface} from 'node:readline';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from 'node:tls';
import {until} from '../../scripts/reactive/cdp.mjs';

const panelNode = 'combined-browser-panel';
const debugNode = 'combined-debug';
const outputNode = 'combined-vjoy';

export function combinedDcsFixture(cdp, request, probeExecutable) {
  const companion = async (command, params={}) =>
    (await request({command, companion:params})).companions;
  const listener = async () => (await companion('companion_inventory')).listener;
  const inventory = async () => (await request({command:'system_inventory'})).systemInventory.virtualDevices;
  let browser, page, peer, slot, probe, report, probeStart, probeReady, listenerEnabled=false;
  const samples=()=>page.evaluate(()=>window.__combinedLamp);
  const waitLamp=(offset,predicate)=>until(async()=>(await samples()).slice(offset).find(predicate))
    .catch(async error=>{throw new Error('Panel feedback did not arrive: '+JSON.stringify((await samples()).slice(-5)),{cause:error});});

  async function connectPanel() {
    const current=await listener();
    assert.equal(current.configuredEnabled,false,'Use a disposable Cockpit with its listener disabled');
    const reservation=createServer();
    await new Promise((resolve,reject)=>{
      reservation.once('error',reject);
      reservation.listen(0,'127.0.0.1',resolve);
    });
    const {port}=reservation.address();
    await new Promise(resolve=>reservation.close(resolve));
    await companion('companion_enable',{address:'127.0.0.1',advertisedHost:'127.0.0.1',port,
      discovery:false,expectedSettingsRevision:current.settingsRevision});
    listenerEnabled=true;
    const {host}=await companion('companion_status');
    const before=new Set(host.clients.map(row=>row.companionId));
    const offer=await companion('companion_pair_offer',{kind:'browser'});
    const url=new URL(offer.pairing.browserUrl);
    assert.equal(url.hostname,'127.0.0.1');
    const pin=await new Promise((resolve,reject)=>{
      const socket=connect({host:url.hostname,port:Number(url.port),rejectUnauthorized:false},()=>{
        try {
          const cert=new X509Certificate(socket.getPeerCertificate().raw);
          resolve(createHash('sha256').update(cert.publicKey.export({type:'spki',format:'der'})).digest('base64'));
        } catch(error){reject(error);} finally {socket.destroy();}
      });
      socket.once('error',reject);
    });
    const require=createRequire(import.meta.url);
    const {chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');
    browser=await chromium.launch({channel:process.env.OSG_BROWSER_CHANNEL??'msedge',headless:true,
      args:['--ignore-certificate-errors-spki-list='+pin]});
    page=await browser.newPage();
    await page.addInitScript(()=>{
      window.__combinedLamp=[];
      const Worker=window.SharedWorker;
      window.SharedWorker=class extends Worker {
        constructor(...args){
          super(...args);
          // The production SharedWorker owns WSS. Observe its deliveries
          // to the real panel without replacing traffic or acknowledging it.
          this.port.addEventListener('message',({data})=>{
            const feedback=data?.reply?.feedback;
            const sample=feedback?.values?.find(row=>row.channelId==='flight-channel-0'
              &&row.outputId==='flight-output-0');
            if(sample&&window.__combinedLamp.length<512)window.__combinedLamp.push({
              sequence:feedback.sequence,status:sample.status,value:sample.value,sourceKind:sample.sourceKind,
            });
          });
        }
      };
    });
    await page.goto(url.href,{waitUntil:'domcontentloaded'});
    peer=await until(async()=>(await companion('companion_status')).host.clients
      .find(row=>!before.has(row.companionId)));
    const bytes=await readFile(new URL('../../scripts/fixtures/compiled-flight-publication.json',import.meta.url));
    const publication=JSON.parse(bytes);
    const {uploadId}=await companion('companion_publish_begin',{size:bytes.length});
    for(let offset=0;offset<bytes.length;offset+=8192){
      await companion('companion_publish_chunk',{uploadId,offset,bytes:[...bytes.subarray(offset,offset+8192)]});
    }
    await companion('companion_publish_commit',{uploadId,saveAsLatest:false});
    return publication.publicationHash;
  }

  async function prepare(document, assignments) {
    const publicationHash=await connectPanel();
    const resource=await until(async()=>(await request({command:'profile_resources_list',limit:128}))
      .profileResources.resources.find(row=>row.attributes?.companionId===peer.companionId
        &&row.attributes.componentKind==='paired-companion'));
    const {ports}=(await request({command:'profile_companion_inspect',resourceId:resource.id,
      publicationHash,offset:0,limit:64})).profileCompanionAuthoring;
    const selected=['flight-input-0','feedback:flight-channel-0:flight-output-0'].map(id=>{
      const port=ports.find(row=>row.descriptor.id===id);
      assert.ok(port,`Missing panel port ${id}`);
      return port;
    });
    await companion('companion_assign',{companionId:peer.companionId,instanceId:'combined-'+randomUUID(),publicationHash});
    await page.getByText('Connected · panel active',{exact:true}).waitFor();
    await until(async()=>{
      const current=(await request({command:'profile_resources_inspect',resourceId:resource.id,limit:64}))
        .profileResources.ports;
      return selected.every(port=>current.some(live=>live.current&&live.descriptor.id===port.descriptor.id
        &&live.contractHash===port.contractHash));
    });
    const mapped=selected.map((port,index)=>({...port.descriptor,id:['press','lamp'][index]}));
    document.resources.push({id:panelNode,name:'Combined browser panel',kind:'companion',required:false,ports:mapped});
    document.companionAssignments.push({resourceId:panelNode,publicationHash});
    document.bindings.push({id:'combined-dcs-lamp',from:{nodeId:'telemetry',portId:'flight.ownship.inAir'},
      to:{nodeId:panelNode,portId:'lamp'}});
    assignments.push({resourceId:panelNode,sourceId:resource.id,ports:selected.map((port,index)=>({
      profilePortId:mapped[index].id,observedPortId:port.descriptor.id,expectedContractHash:port.contractHash,
    }))});
    slot=await until(async()=>(await inventory()).find(device=>device.status==='free'&&device.buttons>=2
      &&Array.isArray(device.axes)&&!device.discreteHats&&!device.forceFeedback
      &&device.buttons+device.axes.length+device.continuousHats<=128));
    const layout={buttons:Array.from({length:slot.buttons},(_,index)=>'button-'+(index+1)),
      axes:slot.axes.map((_,index)=>({portId:'axis-'+(index+1),range:'bipolar',fallback:0})),
      hats:Array.from({length:slot.continuousHats},(_,index)=>'hat-'+(index+1))};
    const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
    document.resources.push({id:debugNode,name:'Combined debug source',kind:'debug-controller',required:false,
      ports:[{id:'button-1',name:'Button',direction:'output',contract}]});
    const draft=await cdp.evaluate(`(async()=>{
      const {graphSetVirtual,graphConnect}=await import('/src/lib/profile-graph.ts');
      let draft=graphSetVirtual(${JSON.stringify({document,assignments})},${JSON.stringify({
        id:outputNode,name:'Combined test output',required:true,layout,
      })});
      draft=graphConnect(draft,{nodeId:'${debugNode}',portId:'button-1'},{nodeId:'${outputNode}',portId:'button-1'});
      return graphConnect(draft,{nodeId:'${panelNode}',portId:'press'},{nodeId:'${outputNode}',portId:'button-2'});
    })()`);
    Object.assign(document,draft.document);
  }

  async function bind(profile) {
    await request({command:'profile_virtual_slot_set',id:profile.id,expectedRevision:profile.revision,
      nodeId:outputNode,slot:slot.slot});
  }

  const expected=Array.from({length:65},(_,index)=>index%2===0);
  async function burst(owner, {setAirborne,setPaused}) {
    await page.getByText('Connected · panel active',{exact:true}).waitFor();
    await waitLamp(0,sample=>sample.status==='fresh'&&sample.value===false);
    probeStart=process.hrtime.bigint();
    probe=spawn(probeExecutable,['--vjoy-buttons'],{windowsHide:true});
    const lines=createInterface({input:probe.stdout});
    const started=once(lines,'line'), trace=[];
    report=(async()=>{
      const [code]=await once(probe,'close');
      assert.equal(code,0);
      return JSON.parse(trace.at(-1));
    })();
    // Keep the child result owned even if an earlier assertion aborts the test.
    void report.catch(()=>{});
    lines.on('line',line=>{if(line!=='ready')trace.push(line);});
    assert.deepEqual(await Promise.race([started,report.then(()=>{throw Error('Raw Input probe exited before readiness');})]),['ready']);
    probeReady=process.hrtime.bigint();
    const mark=(await samples()).length;
    setAirborne(true);
    const active=await waitLamp(mark,sample=>sample.status==='fresh'&&sample.value===true);
    await page.getByRole('button',{name:'probe: press',exact:true}).click();
    const beforePause=(await samples()).length;
    setPaused(true);
    const streamId=randomUUID(), start=performance.now();
    for(const [index,value] of expected.entries()){
      const remaining=start+index*8-performance.now();
      if(remaining>0)await delay(remaining);
      const sequence=String(index+1);
      const {debugController}=await request({command:'profile_debug_controller_input',owner,
        nodeId:debugNode,streamId,sequence,changes:[{portId:'button-1',value}]});
      assert.equal(debugController.sequence,sequence);
    }
    const paused=await waitLamp(beforePause,sample=>sample.status!=='fresh');
    const beforeResume=(await samples()).length;
    setPaused(false);
    const resumed=await waitLamp(beforeResume,sample=>sample.status==='fresh'&&sample.value===true);
    const after=(await samples()).length;
    setAirborne(false);
    const inactive=await waitLamp(after,sample=>sample.status==='fresh'&&sample.value===false);
    return {active,paused,resumed,inactive,authoredTransitions:expected.length,elapsedMs:performance.now()-start};
  }

  async function stopped(stopNs) {
    await until(inventory,rows=>rows.find(row=>row.slot===slot.slot)?.status==='free');
    const raw=await report;
    const events=raw.buttonEvents.filter(event=>event.report===slot.slot);
    assert.equal(new Set(events.map(event=>event.device)).size,1);
    assert.ok(events.every(event=>event.status===0x00110000));
    const edges=field=>events.filter((event,index)=>event[field]!==(events[index-1]?.[field]??false));
    const debug=edges('pressed'), panel=edges('peerPressed');
    assert.deepEqual(debug.map(event=>event.pressed),[...expected,false]);
    assert.deepEqual(panel.map(event=>event.peerPressed),[true,false]);
    const hz=BigInt(raw.receiverTickHz), start=BigInt(raw.receiverStartedTicks);
    const qpcNs=ticks=>ticks*1_000_000_000n/hz;
    assert.ok(qpcNs(start)>=probeStart&&qpcNs(start)<=probeReady,'Raw Input and Node must share QPC');
    const stopReleaseMs=[debug.at(-1),panel.at(-1)].map(event=>Number(
      qpcNs(start+BigInt(event.receivedTicks))-stopNs)/1e6);
    assert.ok(stopReleaseMs.every(ms=>ms>=0),'Both buttons must stay held until Stop');
    return {slot:slot.slot,debugTransitions:expected.length,panelTransitions:1,stopReleaseMs,slotFree:true};
  }

  async function close() {
    probe?.kill();
    try {await browser?.close();} finally {
      try {if(peer)await companion('companion_revoke',{companionId:peer.companionId});} finally {
        if(listenerEnabled)await companion('companion_disable',{expectedSettingsRevision:(await listener()).settingsRevision});
      }
    }
  }
  return {prepare,bind,burst,stopped,close};
}
