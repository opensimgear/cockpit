import assert from 'node:assert/strict';
import test from 'node:test';
import {randomUUID} from 'node:crypto';
import {spawn} from 'node:child_process';
import {mkdir,writeFile} from 'node:fs/promises';
import {createInterface} from 'node:readline';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';
const probeExecutable=process.env.OSG_VJOY_RAW_PROBE;
test('authored vJoy button transitions reach the exact Windows Raw Input slot',{
  skip:process.platform!=='win32'||!probeExecutable,timeout:60000,
},async()=>{
  const page=await nativePage();
  let saved, probe;
  const request=async value=>{
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'vjoy-raw-output-native'})}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);
    return reply.state;
  };
  const status=async()=>(await request({command:'profile_status'})).profiles.profileSession;
  const inventory=async()=>(await request({command:'system_inventory'})).systemInventory.virtualDevices;
  try {
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
    probe=spawn(probeExecutable,['--vjoy-button'],{windowsHide:true});
    const lines=createInterface({input:probe.stdout});
    const trace=[];
    let markReady;
    const ready=new Promise(resolve=>markReady=resolve);
    lines.on('line',line=>{if(line==='ready')markReady();else trace.push(line);});
    const exited=new Promise((resolve,reject)=>{probe.once('error',reject);probe.once('close',resolve);});
    await Promise.race([ready,exited.then(()=>{throw Error('Raw Input probe ended before registering');})]);
    const streamId=randomUUID();
    for(const [index,value] of [true,false,true,false].entries()){
      await request({command:'profile_debug_controller_input',owner:started.owner,nodeId:'debug',streamId,sequence:String(index+1),changes:[{portId:'button-1',value}]});
      await new Promise(resolve=>setTimeout(resolve,250));
    }
    assert.equal(await exited,0);
    const report=JSON.parse(trace.at(-1));
    await mkdir('.research/reactive',{recursive:true});
    await writeFile('.research/reactive/vjoy-raw-output-result.json',JSON.stringify({slot:slot.slot,...report},null,2));
    const events=report.buttonEvents.filter(event=>event.report===slot.slot);
    assert.ok(events.every(event=>event.status===0x00110000),'HID button decoding must succeed');
    assert.equal(new Set(events.map(event=>event.device)).size,1,'The exact slot must appear on one Raw Input device');
    assert.deepEqual(events.map(event=>event.pressed),[true,false,true,false]);
    await request({command:'profile_stop'});
    assert.equal((await status()).owner,null);
    await until(inventory,devices=>devices.find(device=>device.slot===slot.slot)?.status==='free');
  } finally {
    try {
      if(saved){
        if((await status()).owner?.profileId===saved.id)await request({command:'profile_stop'});
        const current=(await request({command:'profile_assignments_get',id:saved.id})).profiles.document;
        await request({command:'profile_delete',id:current.id,expectedRevision:current.revision});
      }
    } finally {probe?.kill();page.close();}
  }
});
