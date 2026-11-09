import assert from 'node:assert/strict';
import {execFile,spawn} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {once} from 'node:events';
import {realpath} from 'node:fs/promises';
import {dirname,isAbsolute,relative,sep} from 'node:path';
import {createInterface} from 'node:readline';
import {setTimeout as delay} from 'node:timers/promises';
import {promisify} from 'node:util';
import test from 'node:test';
import {browserPage,nativeMainTarget,nativePage,until} from '../../scripts/reactive/cdp.mjs';

const runFile=promisify(execFile);
const url=process.env.OSG_GATEWAY_RESTART_URL;
const native=process.env.OSG_GATEWAY_RESTART_NATIVE==='1';
const probePath=process.env.OSG_GATEWAY_RESTART_PROBE;
const outputId='restart-vjoy';
const nodeId='restart-debug';

async function service(expectedPid) {
  const script=`
    $ErrorActionPreference='Stop'
    $root=Get-CimInstance Win32_Process -Filter "ProcessId = $env:OSG_GATEWAY_RESTART_ROOT_PID"
    if($root.Name -ne 'node.exe' -or $root.CommandLine -notmatch 'node_modules[/\\\\]vite[/\\\\]bin[/\\\\]vite.js'){
      throw 'Expected the private Vite process'
    }
    $exe=[IO.Path]::GetFullPath($env:OSG_GATEWAY_RESTART_EXE)
    $children=@(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($root.ProcessId)" |
      Where-Object { $_.ExecutablePath -eq $exe })
    if($children.Count -ne 1){throw 'Expected one native gateway owned by the private Vite process'}
    $row=$children[0]
    $result=@{pid=$row.ProcessId;parent=$row.ParentProcessId;created=$row.CreationDate.ToUniversalTime().ToString('o')}
    if($env:OSG_GATEWAY_RESTART_KILL_PID){
      if($row.ProcessId -ne [int]$env:OSG_GATEWAY_RESTART_KILL_PID){throw 'Gateway identity changed before kill'}
      $process=Get-Process -Id $row.ProcessId -ErrorAction Stop
      if($process.Path -ne $exe -or [Math]::Abs(($process.StartTime-$row.CreationDate).TotalSeconds) -gt 1){
        throw 'Gateway PID was replaced'
      }
      $result.frequency=[Diagnostics.Stopwatch]::Frequency
      $result.before=[string][Diagnostics.Stopwatch]::GetTimestamp()
      Stop-Process -InputObject $process -Force
      if(!$process.WaitForExit(3000)){throw 'The exact gateway service did not exit'}
      $result.after=[string][Diagnostics.Stopwatch]::GetTimestamp()
    }
    ConvertTo-Json -InputObject $result -Compress`;
  const {stdout}=await runFile('powershell.exe',['-NoProfile','-NonInteractive','-Command',script],{
    windowsHide:true,timeout:12000,env:{...process.env,OSG_GATEWAY_RESTART_KILL_PID:expectedPid?String(expectedPid):''},
  });
  return JSON.parse(stdout);
}

