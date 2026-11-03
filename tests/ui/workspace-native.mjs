// Opt-in actual Cockpit workspace observation. This fixture sends no device or output commands.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('native workspace shares its immutable baseline without frontend discovery or state polling',{
  skip:process.env.OSG_WORKSPACE_NATIVE!=='1',timeout:60000,
},async()=>{
  const date=new Date().toISOString().replace(/[:.]/g,'-'),directory=resolve(`.research/reactive/workspace-native/${date}`);
  await mkdir(directory,{recursive:true});
  let page,ownedProbe=false;
  const read=()=>page.evaluate(`(()=>{
    const p=window.__workspaceNativeProbe;
    if(!p)return null;
    const value=p.b?.value,entries=value?.entries,ports=value?.ports;
    return {
      aStatus:p.a?.status,bStatus:p.b?.status,error:p.b?.error,
      sameValue:!!p.a?.value&&p.a.value===p.b?.value,
      validPorts:Array.isArray(ports)&&ports.length<=64&&ports.every(port=>typeof port.path==='string'&&port.path.length>0),
      validEntries:Array.isArray(entries)&&entries.length<=64&&new Set(entries.map(entry=>entry.id)).size===entries.length
        &&entries.every(entry=>typeof entry.id==='string'&&typeof entry.session==='string'
          &&typeof entry.port?.path==='string'&&typeof entry.present==='boolean'&&typeof entry.busy==='boolean'
          &&entry.claimOwners&&Object.values(entry.claimOwners).every(owner=>/^sha256:[a-f0-9]{64}$/.test(owner))
          &&(entry.state===null||Array.isArray(entry.state?.modules))),
      portCount:ports?.length??0,entryCount:entries?.length??0,
      clockId:p.b?.clockId,generation:p.b?.generation,revision:p.b?.revision,
      lateConsumerCached:p.lateConsumerCached??false,firstDisposed:!!p.firstDisposed,
      monitoredMs:p.monitorStarted===undefined?0:performance.now()-p.monitorStarted,
      gatewayRequestCounts:p.gatewayRequestCounts??{},forbiddenRequests:p.forbiddenRequests??[],
    };
  })()`);
  try{
    page=await nativePage();
    await until(async()=>{try{return await page.evaluate("document.readyState==='complete'&&document.title==='OpenSimGear Cockpit'&&typeof window.__TAURI_INTERNALS__?.invoke==='function'");}catch{return false;}});
    await page.evaluate(`(async()=>{
      if(window.__workspaceNativeProbe)throw Error('A workspace acceptance probe is already active.');
      const {observeWorkspaceState}=await import('/src/lib/workspace-read-models/shared.ts');
      const p=window.__workspaceNativeProbe={stops:[],observeWorkspaceState};
      p.stops.push(observeWorkspaceState(state=>p.a=state));
      p.stops.push(observeWorkspaceState(state=>p.b=state));
    })()`);
    ownedProbe=true;
    const baseline=await until(read,state=>state?.aStatus==='ready'&&state?.bStatus==='ready');
    assert.equal(baseline.sameValue,true,'Both consumers must receive the same immutable cached value.');
    assert.equal(baseline.validPorts,true);assert.equal(baseline.validEntries,true);
    assert.match(baseline.clockId,/^workspace:[a-f0-9-]{36}$/);

    await page.evaluate(`(async()=>{
      const p=window.__workspaceNativeProbe;
      p.stops[0]();p.firstDisposed=true;
      await new Promise(resolve=>{
        let stop,done=false;
        stop=p.observeWorkspaceState(state=>{
          if(state.status!=='ready')return;
          p.lateConsumerCached=state.value===p.b?.value;done=true;stop?.();resolve();
        });
        p.stops.push(stop);if(done)stop();
      });
      p.gatewayRequestCounts={};p.forbiddenRequests=[];
      p.originalInvoke=window.__TAURI_INTERNALS__.invoke;
      p.invokeWrapper=function(command,args,...rest){
        if(command==='gateway_request'){
          const name=String(args?.request?.command??'missing');
          p.gatewayRequestCounts[name]=(p.gatewayRequestCounts[name]??0)+1;
          if(['discover','ports','status','connect','start','config_read','action_describe'].includes(name)
            &&p.forbiddenRequests.length<20)p.forbiddenRequests.push(name);
        }
        return Reflect.apply(p.originalInvoke,this,[command,args,...rest]);
      };
      window.__TAURI_INTERNALS__.invoke=p.invokeWrapper;p.monitorStarted=performance.now();
    })()`);
    await page.evaluate('new Promise(resolve=>setTimeout(resolve,6500))');
    const observed=await until(read,state=>state?.bStatus==='ready');
    assert.equal(observed.firstDisposed,true);assert.equal(observed.lateConsumerCached,true,'Removing one consumer must preserve the shared current baseline.');
    assert.equal(observed.validPorts,true);assert.equal(observed.validEntries,true);
    assert.ok(observed.monitoredMs>=6500);
    assert.deepEqual(observed.forbiddenRequests,[],'Native automation must not be driven by frontend gateway IPC polling.');
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify({at:new Date().toISOString(),
      actualNativeCockpit:true,twoConsumersSharedBaseline:true,firstDisposalPreservesCurrentCache:true,
      observedPorts:observed.portCount,observedEntries:observed.entryCount,monitoredMs:Math.floor(observed.monitoredMs),
      gatewayRequestCounts:observed.gatewayRequestCounts,forbiddenUiGatewayRequests:observed.forbiddenRequests,
      fixtureIssuedControlCommands:false,fixtureActivatedOutputs:false,hardwareRequired:false,
      physicalGatewayAcceptance:false,physicalLatencyQualified:false,
    },null,2)+'\n');
  }catch(error){
    await writeFile(resolve(directory,'failure.json'),JSON.stringify({at:new Date().toISOString(),error:String(error),probe:page?await read().catch(()=>null):null},null,2)+'\n');
    throw error;
  }finally{
    if(page){
      try{if(ownedProbe)await page.evaluate(`(()=>{
        const p=window.__workspaceNativeProbe;if(!p)return;
        try{if(p.originalInvoke&&window.__TAURI_INTERNALS__.invoke===p.invokeWrapper)window.__TAURI_INTERNALS__.invoke=p.originalInvoke;}
        finally{for(const stop of p.stops)stop();delete window.__workspaceNativeProbe;}
      })()`);}finally{page.close();}
    }
  }
});
