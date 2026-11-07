import assert from 'node:assert/strict';
import test from 'node:test';
import {randomUUID} from 'node:crypto';
import {execFile, spawn} from 'node:child_process';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createInterface} from 'node:readline';
import {promisify} from 'node:util';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';
import {mcpVjoyObservation} from './mcp-vjoy-observation.mjs';
const probeExecutable=process.env.OSG_VJOY_RAW_PROBE;
const slowPreview=process.env.OSG_VJOY_SLOW_PREVIEW==='1';
const settingsInventory=process.env.OSG_VJOY_SETTINGS_INVENTORY==='1';
const dualRate=process.env.OSG_VJOY_DUAL_RATE==='1';
// Opt in with the full executable path of the dedicated dev Cockpit at the CDP URL.
const faultApp=process.env.OSG_VJOY_FAULT_APP;

async function terminateFixtureFeeder() {
  const endpoint=new URL(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  assert.equal(endpoint.hostname,'127.0.0.1');
  assert.match(endpoint.port,/^[0-9]+$/);
  const script=`
    $ErrorActionPreference = 'Stop'
    $listeners = @(Get-NetTCPConnection -LocalAddress 127.0.0.1 -LocalPort ([int]$env:OSG_VJOY_FAULT_PORT) -State Listen)
    if ($listeners.Count -ne 1) { throw 'Expected one DevTools listener' }
    $app = Get-CimInstance Win32_Process -Filter "ProcessId = $($listeners[0].OwningProcess)"
    for ($depth = 0; $app -and $depth -lt 8 -and $app.ExecutablePath -ne $env:OSG_VJOY_FAULT_APP; $depth++) {
      $app = Get-CimInstance Win32_Process -Filter "ProcessId = $($app.ParentProcessId)"
    }
    if (!$app -or $app.ExecutablePath -ne $env:OSG_VJOY_FAULT_APP) { throw 'Unexpected DevTools process ancestry' }
    $feeders = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($app.ProcessId)" |
      Where-Object { $_.ExecutablePath -eq $env:OSG_VJOY_FAULT_APP -and $_.CommandLine -match '\\s--osg-vjoy-feeder\\s*$' })
    if ($feeders.Count -ne 1) { throw 'Expected one owned vJoy feeder' }
    $feeder = Get-Process -Id $feeders[0].ProcessId
    if ($feeder.Path -ne $env:OSG_VJOY_FAULT_APP -or [Math]::Abs(($feeder.StartTime - $feeders[0].CreationDate).TotalSeconds) -gt 1) {
      throw 'Feeder process identity changed'
    }
    Stop-Process -InputObject $feeder
  `;
  await promisify(execFile)('powershell.exe',['-NoProfile','-NonInteractive','-Command',script],{
    windowsHide:true,timeout:10_000,env:{...process.env,OSG_VJOY_FAULT_APP:resolve(faultApp),OSG_VJOY_FAULT_PORT:endpoint.port},
  });
}

function summarizeMilliseconds(values) {
  const sorted = [...values].sort((a,b) => a-b);
  assert.ok(sorted.length && sorted.every(value => Number.isFinite(value) && value >= 0));
  const at = percentile => sorted[Math.ceil(sorted.length * percentile) - 1];
  return { count: sorted.length, p50: at(0.5), p95: at(0.95), p99: at(0.99), max: sorted.at(-1) };
}

const dualRateLanes = [
  {nodeId:'debug', field:'pressed', intervalMs:1, count:193},
  {nodeId:'debug-peer', field:'peerPressed', intervalMs:8, count:25},
];
const intervals = values => values.slice(1).map((value,index) => value - values[index]);

async function submitDualRate(page, owner) {
  const lanes = dualRateLanes.map(lane => ({...lane, streamId:randomUUID()}));
  return page.evaluate(`(async () => {
    const owner = ${JSON.stringify(owner)};
    const lanes = ${JSON.stringify(lanes)};
    const start = performance.now() + 15;
    const results = await Promise.allSettled(lanes.map(async lane => {
      const submissions = [];
      for (let index = 0; index < lane.count; index++) {
        const remaining = start + index * lane.intervalMs - performance.now();
        if (remaining > 0) await new Promise(resolve => setTimeout(resolve, remaining));
        const value = index % 2 === 0;
        const sequence = String(index + 1);
        const at = performance.now();
        const reply = await window.__TAURI_INTERNALS__.invoke('gateway_request', {request:{
          command:'profile_debug_controller_input', clientId:'vjoy-dual-rate-fixture',
          owner, nodeId:lane.nodeId, streamId:lane.streamId, sequence, changes:[{portId:'button-1',value}],
        }});
        const state = reply.state?.debugController;
        if (!reply.ok || state?.sequence !== sequence || state.replayed
          || !Number.isInteger(state.pendingFrames) || state.pendingFrames < 1 || state.pendingFrames > 256) {
          throw Error(lane.nodeId + ' update ' + sequence + ' was not accepted: ' + JSON.stringify(reply));
        }
        submissions.push({at, roundTripMs:performance.now() - at, pendingFrames:state.pendingFrames, value});
      }
      return submissions;
    }));
    return results.map(result => {
      if (result.status === 'rejected') throw result.reason;
      return result.value;
    });
  })()`);
}

async function bracketWebViewClock(page, reference) {
  const samples = [];
  for (let index = 0; index < 8; index++) {
    const before = Number(process.hrtime.bigint() - reference) / 1e6;
    const browser = await page.evaluate('performance.now()');
    const after = Number(process.hrtime.bigint() - reference) / 1e6;
    // Leave 0.25 ms on each side for WebView timer quantization.
    samples.push({low:before - browser - 0.25, high:after - browser + 0.25});
  }
  return samples.reduce((best, sample) => sample.high - sample.low < best.high - best.low ? sample : best);
}

function dualRateResults(submissions, events, receiverTickHz, receiptMs, clock, stopMs) {
  return dualRateLanes.map((lane, index) => {
    const samples = submissions[index];
    let last = false;
    const edges = events.filter(event => {
      assert.equal(typeof event[lane.field], 'boolean', 'Rebuild RawInputProbe with two-button mode');
      const changed = event[lane.field] !== last;
      last = event[lane.field];
      return changed;
    });
    const targetHz = 1000 / lane.intervalMs;
    const submissionsMs = samples.map(sample => sample.at);
    const actualHz = (samples.length - 1) * 1000 / (submissionsMs.at(-1) - submissionsMs[0]);
    const receiverMs = edges.slice(0, -1).map(event => event.receivedTicks * 1000 / receiverTickHz);
    const expected = samples.map(sample => sample.value).concat(false);
    const received = edges.map(event => event[lane.field]);
    const intact = expected.length === received.length && expected.every((value,i) => value === received[i]);
    const latency = intact ? edges.slice(0, -1).map((event,i) => receiptMs(event) - samples[i].at) : [];
    return {
      ...lane, targetHz, actualHz,
      submissionIntervalsMs:summarizeMilliseconds(intervals(submissionsMs)),
      gatewayRoundTripMs:summarizeMilliseconds(samples.map(sample => sample.roundTripMs)),
      pendingFramesPeak:Math.max(...samples.map(sample => sample.pendingFrames)),
      receiverIntervalsMs:receiverMs.length > 1 ? summarizeMilliseconds(intervals(receiverMs)) : null,
      submissionToReceiverMs: intact ? {
        lower:summarizeMilliseconds(latency.map(ms => Math.max(0, ms - clock.high))),
        upper:summarizeMilliseconds(latency.map(ms => ms - clock.low)),
      } : null,
      stopReleaseMs: intact ? receiptMs(edges.at(-1)) - stopMs : null,
      expected, received,
    };
  });
}

test(faultApp?'owned vJoy feeder loss faults the profile and releases the exact Windows Raw Input slot':
  dualRate?'two paced authored sources deliver ordered vJoy transitions during the same live run':
  'authored vJoy button transitions reach the exact Windows Raw Input slot',{
  skip:process.platform!=='win32'||!probeExecutable,timeout:60000,
},async()=>{
  const page=await nativePage();
  let saved, probe, observation;
  const request=async value=>{
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'vjoy-raw-output-native'})}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);
    return reply.state;
  };
  const status=async()=>(await request({command:'profile_status'})).profiles.profileSession;
  const inventory=async()=>(await request({command:'system_inventory'})).systemInventory.virtualDevices;
  const overview=()=>page.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="Overview"]').click()`);
  const devicesSettings=async()=>{
    await page.evaluate(`Array.from(document.querySelectorAll('.sidebar-bottom button'))
      .find(button => (button.title || button.textContent.trim()) === 'Settings').click()`);
    await until(()=>page.evaluate(`!!document.querySelector('[aria-label="Settings sections"]')`));
    await page.evaluate(`document.querySelector('#settings-tab-outputs').click()`);
  };
  const visibleSlot=name=>page.evaluate(`Array.from(document.querySelectorAll('[aria-label="Configured virtual controllers"] li'))
    .find(row => row.querySelector('strong')?.textContent === ${JSON.stringify(name)})?.querySelector('span')?.textContent`);
  try {
    await until(()=>page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    if(dualRate)assert.ok(!faultApp,'Run feeder loss and dual-rate delivery as separate scenarios');
    if(faultApp)assert.notEqual(process.env.OSG_MCP_SLOW_VJOY,'1','Run feeder loss and MCP Stop as separate scenarios');
    assert.equal((await status()).owner,null);
    const slot=await until(async ()=>(await inventory()).find(device=>
      device.status==='free'&&device.buttons>=(dualRate?2:1)&&Array.isArray(device.axes)&&!device.discreteHats&&!device.forceFeedback
      &&device.buttons+device.axes.length+device.continuousHats<=128))
      .catch(cause=>{throw new Error('A compatible free vJoy slot is required',{cause});});
    if(settingsInventory){
      await devicesSettings();
      await until(()=>visibleSlot(slot.name), value=>value==='free');
    }
    saved=(await request({command:'profile_create',name:'Temporary vJoy lease '+randomUUID()})).profiles.document;
    const layout={
      buttons:Array.from({length:slot.buttons},(_,index)=>'button-'+(index+1)),
      axes:slot.axes.map((_,index)=>({portId:'axis-'+(index+1),range:'bipolar',fallback:0})),
      hats:Array.from({length:slot.continuousHats},(_,index)=>'hat-'+(index+1)),
    };
    const draft=await page.evaluate(`(async()=>{
      const {graphSetVirtual,graphConnect}=await import('/src/lib/profile-graph.ts');
      const document=${JSON.stringify(saved)};
      const boolean={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
      document.resources=[{id:'debug',name:'Debug input',kind:'debug-controller',required:false,
        ports:[{id:'button-1',name:'Button',direction:'output',contract:boolean}]}];
      if (${dualRate}) document.resources.push({...document.resources[0],id:'debug-peer',name:'Peer debug input'});
      const wired=graphConnect(graphSetVirtual({document,assignments:[]},
        {id:'fixture-vjoy',name:'Fixture',required:true,layout:${JSON.stringify(layout)}}),
        {nodeId:'debug',portId:'button-1'},{nodeId:'fixture-vjoy',portId:'button-1'});
      return (${dualRate}?graphConnect(wired,{nodeId:'debug-peer',portId:'button-1'},
        {nodeId:'fixture-vjoy',portId:'button-2'}):wired).document;
    })()`);
    saved=(await request({command:'profile_editor_save',document:draft,assignments:[],expectedRevision:saved.revision})).profiles.document;
    await request({command:'profile_virtual_slot_set',id:saved.id,expectedRevision:saved.revision,nodeId:'fixture-vjoy',slot:slot.slot});
    const started=(await request({command:'profile_apply',id:saved.id,expectedRevision:saved.revision,expectedActiveOwner:null})).profiles.profileSession;
    assert.equal(started.owner?.profileId,saved.id,JSON.stringify(started));
    assert.ok(['running','degraded'].includes(started.state),JSON.stringify(started));
    if(settingsInventory)await until(()=>visibleSlot(slot.name), value=>value?.startsWith('busy'));
    await until(inventory,devices=>devices.find(device=>device.slot===slot.slot)?.status==='busy');
    await new Promise(resolve=>setTimeout(resolve,600));
    if(dualRate||process.env.OSG_MCP_SLOW_VJOY==='1')observation=await mcpVjoyObservation(page,started.owner);
    let previewBefore;
    if(slowPreview){
      await page.evaluate(`(async()=>{
        const {gatewayRequest}=await import('/src/lib/gateway.ts');
        const {createNativeObservationTransport}=await import('/src/lib/streams/native.ts');
        const {observeControllerInputs}=await import('/src/lib/controller-observations.ts');
        const {controllers}=await gatewayRequest({command:'controller_list',offset:0});
        const source=controllers.sources.filter(source=>source.online&&source.baselineReady&&source.vendorId!==0x1234)
          .sort((a,b)=>b.controlCount-a.controlCount)[0];
        if(!source)throw Error('An attached reporting physical controller is required.');
        const state={abort:new AbortController(),held:0,errors:[]};
        window.__vjoySlowPreview=state;
        createNativeObservationTransport().open('controllers.input.'+source.id,{
          frame(){state.held++;},error(message){state.errors.push(message);}
        },state.abort.signal);
        state.stop=observeControllerInputs(source.id,value=>{
          if(value.status==='ready')state.fast=value.detail.source.sequence;
          if(value.status==='error'||value.status==='lost')state.errors.push(value.error??value.status);
        });
      })()`);
      previewBefore=await until(()=>page.evaluate(`(()=>{const s=window.__vjoySlowPreview;
        return s.held===1?s.fast:null;})()`));
    }
    const beforeProbe=process.hrtime.bigint();
    probe=spawn(probeExecutable,[dualRate?'--vjoy-buttons':'--vjoy-button'],{windowsHide:true});
    const lines=createInterface({input:probe.stdout});
    const trace=[];
    let markReady;
    const ready=new Promise(resolve=>markReady=resolve);
    lines.on('line',line=>{if(line==='ready')markReady();else trace.push(line);});
    const exited=new Promise((resolve,reject)=>{probe.once('error',reject);probe.once('close',resolve);});
    await Promise.race([ready,exited.then(()=>{throw Error('Raw Input probe ended before registering');})]);
    const afterReady=process.hrtime.bigint();
    const streamId=randomUUID();
    const burst=process.env.OSG_VJOY_RAW_BURST==='1';
    const levels=Array.from({length:burst?65:3},(_,index)=>index%2===0);
    const intervalMs=burst?8:250;
    const startedAt=performance.now(), submissions=[], submittedNs=[], gatewayRoundTrips=[];
    const clockBefore=dualRate?await bracketWebViewClock(page,beforeProbe):null;
    const dualSubmissions=dualRate?await submitDualRate(page,started.owner):null;
    const clockAfter=dualRate?await bracketWebViewClock(page,beforeProbe):null;
    if(dualRate)await observation.sample(true);
    if(!dualRate)for(const [index,value] of levels.entries()){
      const remaining=startedAt+index*intervalMs-performance.now();
      if(remaining>0)await new Promise(resolve=>setTimeout(resolve,remaining));
      const submitted=performance.now();
      submissions.push(submitted);
      submittedNs.push(process.hrtime.bigint());
      await request({command:'profile_debug_controller_input',owner:started.owner,nodeId:'debug',streamId,sequence:String(index+1),changes:[{portId:'button-1',value}]});
      gatewayRoundTrips.push(performance.now()-submitted);
      await observation?.sample(value);
    }
    const authoredSpanMs=performance.now()-startedAt;
    if(!dualRate&&!burst)await new Promise(resolve=>setTimeout(resolve,intervalMs));
    // Hold for several renewals and more than one complete 250 ms feeder
    // lease. The final release must come from Stop, not the input fixture.
    const heldMs=750;
    await new Promise(resolve=>setTimeout(resolve,heldMs));
    assert.equal((await status()).owner?.profileId,saved.id);
    assert.equal((await inventory()).find(device=>device.slot===slot.slot)?.status,'busy');
    const preview=slowPreview?await page.evaluate(`(()=>{const {fast,held,errors}=window.__vjoySlowPreview;
      return {fast,held,errors};})()`):null;
    if(preview){
      assert.equal(preview.held,1,'The slow preview must still hold only its initial delivery');
      assert.deepEqual(preview.errors,[]);
      assert.ok(BigInt(preview.fast)>BigInt(previewBefore),'Independent physical preview must advance while vJoy is active');
    }
    if(settingsInventory){
      await overview();
      await until(()=>page.evaluate(`!document.querySelector('[aria-label="Controller and desktop output settings"]')`));
    }
    const stopNs=process.hrtime.bigint();
    const stopReplyMs=observation?await observation.stop():null;
    if(faultApp){
      await terminateFixtureFeeder();
      await until(status,current=>current.state==='faulted');
    }else{
      if(!observation)await request({command:'profile_stop'});
      assert.equal((await status()).owner,null);
    }
    await until(inventory,devices=>devices.find(device=>device.slot===slot.slot)?.status==='free');
    if(settingsInventory){
      await devicesSettings();
      await until(()=>visibleSlot(slot.name), value=>value==='free');
    }
    const slowReader=observation?await observation.resumeSlow():null;
    assert.equal(await exited,0);
    const report=JSON.parse(trace.at(-1));
    const events=report.buttonEvents.filter(event=>event.report===slot.slot);
    assert.ok(events.every(event=>event.status===0x00110000),'HID button decoding must succeed');
    assert.equal(new Set(events.map(event=>event.device)).size,1,'The exact slot must appear on one Raw Input device');
    assert.ok(Number.isSafeInteger(report.receiverTickHz) && report.receiverTickHz > 0,
      'Rebuild RawInputProbe from the current source to collect receiver timing');
    assert.ok(events.every((event,index) => Number.isSafeInteger(event.receivedTicks)
      && event.receivedTicks >= 0 && (index === 0 || event.receivedTicks >= events[index-1].receivedTicks)));
    assert.match(report.receiverStartedTicks,/^[0-9]+$/,'Rebuild RawInputProbe with the absolute receiver start stamp');
    // On Windows libuv hrtime and high-resolution Stopwatch use QPC.
    // Refuse to join clocks if the receiver start falls outside its launch bracket.
    const startTicks=BigInt(report.receiverStartedTicks), hz=BigInt(report.receiverTickHz);
    const qpcNs=ticks=>ticks*1_000_000_000n/hz;
    assert.ok(qpcNs(startTicks)>=beforeProbe&&qpcNs(startTicks)<=afterReady,
      'Raw Input and Node must share the Windows performance counter');
    if(dualSubmissions){
      assert.ok(clockBefore.low<=clockAfter.high && clockAfter.low<=clockBefore.high,
        'WebView clock brackets must agree before and after the burst');
      const clock={low:Math.min(clockBefore.low,clockAfter.low),high:Math.max(clockBefore.high,clockAfter.high)};
      const receiptMs=event=>Number(qpcNs(startTicks+BigInt(event.receivedTicks))-beforeProbe)/1e6;
      const lanes=dualRateResults(dualSubmissions,events,report.receiverTickHz,receiptMs,clock,Number(stopNs-beforeProbe)/1e6);
      await mkdir('.research/reactive',{recursive:true});
      await writeFile('.research/reactive/vjoy-dual-rate-result.json',JSON.stringify({
        slot:slot.slot,heldMs,stopReplyMs,slowReader,preview,previewBefore,lanes,submissions:dualSubmissions,
        webViewClock:{before:clockBefore,after:clockAfter,offset:clock},...report,
      },null,2));
      for(const lane of lanes){
        assert.deepEqual(lane.received,lane.expected,`${lane.nodeId} ordered delivery and Stop release`);
        assert.ok(lane.stopReleaseMs>=0,`${lane.nodeId} released before Stop`);
      }
      return;
    }
    assert.deepEqual(events.map(event=>event.pressed),[...levels,false]);
    const receiverMs = events.slice(0,-1).map(event => event.receivedTicks * 1000 / report.receiverTickHz);
    const submissionToReceiverMs=events.slice(0,-1).map((event,index)=>
      Number(qpcNs(startTicks+BigInt(event.receivedTicks))-submittedNs[index])/1e6);
    const timing = {
      submissionIntervalsMs: summarizeMilliseconds(intervals(submissions)),
      gatewayRoundTripMs: summarizeMilliseconds(gatewayRoundTrips),
      receiverIntervalsMs: summarizeMilliseconds(intervals(receiverMs)),
      submissionToReceiverMs: summarizeMilliseconds(submissionToReceiverMs),
    };
    await mkdir('.research/reactive',{recursive:true});
    const resultFile=faultApp?'vjoy-feeder-loss-result.json':burst?'vjoy-raw-burst-result.json':'vjoy-raw-output-result.json';
    await writeFile('.research/reactive/'+resultFile,JSON.stringify({
      slot:slot.slot,heldMs,stopReplyMs,slowReader,preview,previewBefore,
      authoredTransitions:levels.length,requestedIntervalMs:intervalMs,authoredSpanMs,timing,submissionToReceiverMs,
      timingClock:{source:'Windows QPC',libuv:process.versions.uv,launchBracketMs:Number(afterReady-beforeProbe)/1e6},
      releasedAfter:faultApp?'feeder-loss':'stop',...report,
    },null,2));
  } finally {
    try {
      if(saved){
        if((await status()).owner?.profileId===saved.id)await request({command:'profile_stop'});
        const current=(await request({command:'profile_assignments_get',id:saved.id})).profiles.document;
        await request({command:'profile_delete',id:current.id,expectedRevision:current.revision});
      }
    } finally {
      try { await observation?.close(); }
      finally {
        try {
          if(slowPreview)await page.evaluate(`(()=>{const s=window.__vjoySlowPreview;
            s?.abort.abort();s?.stop?.();delete window.__vjoySlowPreview;})()`);
          if(settingsInventory)await overview().catch(()=>{});
        } finally {probe?.kill();page.close();}
      }
    }
  }
});
