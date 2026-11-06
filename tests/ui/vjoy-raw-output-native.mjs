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

test(faultApp?'owned vJoy feeder loss faults the profile and releases the exact Windows Raw Input slot':
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
  try {
    if(faultApp)assert.notEqual(process.env.OSG_MCP_SLOW_VJOY,'1','Run feeder loss and MCP Stop as separate scenarios');
    assert.equal((await status()).owner,null);
    const slot=(await inventory()).find(device=>device.status==='free'&&device.buttons>0&&Array.isArray(device.axes)&&!device.discreteHats&&!device.forceFeedback&&device.buttons+device.axes.length+device.continuousHats<=128);
    assert.ok(slot,'A compatible free vJoy slot is required');
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
      return graphConnect(graphSetVirtual({document,assignments:[]},
        {id:'fixture-vjoy',name:'Fixture',required:true,layout:${JSON.stringify(layout)}}),
        {nodeId:'debug',portId:'button-1'},{nodeId:'fixture-vjoy',portId:'button-1'}).document;
    })()`);
    saved=(await request({command:'profile_editor_save',document:draft,assignments:[],expectedRevision:saved.revision})).profiles.document;
    await request({command:'profile_virtual_slot_set',id:saved.id,expectedRevision:saved.revision,nodeId:'fixture-vjoy',slot:slot.slot});
    const started=(await request({command:'profile_apply',id:saved.id,expectedRevision:saved.revision,expectedActiveOwner:null})).profiles.profileSession;
    assert.equal(started.owner?.profileId,saved.id,JSON.stringify(started));
    assert.ok(['running','degraded'].includes(started.state),JSON.stringify(started));
    await until(inventory,devices=>devices.find(device=>device.slot===slot.slot)?.status==='busy');
    await new Promise(resolve=>setTimeout(resolve,600));
    if(process.env.OSG_MCP_SLOW_VJOY==='1')observation=await mcpVjoyObservation(page,started.owner);
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
    probe=spawn(probeExecutable,['--vjoy-button'],{windowsHide:true});
    const lines=createInterface({input:probe.stdout});
    const trace=[];
    let markReady;
    const ready=new Promise(resolve=>markReady=resolve);
    lines.on('line',line=>{if(line==='ready')markReady();else trace.push(line);});
    const exited=new Promise((resolve,reject)=>{probe.once('error',reject);probe.once('close',resolve);});
    await Promise.race([ready,exited.then(()=>{throw Error('Raw Input probe ended before registering');})]);
    const streamId=randomUUID();
    const burst=process.env.OSG_VJOY_RAW_BURST==='1';
    const levels=Array.from({length:burst?65:3},(_,index)=>index%2===0);
    const intervalMs=burst?8:250;
    const startedAt=performance.now();
    for(const [index,value] of levels.entries()){
      const remaining=startedAt+index*intervalMs-performance.now();
      if(remaining>0)await new Promise(resolve=>setTimeout(resolve,remaining));
      await request({command:'profile_debug_controller_input',owner:started.owner,nodeId:'debug',streamId,sequence:String(index+1),changes:[{portId:'button-1',value}]});
      await observation?.sample(value);
    }
    const authoredSpanMs=performance.now()-startedAt;
    if(!burst)await new Promise(resolve=>setTimeout(resolve,intervalMs));
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
    const stopReplyMs=observation?await observation.stop():null;
    if(faultApp){
      await terminateFixtureFeeder();
      await until(status,current=>current.state==='faulted');
    }else{
      if(!observation)await request({command:'profile_stop'});
      assert.equal((await status()).owner,null);
    }
    await until(inventory,devices=>devices.find(device=>device.slot===slot.slot)?.status==='free');
    const slowReader=observation?await observation.resumeSlow():null;
    assert.equal(await exited,0);
    const report=JSON.parse(trace.at(-1));
    const events=report.buttonEvents.filter(event=>event.report===slot.slot);
    assert.ok(events.every(event=>event.status===0x00110000),'HID button decoding must succeed');
    assert.equal(new Set(events.map(event=>event.device)).size,1,'The exact slot must appear on one Raw Input device');
    assert.deepEqual(events.map(event=>event.pressed),[...levels,false]);
    await mkdir('.research/reactive',{recursive:true});
    const resultFile=faultApp?'vjoy-feeder-loss-result.json':burst?'vjoy-raw-burst-result.json':'vjoy-raw-output-result.json';
    await writeFile('.research/reactive/'+resultFile,JSON.stringify({
      slot:slot.slot,heldMs,stopReplyMs,slowReader,preview,previewBefore,
      authoredTransitions:levels.length,requestedIntervalMs:intervalMs,authoredSpanMs,
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
        } finally {probe?.kill();page.close();}
      }
    }
  }
});