async function nativeHost() {
  const executable=await realpath(process.env.OSG_GATEWAY_RESTART_EXE);
  const database=await realpath(dirname(process.env.OSG_SQLITE_DATABASE));
  const webview=await realpath(dirname(process.env.WEBVIEW2_USER_DATA_FOLDER));
  const privateRoot=await realpath(process.env.OSG_GATEWAY_RESTART_PRIVATE_ROOT);
  const relativeToRoot=relative(privateRoot,dirname(executable));
  assert.ok(relativeToRoot!=='..'&&!relativeToRoot.startsWith('..'+sep)&&!isAbsolute(relativeToRoot));
  assert.equal(database.toLowerCase(),privateRoot.toLowerCase());
  assert.equal(webview.toLowerCase(),privateRoot.toLowerCase());
  const endpoint=new URL(process.env.OSG_NATIVE_CDP_URL);
  assert.equal(endpoint.hostname,'127.0.0.1');
  let child,target;
  const currentTarget=async()=>{
    try{return nativeMainTarget(await (await fetch(new URL('/json',endpoint),{signal:AbortSignal.timeout(1000)})).json());}
    catch{return undefined;}
  };
  return {
    async open(){
      assert.equal(await currentTarget(),undefined,'A private native CDP port is required');
      child=spawn(executable,[],{windowsHide:true,stdio:'ignore'});
      await once(child,'spawn');
      target=await until(currentTarget,Boolean,12000);
      const page=await nativePage(endpoint.origin,target.webSocketDebuggerUrl);
      try {
        await until(async()=>{
          try{return await page.evaluate("typeof window.__TAURI_INTERNALS__?.invoke === 'function'");}
          catch{return false;}
        },Boolean,5000);
        const state=await page.evaluate("window.__TAURI_INTERNALS__.invoke('persistence_status')");
        assert.equal((await realpath(state.path)).toLowerCase(),
          (await realpath(process.env.OSG_SQLITE_DATABASE)).toLowerCase(),'Expected the private Cockpit database');
        return page;
      } catch(error) {page.close();throw error;}
    },
    identity(){return {pid:child.pid,target:target.id};},
    async terminate(expected){
      assert.deepEqual(this.identity(),expected);
      const exited=once(child,'exit');
      const before=process.hrtime.bigint();
      assert.ok(child.kill(),'Failed to terminate the exact private app');
      await exited;
      const after=process.hrtime.bigint();
      await until(async()=>!await currentTarget(),Boolean,5000);
      return {pid:expected.pid,target:expected.target,before:String(before),after:String(after)};
    },
    async close(){
      if(child?.pid&&child.exitCode===null&&child.signalCode===null){
        const exited=once(child,'exit');
        child.kill();
        await exited;
      }
    },
  };
}

async function receive() {
  const launch=process.hrtime.bigint();
  const probe=spawn(probePath,['--vjoy-buttons','15000'],{windowsHide:true});
  const lines=createInterface({input:probe.stdout});
  const done=once(probe,'close');
  const opened=once(lines,'line');
  const rows=[];
  let stderr='';
  lines.on('line',line=>{if(line!=='ready')rows.push(line);});
  probe.stderr.on('data',data=>stderr=(stderr+data).slice(-4096));
  try {
    const [line]=await Promise.race([opened,done.then(()=>{throw Error('Raw Input registration failed '+stderr);})]);
    assert.equal(line,'ready');
    const ready=process.hrtime.bigint();
    return {
      async result(){
        assert.equal((await done)[0],0,stderr);
        assert.equal(rows.length,1);
        return {report:JSON.parse(rows[0]),launch,ready};
      },
      async close(){if(probe.exitCode===null&&probe.signalCode===null)probe.kill();await done;lines.close();},
    };
  } catch(error) {probe.kill();throw error;}
}

function checkReceipts(trace,slot,exit,firstPress,freshPress) {
  const {report,launch,ready}=trace;
  assert.equal(report.captureOverflow,false);
  if(exit.frequency)assert.equal(report.receiverTickHz,exit.frequency);
  const hz=BigInt(report.receiverTickHz);
  const ns=ticks=>BigInt(ticks)*1_000_000_000n/hz;
  const start=BigInt(report.receiverStartedTicks);
  assert.ok(ns(start)>=launch&&ns(start)<=ready,'Raw Input and Node must share QPC');
  const events=report.buttonEvents.filter(row=>row.report===slot).map(row=>{
    assert.equal(row.status,0x00110000);
    assert.equal(row.peerPressed,false);
    assert.ok(report.devices.some(device=>device.id===row.device&&device.virtualVjoy));
    return {device:row.device,pressed:row.pressed,ns:ns(start+BigInt(row.receivedTicks))};
  });
  assert.equal(new Set(events.map(row=>row.device)).size,1,'The slot must use one native device');
  const changes=events.filter((event,index)=>event.pressed!==(events[index-1]?.pressed??false));
  assert.deepEqual(changes.map(row=>row.pressed),[true,false,true,false],
    'The old press must release and only the fresh press may rearm the output');
  const killNs=exit.frequency?ns:BigInt;
  const startKill=killNs(exit.before), endKill=killNs(exit.after);
  assert.ok(changes[0].ns>=firstPress&&changes[0].ns<startKill);
  assert.ok(changes[1].ns>=startKill&&changes[1].ns<freshPress);
  assert.ok(changes[2].ns>=freshPress);
  const upperMs=Number(changes[1].ns-startKill)/1e6;
  const lowerMs=Math.max(0,Number(changes[1].ns-endKill)/1e6);
  assert.ok(upperMs<=250,`Process kill to native release upper bound was ${upperMs} ms`);
  return {edges:changes.map(row=>({...row,ns:String(row.ns)})),
    killBracketMs:Number(endKill-startKill)/1e6,releaseLowerMs:lowerMs,releaseUpperMs:upperMs};
}

