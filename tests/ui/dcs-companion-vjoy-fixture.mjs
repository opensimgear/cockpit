// One applied profile owns authored DCS telemetry, paired browser panels and vJoy.
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
import {measurePanelPresentation,tracePanelCanvas} from './dcs-panel-presentation-timing.mjs';
import {airborneChannel,withInspectorPresentation} from './dcs-inspector-presentation.mjs';
import {mcpVjoyObservation} from './mcp-vjoy-observation.mjs';
import {createNativeRateBurst,extendedNativeRates} from './dcs-native-rate-burst.mjs';
import {commandPanelPublication,nativeDcsCommands} from './dcs-native-command-fixture.mjs';
import {runDcsSoak} from './dcs-feature-soak.mjs';
import {startDualRateSoak} from './dcs-dual-rate-soak.mjs';
import {installSoakPanel} from './dcs-soak-panel.mjs';
import {dualRateLanes,dualRateResults,submitBrowserRate,summarizeMilliseconds} from './vjoy-rate-transport.mjs';

const panelNode = 'combined-browser-panel';
const debugNode = 'combined-debug';
const outputNode = 'combined-vjoy';
export function combinedDcsFixture(cdp, request, probeExecutable, {browserUrl, observeMcp=false, nativeRates=false, seconds=0}={}) {
  const inspectorTiming=process.env.OSG_DCS_INSPECTOR_PRESENTATION==='1';
  if(inspectorTiming)assert.ok(nativeRates&&extendedNativeRates&&!seconds,'Inspector timing requires the extended native dual-rate burst');
  const companion = async (command, params={}) =>
    (await request({command, companion:params})).companions;
  const listener = async () => (await companion('companion_inventory')).listener;
  const inventory = async () => (await request({command:'system_inventory'})).systemInventory.virtualDevices;
  let browser, page, slot, probe, report, probeStart, probeReady, listenerEnabled=false, dualSubmissions, presentation;
  let observation, nativeSubmissions, dualRateSoak;
  const commands=process.env.OSG_DCS_COMMANDS_NATIVE==='1'?nativeDcsCommands(cdp,request):null;
  const paired=Boolean(browserUrl || observeMcp);
  const panels=[];
  const samples=(target=page)=>target.evaluate(()=>window.__combinedLamp);
  const waitLamp=(offset,predicate,target=page)=>until(async()=>(await samples(target)).slice(offset).find(predicate))
    .catch(async error=>{throw new Error('Panel feedback did not arrive: '+JSON.stringify((await samples(target)).slice(-5)),{cause:error});});

  async function enableListener() {
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
  }

  async function connectPanel() {
    const {host}=await companion('companion_status');
    const before=new Set(host.clients.map(row=>row.companionId));
    const offer=await companion('companion_pair_offer',{kind:'browser'});
    const url=new URL(offer.pairing.browserUrl);
    assert.equal(url.hostname,'127.0.0.1');
    if(!browser){
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
        args:['--ignore-certificate-errors-spki-list='+pin,
          // The test's two headless contexts represent separately visible panels.
          ...(paired?['--disable-background-timer-throttling','--disable-renderer-backgrounding','--disable-backgrounding-occluded-windows']:[])]});
    }
    const panel=await browser.newPage();
    await tracePanelCanvas(panel);
    if(seconds)await installSoakPanel(panel);
    await panel.addInitScript(()=>{
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
              sourceAgeMs:sample.sourceAgeMs,receivedAt:performance.now(),
            });
          });
        }
      };
    });
    await panel.goto(url.href,{waitUntil:'domcontentloaded'});
    const peer=await until(async()=>(await companion('companion_status')).host.clients
      .find(row=>!before.has(row.companionId)));
    const connected={page:panel,peer};
    panels.push(connected);
    return connected;
  }

  async function publishPanel() {
    const original=await readFile(new URL('../../scripts/fixtures/compiled-flight-timing-publication.json',import.meta.url));
    const publication=commands?await commandPanelPublication(JSON.parse(original)):JSON.parse(original);
    const bytes=commands?Buffer.from(JSON.stringify(publication)):original;
    const {uploadId}=await companion('companion_publish_begin',{size:bytes.length});
    for(let offset=0;offset<bytes.length;offset+=8192){
      await companion('companion_publish_chunk',{uploadId,offset,bytes:[...bytes.subarray(offset,offset+8192)]});
    }
    await companion('companion_publish_commit',{uploadId,saveAsLatest:false});
    return publication.publicationHash;
  }

  async function assignPanel(document, assignments, publicationHash, {page,peer}, nodeId) {
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
    document.resources.push({id:nodeId,name:'Combined browser panel '+panels.length,kind:'companion',required:false,ports:mapped});
    document.companionAssignments.push({resourceId:nodeId,publicationHash});
    document.bindings.push({id:nodeId+'-lamp',from:{nodeId:'telemetry',portId:'flight.ownship.inAir'},
      to:{nodeId,portId:'lamp'}});
    assignments.push({resourceId:nodeId,sourceId:resource.id,ports:selected.map((port,index)=>({
      profilePortId:mapped[index].id,observedPortId:port.descriptor.id,expectedContractHash:port.contractHash,
    }))});
  }

  async function prepare(document, assignments, aircraft) {
    await enableListener();
    const first=await connectPanel();
    page=first.page;
    const publicationHash=await publishPanel();
    await assignPanel(document,assignments,publicationHash,first,panelNode);
    if(paired)await assignPanel(document,assignments,publicationHash,await connectPanel(),panelNode+'-peer');
    slot=await until(async()=>(await inventory()).find(device=>device.status==='free'&&device.buttons>=2
      &&Array.isArray(device.axes)&&!device.discreteHats&&!device.forceFeedback
      &&device.buttons+device.axes.length+device.continuousHats<=128));
    const layout={buttons:Array.from({length:slot.buttons},(_,index)=>'button-'+(index+1)),
      axes:slot.axes.map((_,index)=>({portId:'axis-'+(index+1),range:'bipolar',fallback:0})),
      hats:Array.from({length:slot.continuousHats},(_,index)=>'hat-'+(index+1))};
    const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
    const sources=browserUrl||nativeRates?dualRateLanes.map(lane=>lane.nodeId):[debugNode,panelNode];
    for(const id of browserUrl||nativeRates?sources:[debugNode])document.resources.push({id,name:'Combined debug '+id,kind:'debug-controller',required:false,
      ports:[{id:'button-1',name:'Button',direction:'output',contract}]});
    const draft=await cdp.evaluate(`(async()=>{
      const {graphSetVirtual,graphConnect}=await import('/src/lib/profile-graph.ts');
      let draft=graphSetVirtual(${JSON.stringify({document,assignments})},${JSON.stringify({
        id:outputNode,name:'Combined test output',required:true,layout,
      })});
      draft=graphConnect(draft,{nodeId:'${sources[0]}',portId:'button-1'},{nodeId:'${outputNode}',portId:'button-1'});
      return graphConnect(draft,{nodeId:'${sources[1]}',portId:'${browserUrl||nativeRates?'button-1':'press'}'},{nodeId:'${outputNode}',portId:'button-2'});
    })()`);
    Object.assign(document,draft.document);
    await commands?.attach(document,assignments,aircraft);
  }

  async function bind(profile) {
    await request({command:'profile_virtual_slot_set',id:profile.id,expectedRevision:profile.revision,
      nodeId:outputNode,slot:slot.slot});
  }

  async function measurePresentation(owner,submitOwnship) {
    await Promise.all(panels.map(panel=>waitLamp(0,sample=>sample.status==='fresh'&&sample.value===false,panel.page)));
    if(observeMcp&&!observation)observation=await mcpVjoyObservation(cdp,owner,{nodeId:outputNode,capturePhysical:false});
    presentation=await measurePanelPresentation(paired?panels.map(panel=>panel.page):page,submitOwnship);
    return presentation;
  }

  const expected=Array.from({length:65},(_,index)=>index%2===0);
  const streamId=randomUUID();
  let sequence=0, closedPreviewAt, stopTrace, previewReads=0, previewOpens=0, previewError;
  let inspectorSubscription, inspectorReads=0, unscopedInspectorReads=0;
  const previewPending=new Map(), previewActive=new Map(), previewRetired=new Set();
  async function sendDebug(owner,value) {
    const sent=String(++sequence);
    const {debugController}=await request({command:'profile_debug_controller_input',owner,
      nodeId:debugNode,streamId,sequence:sent,changes:[{portId:'button-1',value}]});
    assert.equal(debugController.sequence,sent);
    return debugController.pendingFrames;
  }

  async function soak(options) {
    const openMcp=async()=>observation=await mcpVjoyObservation(cdp,options.owner,{nodeId:outputNode,
      capturePhysical:false,streamTimeoutMs:seconds*1000+120000});
    const beginDualRates=nativeRates?async()=>dualRateSoak=await startDualRateSoak(cdp,probeExecutable,slot.slot,
      options.owner,options.directory,seconds):undefined;
    const evidence=await runDcsSoak({cdp,request,panels,seconds,sendDebug,openMcp,beginDualRates,nodeId:outputNode,...options});
    await Promise.all(panels.map(({page})=>page.evaluate(()=>{
      const baseline=window.__combinedLamp.findLast(sample=>sample.status==='fresh'&&sample.value===false);
      window.__combinedLamp.splice(0,window.__combinedLamp.length,...(baseline?[baseline]:[]));
      window.__panelPaints.length=0;
    })));
    return evidence;
  }

  async function openInspector() {
    await cdp.call('Network.enable',{maxPostDataSize:65536});
    stopTrace=cdp.listen(({method,params})=>{
      const id=params?.requestId;
      if(method==='Network.requestWillBeSent'&&params.request.postData){
        const path=new URL(params.request.url).pathname;
        const gateway=browserUrl?'/api/gateway/':'/gateway_request';
        const subscribe=browserUrl?'/api/observe/subscribe':'/observation_subscribe';
        const unsubscribe=browserUrl?'/api/observe/unsubscribe':'/observation_unsubscribe';
        if(![gateway,subscribe,unsubscribe].includes(path))return;
        const body=JSON.parse(params.request.postData);
        const request=body.request??body;
        if(path===gateway&&['game_telemetry_inspect','profile_runtime_inspect'].includes(request.command))previewReads++;
        if(path===gateway&&request.command==='game_telemetry_inspect'&&request.gameId==='dcs'&&request.search===airborneChannel){
          inspectorReads++;
          if(request.observationId!==inspectorSubscription)unscopedInspectorReads++;
        }
        const browserKey=JSON.stringify([body.connectionId,body.requestId]);
        if(path===subscribe&&/^(games\.telemetry$|profiles\.samples\.|controllers\.input\.)/.test(body.topic)){
          previewOpens++;
          previewPending.set(id,{topic:body.topic,browserKey});
        }else if(path===unsubscribe)previewPending.set(id,{token:browserUrl?browserKey:body.subscriptionId});
      }
      const pending=previewPending.get(id);
      if(!pending)return;
      if(method==='Network.responseReceived')pending.status=params.response.status;
      if(method!=='Network.loadingFinished'&&method!=='Network.loadingFailed')return;
      if(pending.topic){
        void cdp.call('Network.getResponseBody',{requestId:id}).then(({body})=>{
          assert.equal(pending.status,200);
          const response=JSON.parse(body);
          const token=browserUrl?response.subscriptionId:response;
          assert.equal(typeof token,'string');
          const key=browserUrl?pending.browserKey:token;
          if(!previewRetired.has(key))previewActive.set(key,pending.topic);
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
    inspectorSubscription=[...previewActive].find(([,topic])=>topic==='games.telemetry')[0];
  }
  async function withPreviewsClosed(owner,setAirborne) {
    await cdp.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="Overview"]').click()`);
    await until(async()=>{
      if(previewError)throw previewError;
      return previewPending.size===0 && previewActive.size===0 && await cdp.evaluate(`(()=>{
        const inspector=document.querySelector('[aria-label="Incoming game telemetry"]');
        return !inspector || !!inspector.closest('[hidden]');
      })()`);
    }).catch(error=>{throw new Error(`Preview teardown left ${JSON.stringify([...previewActive.values()])} active with ${previewPending.size} pending calls`,{cause:error});});
    const before=[previewOpens,previewReads], offset=(await samples()).length;
    closedPreviewAt=process.hrtime.bigint();
    setAirborne(true);
    if(!browserUrl&&!nativeRates)await sendDebug(owner,false);
    const lamp=await waitLamp(offset,sample=>sample.status==='fresh'&&sample.value===true);
    if(!browserUrl&&!nativeRates)await sendDebug(owner,true);
    assert.deepEqual([previewOpens,previewReads],before,'Closed previews must not subscribe or read while native routes run');
    return {lamp,retiredReaders:before[0],previewOpens:0,previewReads:0};
  }

  async function startProbe() {
    probeStart=process.hrtime.bigint();
    const args=seconds||nativeRates?['--vjoy-buttons','15000']:['--vjoy-buttons'];
    probe=spawn(probeExecutable,args,{windowsHide:true});
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
  }

  const paintOffsets=()=>Promise.all(panels.map(({page})=>page.evaluate(()=>window.__panelPaints.length)));

  function litCanvases(offsets) {
    return Promise.all(panels.map(async({page},index)=>{
      const paint=await until(()=>page.evaluate(`window.__panelPaints.slice(${offsets[index]}).find(row=>row.lit)`));
      const {initial,final}=presentation[index];
      const low=Math.max(initial.low,final.low), high=Math.min(initial.high,final.high);
      const reference=Number(probeStart)/1e6;
      return {panel:index+1,earliestMs:paint.at-high-reference,latestMs:paint.at-low-reference};
    }));
  }

  async function browserBurst(owner,setAirborne) {
    const offsets=await paintOffsets();
    dualSubmissions=await submitBrowserRate(request,owner,probeStart,()=>setAirborne(true));
    const overlapStart=Math.max(...dualSubmissions.map(rows=>rows[0].at));
    const overlapEnd=Math.min(...dualSubmissions.map(rows=>rows.at(-1).at));
    const canvases=await litCanvases(offsets);
    const after=await Promise.all(panels.map(({page})=>samples(page).then(rows=>rows.length)));
    setAirborne(false);
    await Promise.all(panels.map(({page},index)=>waitLamp(after[index],sample=>sample.status==='fresh'&&sample.value===false,page)));
    return {overlapMs:overlapEnd-overlapStart,overlapStart,overlapEnd,canvases};
  }

  async function sendNativeBurst(owner) {
    const submissions=[];
    const started=performance.now();
    for(const [index,value] of expected.entries()){
      const remaining=started+index*8-performance.now();
      if(remaining>0)await delay(remaining);
      const at=Number(process.hrtime.bigint()-probeStart)/1e6;
      submissions.push({at,pendingFrames:await sendDebug(owner,value)});
    }
    return submissions;
  }

  async function nativeMcpBurst(owner,{setAirborne,setPaused}) {
    const offsets=await paintOffsets();
    setAirborne(true);
    const [debug, observed]=await Promise.allSettled([
      sendNativeBurst(owner),
      (async()=>{
        const debugSample=await observation.sample(true,'button-1');
        const canvases=await litCanvases(offsets);
        await page.getByRole('button',{name:'probe: press',exact:true}).click();
        const panelSample=await observation.sample(true,'button-2');
        assert.equal(panelSample.generation,debugSample.generation);
        assert.ok(BigInt(panelSample.revision)>BigInt(debugSample.revision));
        return {debugSample,panelSample,canvases,panelSampleAtMs:Number(process.hrtime.bigint()-probeStart)/1e6};
      })(),
    ]);
    if(debug.status==='rejected')throw debug.reason;
    if(observed.status==='rejected')throw observed.reason;
    nativeSubmissions=debug.value;
    const overlapStart=nativeSubmissions[0].at, overlapEnd=nativeSubmissions.at(-1).at;
    const actualHz=(nativeSubmissions.length-1)*1000/(overlapEnd-overlapStart);
    assert.ok(Math.abs(actualHz/125-1)<=.05,`Native debug sustained ${actualHz.toFixed(2)} Hz against 125 Hz`);
    assert.ok(observed.value.panelSampleAtMs>=overlapStart&&observed.value.panelSampleAtMs<=overlapEnd,
      'MCP must see the panel press during the native debug burst');
    for(const panel of observed.value.canvases){
      assert.ok(panel.earliestMs>=overlapStart&&panel.latestMs<=overlapEnd,
        `Panel ${panel.panel} must update during the native debug burst`);
    }
    const mark=(await samples()).length;
    setPaused(true);
    const paused=await waitLamp(mark,sample=>sample.status!=='fresh');
    const afterPause=(await samples()).length;
    setPaused(false);
    const resumed=await waitLamp(afterPause,sample=>sample.status==='fresh'&&sample.value===true);
    const after=(await samples()).length;
    setAirborne(false);
    const inactive=await waitLamp(after,sample=>sample.status==='fresh'&&sample.value===false);
    return {...observed.value,paused,resumed,inactive,overlapStart,overlapEnd,
      authoredTransitions:nativeSubmissions.length,actualHz};
  }

  async function burst(owner, {setAirborne,setPaused,submitOwnship}) {
    await page.getByText('Connected · panel active',{exact:true}).waitFor();
    await waitLamp(0,sample=>sample.status==='fresh'&&sample.value===false);
    await startProbe();
    if(browserUrl)return browserBurst(owner,setAirborne);
    if(nativeRates){
      const commandBurst=commands&&((start,durationMs)=>commands.run(owner,start,durationMs,page));
      const run=createNativeRateBurst({cdp,observation,lanes:dualRateLanes,delay,paintOffsets,litCanvases,panels,samples,waitLamp,probeStart,commandBurst});
      const readScope=()=>({active:previewActive.get(inspectorSubscription)==='games.telemetry',inspectorReads,unscopedInspectorReads,previewError});
      const result=inspectorTiming?await withInspectorPresentation(cdp,submitOwnship,readScope,show=>run(owner,setAirborne,show))
        :await run(owner,setAirborne,()=>setAirborne(true));
      const {submissions,evidence:nativeRateEvidence,...load}=result;
      dualSubmissions=submissions;
      return {...load,nativeRateEvidence,submissionTimestamp:'source offer'};
    }
    if(observation)return nativeMcpBurst(owner,{setAirborne,setPaused});
    const mark=(await samples()).length;
    setAirborne(true);
    const active=await waitLamp(mark,sample=>sample.status==='fresh'&&sample.value===true);
    await page.getByRole('button',{name:'probe: press',exact:true}).click();
    const beforePause=(await samples()).length;
    setPaused(true);
    const start=performance.now();
    await sendNativeBurst(owner);
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
    if(dualRateSoak)return {...await dualRateSoak.stopped(stopNs),slot:slot.slot,slotFree:true};
    const raw=await report;
    assert.ok(!raw.captureOverflow,'Raw Input capture exceeded the event bound');
    const events=raw.buttonEvents.filter(event=>event.report===slot.slot);
    assert.equal(new Set(events.map(event=>event.device)).size,1);
    assert.ok(events.every(event=>event.status===0x00110000));
    const hz=BigInt(raw.receiverTickHz), start=BigInt(raw.receiverStartedTicks);
    const qpcNs=ticks=>ticks*1_000_000_000n/hz;
    assert.ok(qpcNs(start)>=probeStart&&qpcNs(start)<=probeReady,'Raw Input and Node must share QPC');
    let probeAfterStopMs;
    if(seconds||nativeRates){
      assert.ok(raw.receiverEndedTicks,'Use a Raw Input probe with a capture end timestamp');
      probeAfterStopMs=Number(qpcNs(BigInt(raw.receiverEndedTicks))-stopNs)/1e6;
      assert.ok(probeAfterStopMs>0,`Raw Input must keep capturing through Stop (${probeAfterStopMs.toFixed(2)} ms)`);
    }
    if(browserUrl||nativeRates){
      const receiver=event=>Number(qpcNs(start+BigInt(event.receivedTicks))-probeStart)/1e6;
      const lanes=dualRateResults(dualSubmissions,events,raw.receiverTickHz,receiver,{low:0,high:0},Number(stopNs-probeStart)/1e6);
      for(const lane of lanes){
        assert.deepEqual(lane.received,lane.expected,`${lane.nodeId} ordered delivery and Stop release`);
        assert.ok(lane.stopReleaseMs>=0,`${lane.nodeId} must remain held until Stop`);
      }
      return {slot:slot.slot,lanes,submissions:dualSubmissions,slotFree:true,probeAfterStopMs};
    }
    const edges=field=>events.filter((event,index)=>event[field]!==(events[index-1]?.[field]??false));
    const debug=edges('pressed'), panel=edges('peerPressed');
    assert.deepEqual(debug.map(event=>event.pressed),[...expected,false,true,false]);
    assert.deepEqual(panel.map(event=>event.peerPressed),[true,false]);
    const closedPreviewEdges=debug.slice(expected.length,-1);
    assert.equal(closedPreviewEdges.length,2);
    assert.ok(closedPreviewEdges.every(event=>qpcNs(start+BigInt(event.receivedTicks))>=closedPreviewAt));
    const stopReleaseMs=[debug.at(-1),panel.at(-1)].map(event=>Number(
      qpcNs(start+BigInt(event.receivedTicks))-stopNs)/1e6);
    assert.ok(stopReleaseMs.every(ms=>ms>=0),'Both buttons must stay held until Stop');
    let timing;
    if(nativeSubmissions){
      const submissionToRawInputMs=summarizeMilliseconds(nativeSubmissions.map(({at},index)=>
        Number(qpcNs(start+BigInt(debug[index].receivedTicks))-probeStart)/1e6-at));
      const pendingFramesPeak=Math.max(...nativeSubmissions.map(sample=>sample.pendingFrames));
      assert.ok(pendingFramesPeak>0&&pendingFramesPeak<=256);
      assert.ok(submissionToRawInputMs.p95<=20&&submissionToRawInputMs.p99<=50,
        'Native debug must meet the submission-to-Raw-Input budget');
      timing={submissionToRawInputMs,pendingFramesPeak};
    }
    return {slot:slot.slot,debugTransitions:expected.length,closedPreviewTransitions:closedPreviewEdges.length,
      panelTransitions:1,stopReleaseMs,slotFree:true,...timing};
  }
  async function close() {
    probe?.kill();
    dualRateSoak?.close();
    stopTrace?.();
    try {await observation?.close();} finally {try {await closePanels();} finally {await commands?.close();}}
  }
  async function closePanels() {
    try {await browser?.close();} finally {
      try {for(const {peer} of panels)await companion('companion_revoke',{companionId:peer.companionId});} finally {
        if(listenerEnabled)await companion('companion_disable',{expectedSettingsRevision:(await listener()).settingsRevision});
      }
    }
  }
  async function stop(stopNs) {
    if(!observation){
      await request({command:'profile_stop'});
      return stopped(stopNs);
    }
    const stopReplyMs=await observation.stop();
    const {unreadMs,retired}=await observation.resumeSlow({allowClosed:seconds>0||nativeRates&&extendedNativeRates});
    const fastUpdates=observation.fastUpdates();
    await observation.close();
    observation=undefined;
    return {...await stopped(stopNs),mcp:{stopReplyMs,unreadMs,fastRetired:true,slowRetired:retired,fastUpdates}};
  }
  return {prepare,bind,openInspector,measurePresentation,burst,withPreviewsClosed,stop,soak,close,openCommands:()=>commands?.open()};
}
export {combinedTimingMisses} from './vjoy-rate-transport.mjs';
