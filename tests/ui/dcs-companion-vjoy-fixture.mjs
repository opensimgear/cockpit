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
  const streamId=randomUUID();
  let sequence=0, closedPreviewAt, stopTrace, previewReads=0, previewOpens=0, previewError;
  const previewPending=new Map(), previewActive=new Map(), previewRetired=new Set();
  async function sendDebug(owner,value) {
    const sent=String(++sequence);
    const {debugController}=await request({command:'profile_debug_controller_input',owner,
      nodeId:debugNode,streamId,sequence:sent,changes:[{portId:'button-1',value}]});
    assert.equal(debugController.sequence,sent);
  }

  async function openInspector() {
    await cdp.call('Network.enable',{maxPostDataSize:65536});
    stopTrace=cdp.listen(({method,params})=>{
      const id=params?.requestId;
      if(method==='Network.requestWillBeSent'&&params.request.postData){
        const path=new URL(params.request.url).pathname;
        if(!['/gateway_request','/observation_subscribe','/observation_unsubscribe'].includes(path))return;
        const body=JSON.parse(params.request.postData);
        if(path==='/gateway_request'&&['game_telemetry_inspect','profile_runtime_inspect'].includes(body.request?.command))previewReads++;
        if(path==='/observation_subscribe'&&/^(games\.telemetry$|profiles\.samples\.|controllers\.input\.)/.test(body.topic)){
          previewOpens++;
          previewPending.set(id,{topic:body.topic});
        }else if(path==='/observation_unsubscribe')previewPending.set(id,{token:body.subscriptionId});
      }
      const pending=previewPending.get(id);
      if(!pending)return;
      if(method==='Network.responseReceived')pending.status=params.response.status;
      if(method!=='Network.loadingFinished'&&method!=='Network.loadingFailed')return;
      if(pending.topic){
        void cdp.call('Network.getResponseBody',{requestId:id}).then(({body})=>{
          assert.equal(pending.status,200);
          const token=JSON.parse(body);
          assert.equal(typeof token,'string');
          if(!previewRetired.has(token))previewActive.set(token,pending.topic);
        }).catch(error=>previewError=error).finally(()=>previewPending.delete(id));
      }else{
        if(pending.status===200){previewRetired.add(pending.token);previewActive.delete(pending.token);}
        previewPending.delete(id);
      }
    });
    await cdp.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="Debug"]').click()`);
    await until(async()=>{
      if(previewError)throw previewError;
      return [...previewActive.values()].includes('games.telemetry') && await cdp.evaluate(`(()=>{
        const table=document.querySelector('[aria-label="Incoming telemetry channel values"]');
        return !!table && !table.closest('[hidden]');
      })()`);
    }).catch(async error=>{
      const inspector=await cdp.evaluate(`document.querySelector('.game-telemetry')?.innerText.slice(0,500)`);
      throw new Error(`Inspector did not open. Topics: ${[...previewActive.values()]}. View: ${inspector}`,{cause:error});
    });
  }

  async function withPreviewsClosed(owner,setAirborne) {
    await cdp.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="Overview"]').click()`);
    await until(async()=>{
      if(previewError)throw previewError;
      return previewPending.size===0 && previewActive.size===0 && await cdp.evaluate(`(()=>{
        const inspector=document.querySelector('[aria-label="Incoming game telemetry"]');
        return !inspector || !!inspector.closest('[hidden]');
      })()`);
    });
    const before=[previewOpens,previewReads], offset=(await samples()).length;
    closedPreviewAt=process.hrtime.bigint();
    setAirborne(true);
    await sendDebug(owner,false);
    const lamp=await waitLamp(offset,sample=>sample.status==='fresh'&&sample.value===true);
    await sendDebug(owner,true);
    assert.deepEqual([previewOpens,previewReads],before,'Closed previews must not subscribe or read while native routes run');
    return {lamp,retiredReaders:before[0],previewOpens:0,previewReads:0};
  }

  async function burst(owner, {setAirborne,setPaused}) {
    await page.getByText('Connected · panel active',{exact:true}).waitFor();
    await waitLamp(0,sample=>sample.status==='fresh'&&sample.value===false);
    probeStart=process.hrtime.bigint();
    probe=spawn(probeExecutable,['--vjoy-buttons'],{windowsHide:true});
    const lines=createInterface({input:probe.stdout});
    const started=once(lines,'line'), trace=[];
    let probeError='';
    probe.stderr.setEncoding('utf8').on('data',chunk=>{probeError=(probeError+chunk).slice(-4096);});
    report=(async()=>{
      const [code]=await once(probe,'close');
      assert.equal(code,0,probeError);
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
    const start=performance.now();
    for(const [index,value] of expected.entries()){
      const remaining=start+index*8-performance.now();
      if(remaining>0)await delay(remaining);
      await sendDebug(owner,value);
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
    assert.deepEqual(debug.map(event=>event.pressed),[...expected,false,true,false]);
    assert.deepEqual(panel.map(event=>event.peerPressed),[true,false]);
    const hz=BigInt(raw.receiverTickHz), start=BigInt(raw.receiverStartedTicks);
    const qpcNs=ticks=>ticks*1_000_000_000n/hz;
    assert.ok(qpcNs(start)>=probeStart&&qpcNs(start)<=probeReady,'Raw Input and Node must share QPC');
    const closedPreviewEdges=debug.slice(expected.length,-1);
    assert.equal(closedPreviewEdges.length,2);
    assert.ok(closedPreviewEdges.every(event=>qpcNs(start+BigInt(event.receivedTicks))>=closedPreviewAt));
    const stopReleaseMs=[debug.at(-1),panel.at(-1)].map(event=>Number(
      qpcNs(start+BigInt(event.receivedTicks))-stopNs)/1e6);
    assert.ok(stopReleaseMs.every(ms=>ms>=0),'Both buttons must stay held until Stop');
    return {slot:slot.slot,debugTransitions:expected.length,closedPreviewTransitions:closedPreviewEdges.length,
      panelTransitions:1,stopReleaseMs,slotFree:true};
  }

  async function close() {
    probe?.kill();
    stopTrace?.();
    try {await browser?.close();} finally {
      try {if(peer)await companion('companion_revoke',{companionId:peer.companionId});} finally {
        if(listenerEnabled)await companion('companion_disable',{expectedSettingsRevision:(await listener()).settingsRevision});
      }
    }
  }
  return {prepare,bind,openInspector,burst,withPreviewsClosed,stopped,close};
}