test(`${native?'native Cockpit':'browser-dev gateway'} death releases held vJoy and reopens from a neutral owner`,{
  skip:process.platform!=='win32'||!native&&!url,timeout:60000,
},async()=>{
  assert.equal(process.env.OSG_GATEWAY_RESTART_PRIVATE,'1');
  assert.ok(process.env.OSG_SQLITE_DATABASE);
  if(!native)assert.equal(new URL(url).hostname,'127.0.0.1');
  const host=native?await nativeHost():null;
  let page,profile,slot,probe,induced=false;
  const gateway=request=>page.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest})=>
    gatewayRequest(${JSON.stringify(request)}))`);
  const status=async()=>(await gateway({command:'profile_status'})).profiles.profileSession;
  const inventory=async()=>(await gateway({command:'system_inventory'})).systemInventory.virtualDevices;
  const input=(owner,streamId,sequence,value)=>gateway({command:'profile_debug_controller_input',owner,nodeId,
    streamId,sequence:String(sequence),changes:[{portId:'button-1',value}]});
  const readInput=async owner=>(await gateway({command:'profile_debug_controller_status',owner,nodeId}))
    .debugController.values.find(port=>port.portId==='button-1').value;
  const apply=async()=> (await gateway({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,
    expectedActiveOwner:null})).profiles.profileSession.owner;
  const openObserver=name=>page.evaluate(`(async()=>{
    const {SharedObservations}=await import('/src/lib/streams/shared.ts');
    const shared=window.__gatewayRestart?.shared??new SharedObservations(
      ${native
        ? "(await import('/src/lib/streams/native.ts')).createNativeObservationTransport()"
        : "(await import('/src/lib/streams/browser.ts')).createBrowserObservationTransport()"});
    const trace=window.__gatewayRestart??={identity:crypto.randomUUID(),old:[],fresh:[],shared};
    shared.subscribe('profiles.runtime',state=>trace[${JSON.stringify(name)}].push(state));
  })()`);
  try {
    page=host?await host.open():await browserPage(url);
    await until(()=>page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    assert.equal((await status()).owner,null);
    slot=await until(async()=>(await inventory()).find(row=>row.status==='free'&&row.buttons>=1&&Array.isArray(row.axes)
      &&!row.forceFeedback&&!row.discreteHats&&row.buttons+row.axes.length+row.continuousHats<=128));
    profile=(await gateway({command:'profile_create',name:'Private gateway restart '+randomUUID()})).profiles.document;
    const layout={buttons:Array.from({length:slot.buttons},(_,i)=>`button-${i+1}`),
      axes:slot.axes.map((_,i)=>({portId:`axis-${i+1}`,range:'bipolar',fallback:0})),
      hats:Array.from({length:slot.continuousHats},(_,i)=>`hat-${i+1}`)};
    const document=await page.evaluate(`(async()=>{
      const {graphSetVirtual,graphConnect}=await import('/src/lib/profile-graph.ts');
      const document=${JSON.stringify(profile)}, id=${JSON.stringify(nodeId)};
      const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:60000,fallback:false};
      document.resources.push({id,name:'Restart debug',kind:'debug-controller',required:false,
        ports:[{id:'button-1',name:'Button',direction:'output',contract}]});
      const outputId=${JSON.stringify(outputId)};
      const draft=graphSetVirtual({document,assignments:[]},{id:outputId,name:'Restart vJoy',required:true,
        layout:${JSON.stringify(layout)}});
      return graphConnect(draft,{nodeId:id,portId:'button-1'},{nodeId:outputId,portId:'button-1'}).document;
    })()`);
    profile=(await gateway({command:'profile_editor_save',document,assignments:[],expectedRevision:profile.revision})).profiles.document;
    await gateway({command:'profile_virtual_slot_set',id:profile.id,expectedRevision:profile.revision,nodeId:outputId,slot:slot.slot});
    const oldOwner=await apply(), oldStream=randomUUID();
    assert.equal(await readInput(oldOwner),false);
    const original=host?host.identity():await service();
    await openObserver('old');
    const snapshot=name=>page.evaluate(`(()=>{const trace=window.__gatewayRestart;return {identity:trace.identity,
      state:trace[${JSON.stringify(name)}].at(-1),lost:trace.old.some(row=>['error','lost'].includes(row.status))};})()`);
    const before=await until(()=>snapshot('old'),row=>row.state?.status==='ready'&&row.state.frame?.payload?.profileSession?.owner?.session===oldOwner.session);
    probe=await receive();
    const firstPress=process.hrtime.bigint();
    await input(oldOwner,oldStream,1,true);
    await until(()=>readInput(oldOwner),value=>value===true);
    await delay(650);
    assert.equal(await readInput(oldOwner),true);
    assert.equal((await inventory()).find(row=>row.slot===slot.slot)?.status,'busy');
    const exit=host?await host.terminate(original):await service(original.pid);
    induced=true;
    if(host){
      page.close();page=null;
      page=await host.open();
    }else{
      assert.equal(exit.created,original.created);
      const lost=await until(()=>snapshot('old'),row=>row.lost);
      assert.equal(lost.identity,before.identity);
    }
    const recovered=await status();
    assert.equal(recovered.owner,null,'Gateway restart must leave the old run stopped');
    if(recovered.state==='recovery-needed'||recovered.state==='faulted'){
      await gateway({command:'profile_recover'});
    }
    await until(inventory,rows=>rows.some(row=>row.slot===slot.slot&&row.status==='free'),5000);
    const replacement=host?host.identity():await service();
    assert.notEqual(replacement.pid,original.pid);
    if(host)assert.notEqual(replacement.target,original.target);
    else assert.equal(replacement.parent,original.parent);
    await openObserver('fresh');
    const reopened=await until(()=>snapshot('fresh'),row=>row.state?.status==='ready');
    assert.equal(reopened.identity===before.identity,!native);
    assert.equal(reopened.state.frame.kind,'snapshot');
    assert.equal(reopened.state.frame.payload.profileSession.owner,null);
    assert.notEqual(reopened.state.frame.session,before.state.frame.session);
    await assert.rejects(input(oldOwner,oldStream,2,true));
    const owner=await apply();
    assert.notEqual(owner.session,oldOwner.session);
    assert.equal(await readInput(owner),false);
    await delay(350);
    assert.equal(await readInput(owner),false);
    const freshPress=process.hrtime.bigint(), stream=randomUUID();
    await input(owner,stream,1,true);
    await until(()=>readInput(owner),value=>value===true);
    await delay(50);
    await input(owner,stream,2,false);
    await until(()=>readInput(owner),value=>value===false);
    await gateway({command:'profile_stop'});
    await until(inventory,rows=>rows.some(row=>row.slot===slot.slot&&row.status==='free'),5000);
    const receipt=checkReceipts(await probe.result(),slot.slot,exit,firstPress,freshPress);
    console.log(JSON.stringify({profile:{id:profile.id,revision:profile.revision},slot:slot.slot,
      original,exit,replacement,recoveryState:recovered.state,oldOwner,owner,
      oldObservationSession:before.state.frame.session,newObservationSession:reopened.state.frame.session,
      sameBrowser:!native,staleInputRejected:true,slotFree:true,receipt}));
  } finally {
    try {
      await probe?.close();
      await page?.evaluate('window.__gatewayRestart?.shared.close()');
      if(page&&profile){
        const current=await status();
        if(current.owner?.profileId===profile.id)await gateway({command:'profile_stop'});
        if(induced&&(current.state==='recovery-needed'||current.state==='faulted'))await gateway({command:'profile_recover'});
        await gateway({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});
      }
    } finally {await page?.close();await host?.close();}
  }
});
