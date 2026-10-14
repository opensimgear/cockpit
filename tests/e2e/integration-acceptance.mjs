// Authored ACR byte snapshots -> normal Rust workers -> production C USB/CAN.
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomBytes } from 'node:crypto';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { processClient } from './process-client.mjs';
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
function frame(packetId,rpm,status=2,maxRpm=7500,epoch=1) {
  const p=Buffer.alloc(32),g=Buffer.alloc(8),s=Buffer.alloc(416);
  p.writeUInt32LE(packetId>>>0,0);p.writeInt32LE(5,16);p.writeInt32LE(rpm,20);p.writeFloatLE(124.25,28);g.writeUInt32LE(packetId>>>0,0);g.writeInt32LE(status,4);s.writeInt32LE(maxRpm,412);
  for(const [offset,value] of [[0,'synthetic'],[30,'fixture-1'],[68,'Acceptance car'],[134,'Test track']])s.write(value,offset,undefined,'utf16le');
  return {sourceId:'acceptance-acr',sourceEpoch:epoch,physics:[...p],graphics:[...g],staticPage:[...s]};
}

test('portable Rally outputs own real simulated endpoints, recover overrides, frozen sources and reboot without HID', {timeout:120000},async()=>{
  const launch=JSON.parse(await readFile(process.env.OSG_E2E_MANIFEST,'utf8'));
  await mkdir(launch.artifacts,{recursive:true});const directory=await mkdtemp(resolve(launch.artifacts,'integration-'));
  const env={...process.env,OSG_SIMULATION_NAMESPACE:`m4-${randomBytes(8).toString('hex')}`,OSG_DCS_BIOS_CONFIG:resolve(directory,'dcs.json')};
  const uids=[randomBytes(12).toString('hex'),randomBytes(12).toString('hex')],moduleId=randomBytes(16).toString('hex');
  const boardId=randomBytes(16).toString('hex');let sequence=0,packet=0,rpm=7100,operational=2,maximum=7500,epoch=1,feeding=false,timer;
  const peers=[];let service=processClient(launch.serviceBinary,[],env);
  const request=(command,extra={})=>service.request({command,clientId:'cockpit-milestone-four-acceptance',...extra});
  const ok=async(command,extra={})=>{const r=await request(command,extra);assert.equal(r.ok,true,`${command}: ${r.error}\n${JSON.stringify(r.state?.integration?.validation)}`);return r.state;};
  const admin=async(device,command)=>{const r=await device.line(`sim ${++sequence} ${command}`);assert.equal(r.ok,true,`${command}: ${r.error}`);return r.simulation;};
  async function wait(operation,accepts,label,limit=8000){const until=Date.now()+limit;let state;do{state=await operation();if(accepts(state))return state;await sleep(50);}while(Date.now()<until);assert.fail(`${label}: ${JSON.stringify(state)}`);}
  const devices=[];
  function stream(){timer=setInterval(async()=>{if(feeding)return;feeding=true;try{await ok('integration_simulate',{telemetryFrame:frame(++packet,rpm,operational,maximum,epoch)});}catch{}finally{feeding=false;}},60);}
  try {
    for(const [index,uid] of uids.entries()) {
      const gateway=processClient(launch.gatewayBinary,['--identity',uid,'--state-dir',resolve(directory,`gateway-${index}`)],env);peers.push(gateway);
      const definition=JSON.parse(await readFile(resolve(launch.deviceRepository,'simulation/configs/output-only.json'),'utf8'));definition.identity=moduleId;
      const path=resolve(directory,`device-${index}.json`);await writeFile(path,JSON.stringify(definition));
      const device=processClient(launch.python,[resolve(launch.deviceRepository,'simulation/run.py'),'--config',path,'--gateway',uid,'--state-dir',resolve(directory,`device-${index}`),'--output-dir',launch.deviceBuild,'--no-build'],env);devices.push(device);peers.push(device);
      const ports=await wait(()=>ok('ports'),s=>s.ports.some(p=>p.serialNumber===uid),'gateway discovery');
      await ok('connect',{gatewayId:`stm32:${uid}`,port:ports.ports.find(p=>p.serialNumber===uid).path});
      const state=await wait(()=>ok('status',{gatewayId:`stm32:${uid}`}),s=>s.modules.some(m=>m.id===moduleId && m.online && m.outputOnly),'output-only enrollment');
      const module=state.modules.find(m=>m.id===moduleId);assert.equal(module.enabled,false);assert.equal(module.published,false);assert.equal(module.reportId,null);assert.equal(module.buttonCount+module.axisCount+module.hatCount,0);
      await ok('action_describe',{gatewayId:`stm32:${uid}`,moduleId});
    }
    const board=JSON.parse(await readFile(resolve(launch.deviceRepository,'simulation/configs/output-only-board-led.json'),'utf8'));board.identity=boardId;
    const boardPath=resolve(directory,'board.json');await writeFile(boardPath,JSON.stringify(board));
    const boardDevice=processClient(launch.python,[resolve(launch.deviceRepository,'simulation/run.py'),'--config',boardPath,'--gateway',uids[0],'--state-dir',resolve(directory,'board'),'--output-dir',launch.deviceBuild,'--no-build'],env);peers.push(boardDevice);
    await wait(()=>ok('status',{gatewayId:`stm32:${uids[0]}`}),s=>s.modules.some(m=>m.id===boardId && m.online),'board output-only enrollment');
    const config={document:{schemaVersion:1,id:'rally-rev-lights',name:'Rally rev lights',provider:'assetto-corsa-rally',context:{},roles:[{id:'left',label:'Left'},{id:'right',label:'Right'},{id:'board',label:'Board LED'}],inputs:[],outputs:[
      ...['left','right'].map(role=>({id:`${role}-rev`,role,endpoint:'led.rgb',actionId:'led.rgb.set_color',channel:'rev_ratio',format:{kind:'rgb',bands:[{minimum:0.7,color:'00FF00'},{minimum:0.85,color:'FFFF00'},{minimum:0.95,color:'FF0000'}]}})),
      {id:'shift',role:'left',endpoint:'led.boolean',actionId:'led.boolean.set_enabled',channel:'rpm',format:{kind:'threshold',threshold:7000}},
      {id:'board-shift',role:'board',endpoint:'led.status',actionId:'led.status.set_mode',channel:'rpm',format:{kind:'threshold',threshold:7000}}
    ]},connection:{source:'synthetic',freshnessMs:300},assignments:[{role:'left',gatewayId:`stm32:${uids[0]}`,moduleId},{role:'right',gatewayId:`stm32:${uids[1]}`,moduleId},{role:'board',gatewayId:`stm32:${uids[0]}`,moduleId:boardId}]};
    // Check actual descriptor endpoints instead of assuming board-specific names.
    for(const output of config.document.outputs) {const a=config.assignments.find(a=>a.role===output.role);const state=await ok('action_describe',a);output.endpoint=state.moduleActions[a.moduleId].actions.find(action=>action.id===output.actionId).endpoint;}
    await ok('integration_save',{integrationConfig:config});assert.equal((await ok('integration_get_state')).integration.running,false);
    await ok('integration_validate',{integrationConfig:config});await ok('integration_start');stream();
    const applied=async()=> (await ok('integration_get_state')).integration;
    await wait(applied,s=>s.outputs.every(o=>o.owned && !o.pending && o.status==='ready'),'all claimed outputs converge');
    assert.deepEqual((await applied()).outputs.find(o=>o.id==='left-rev').applied,['FFFF00']);
    // Distinct direct-USB client cannot take an endpoint claimed over CAN.
    let direct=await admin(devices[0],'direct_action invoke 1 led.rgb.set_color 112233');assert.equal(direct.actionResult.ok,false);assert.equal(direct.actionResult.error,'outputBusy');
    direct=await admin(devices[0],'direct_action claim 1 led.rgb.set_color 99887766-5544-3322-1100-aabbccddeeff 1000 000000');assert.equal(direct.actionResult.error,'outputBusy');
    await ok('integration_override',{bindingId:'left-rev',values:['0000FF'],durationMs:600});
    await wait(applied,s=>s.outputs.find(o=>o.id==='left-rev').applied?.[0]==='0000FF','temporary override applies');
    rpm=6000;await wait(applied,s=>s.outputs.find(o=>o.id==='right-rev').applied?.[0]==='00FF00','peer follows current telemetry while override held');
    await wait(applied,s=>s.outputs.find(o=>o.id==='left-rev').applied?.[0]==='00FF00','expiry resumes latest telemetry');
    maximum=0;await wait(applied,s=>s.outputs.filter(o=>o.id.endsWith('-rev')).every(o=>o.applied?.[0]==='000000'),'unknown maximum drives ratio fallback');maximum=7500;
    operational=3;await wait(applied,s=>s.outputs.every(o=>o.status==='stale' && !o.pending),'paused source fallback');operational=2;
    await wait(applied,s=>s.outputs.every(o=>o.status==='ready' && !o.pending),'fresh advancing recovery');
    clearInterval(timer);timer=undefined;await sleep(100);const frozenPacket=packet;
    for(let i=0;i<8;i++){await ok('integration_simulate',{telemetryFrame:frame(frozenPacket,rpm)});await sleep(70);}
    const frozen=await applied();assert.equal(frozen.feedbackFresh,false);assert.ok(frozen.outputs.every(o=>o.status==='stale'));stream();
    await wait(applied,s=>s.feedbackFresh && s.outputs.every(o=>!o.pending),'advancing recovery after frozen copies');
    await admin(devices[1],'reboot');await wait(applied,s=>s.outputs.find(o=>o.id==='right-rev').owned && s.outputs.find(o=>o.id==='right-rev').applied?.[0]==='00FF00','reboot establishes new route and claim');
    clearInterval(timer);timer=undefined;await ok('integration_stop');const stopped=await applied();assert.equal(stopped.running,false);assert.equal(stopped.telemetry?.fresh ?? false,false);
    direct=await admin(devices[0],'direct_action invoke 1 led.rgb.set_color 112233');assert.equal(direct.actionResult.ok,true,'Stop releases actual device ownership');
    await ok('integration_validate',{integrationConfig:config});await ok('integration_start');rpm=7100;stream();await wait(applied,s=>s.outputs.every(o=>o.owned && !o.pending),'restart acquires fresh claims');
    clearInterval(timer);timer=undefined;await sleep(100);service.child.kill('SIGKILL');
    await wait(()=>admin(devices[0],'direct_action state'),s=>s.actions.states.find(o=>o.id==='led.rgb.set_color').values[0]==='000000','device TTL supplies fallback after host crash',8500);
    service=processClient(launch.serviceBinary,[],env);const persisted=await ok('integration_get_state');assert.equal(persisted.integration.configured,true);assert.equal(persisted.integration.running,false);
    await writeFile(resolve(directory,'result.json'),JSON.stringify({scope:'software simulation',provider:'assetto-corsa-rally',gateways:2,outputOnlyDevices:3,claims:true,override:true,frozen:true,reboot:true,hostExpiry:true,liveGame:false,hardware:false},null,2));
  } finally {clearInterval(timer);await service.stop();await Promise.all(peers.reverse().map(p=>p.stop()));}
});

import { createSocket } from 'node:dgram';
import { fileURLToPath } from 'node:url';

test('portable DCS profile owns a production board LED and forwards current panel events without held replay', {timeout:120000},async()=>{
  const launch=JSON.parse(await readFile(process.env.OSG_E2E_MANIFEST,'utf8'));
  const root=launch.root??fileURLToPath(new URL('../..',import.meta.url));
  await mkdir(launch.artifacts,{recursive:true});const directory=await mkdtemp(resolve(launch.artifacts,'integration-dcs-'));
  const env={...process.env,OSG_LOCAL_GATEWAYS_ONLY:'1',OSG_SIMULATION_NAMESPACE:`m4-dcs-${randomBytes(8).toString('hex')}`,OSG_DCS_BIOS_CONFIG:resolve(directory,'dcs.json')};
  const uid=randomBytes(12).toString('hex'),gatewayId=`stm32:${uid}`,moduleId=randomBytes(16).toString('hex');
  const definition=JSON.parse(await readFile(resolve(launch.deviceRepository,'simulation/configs/master-caution.json'),'utf8'));definition.identity=moduleId;
  const definitionPath=resolve(directory,'master-caution.json');await writeFile(definitionPath,JSON.stringify(definition));
  async function port(){const socket=createSocket('udp4');await new Promise(resolve=>socket.bind(0,'127.0.0.1',resolve));const value=socket.address().port;await new Promise(resolve=>socket.close(resolve));return value;}
  const commandPort=await port(),exportPort=await port();
  const service=processClient(launch.serviceBinary,[],env),peers=[];let device,peer,sequence=0;
  const request=(command,extra={})=>service.request({command,clientId:'cockpit-portable-dcs-acceptance',gatewayId,...extra});
  const ok=async(command,extra={})=>{const reply=await request(command,extra);assert.equal(reply.ok,true,`${command}: ${reply.error}\n${JSON.stringify(reply.state?.integration?.validation)}`);return reply.state;};
  const admin=async(process,command)=>{const reply=await process.line(`sim ${++sequence} ${command}`);assert.equal(reply.ok,true,`${command}: ${reply.error}`);return reply.simulation;};
  async function wait(operation,accepts,label,limit=8000){const until=Date.now()+limit;let value;do{value=await operation();if(accepts(value))return value;await sleep(50);}while(Date.now()<until);assert.fail(`${label}: ${JSON.stringify(value)}`);}
  const integration=async()=>(await ok('integration_get_state')).integration;
  const history=async()=>(await admin(peer,'inspect')).commands;
  const inputReady=async(label)=>wait(integration,s=>s.running&&s.feedbackFresh&&s.inputs?.find(i=>i.id==='ack')?.ready===true,label);
  try {
    const gateway=processClient(launch.gatewayBinary,['--identity',uid,'--state-dir',resolve(directory,'gateway')],env);peers.push(gateway);
    const ports=await wait(()=>ok('ports'),s=>s.ports.some(p=>p.serialNumber===uid),'gateway discovery');
    await ok('connect',{port:ports.ports.find(p=>p.serialNumber===uid).path});
    device=processClient(launch.python,[resolve(launch.deviceRepository,'simulation/run.py'),'--config',definitionPath,'--gateway',uid,'--state-dir',resolve(directory,'device'),'--output-dir',launch.deviceBuild,'--no-build'],env);peers.push(device);
    await wait(()=>ok('status'),s=>!s.layoutPending&&s.modules.some(m=>m.id===moduleId&&m.online),'master-caution route');
    await ok('disable',{moduleId});
    await wait(()=>ok('status'),s=>!s.layoutPending&&s.modules.some(m=>m.id===moduleId&&m.online&&m.enabled===false&&m.published===false&&m.reportId===null),'HID disabled while independent panel events remain available');
    const actions=(await ok('action_describe',{moduleId})).moduleActions[moduleId];
    assert.deepEqual(actions.ownership,{schemaVersion:1,scope:'endpoint',maxLeaseMs:5000},'The actual device advertises ownership before Start claims it');
    const action=actions.actions.find(a=>a.id==='led.status.set_mode');assert.ok(action?.readable);assert.equal(action.semantics,'setter');
    const boolean=action.parameters[0].type==='boolean';assert.ok(boolean||action.parameters[0].type==='enum');
    const on=boolean?true:'on',off=boolean?false:'off';
    const value=async()=>(await admin(device,'inspect')).actions.states.find(s=>s.id===action.id).values[0];
    const config={document:{schemaVersion:1,id:'portable-master-caution',name:'Portable Master Caution',provider:'dcs-bios',context:{aircraftModule:'A-10C'},roles:[{id:'panel',label:'Master Caution panel'}],
      inputs:[{id:'ack',role:'panel',kind:'button',control:0,command:'UFC_MASTER_CAUTION',interface:'set_state',pressedValue:1,releasedValue:0}],
      outputs:[{id:'warning',role:'panel',channel:'MASTER_CAUTION',endpoint:action.endpoint,actionId:action.id,format:{kind:'boolean'}}]},
      connection:{cataloguePath:resolve(root,'simulation/dcs-bios/catalogue'),commandHost:'127.0.0.1',commandPort,listenAddress:'127.0.0.1',listenPort:exportPort,multicastGroup:'',multicastInterface:'127.0.0.1',freshnessMs:250},
      assignments:[{role:'panel',gatewayId,moduleId}]};
    peer=processClient(process.execPath,[resolve(root,'simulation/dcs-bios/peer.mjs'),'--command-port',String(commandPort),'--export-port',String(exportPort)],env);peers.push(peer);
    await admin(peer,'warning 1');await admin(device,'contact 0 1');
    await ok('integration_save',{integrationConfig:config});assert.equal((await integration()).running,false);
    await ok('integration_validate',{integrationConfig:config});assert.equal((await integration()).validation.valid,true);
    await ok('integration_start');
    await wait(integration,s=>s.feedbackFresh&&s.outputs.find(o=>o.id==='warning').owned&&s.inputs.find(i=>i.id==='ack')?.status==='waitingRelease','Start claims the endpoint and baselines held input');
    await wait(value,v=>v===on,'DCS feedback reaches the actual board LED callback');assert.equal((await history()).length,0,'Held-at-start input is not replayed');
    const owner=(await ok('action_ownership',{moduleId})).moduleActions[moduleId].ownershipPage.ownership.find(o=>o.endpoint===action.endpoint);assert.equal(owner.owned,true);assert.ok(owner.leaseId>0);
    let competing=await request('action_invoke',{moduleId,boot:(await ok('status')).modules.find(m=>m.id===moduleId).boot,catalogueRevision:actions.catalogueRevision,actionId:action.id,values:[off]});
    assert.equal(competing.ok,false);assert.match(competing.error,/outputBusy/,'A manual CAN client cannot bypass portable ownership');
    const direct=await admin(device,`direct_action claim ${actions.catalogueRevision} ${action.id} 99887766-5544-3322-1100-aabbccddeeff 1000 ${boolean?'0':'off'}`);assert.equal(direct.actionResult.error,'outputBusy','A second USB-scoped client cannot replace the CAN owner');
    await admin(device,'contact 0 0');await inputReady('Released startup input arms current head');assert.equal((await history()).length,0);
    // The contact admin commands advance the production sampler for its 30 ms
    // debounce; no synthetic event log or command history is injected.
    await admin(device,'contact 0 1');await admin(device,'contact 0 0');
    await wait(history,h=>h.length===2,'Debounced press and release arrive in order');assert.deepEqual((await history()).map(c=>c.line),['UFC_MASTER_CAUTION 1','UFC_MASTER_CAUTION 0']);
    await wait(value,v=>v===off,'Acknowledgment clears the LED');
    await admin(peer,'warning 1');await wait(value,v=>v===on,'New warning feedback');await admin(peer,'pause 1');
    await wait(integration,s=>s.feedbackFresh===false&&s.inputs.find(i=>i.id==='ack')?.ready===false,'Stale feedback disarms before held recovery');await wait(value,v=>v===off,'Stale feedback fallback is applied');
    await admin(device,'contact 0 1');await admin(peer,'pause 0');
    await wait(integration,s=>s.feedbackFresh&&s.inputs.find(i=>i.id==='ack')?.status==='waitingRelease','Fresh recovery baselines the held contact');assert.equal((await history()).length,2);
    await admin(device,'contact 0 0');await inputReady('Fresh release arms without a historical command');assert.equal((await history()).length,2);
    await admin(device,'contact 0 1');await admin(device,'contact 0 0');await wait(history,h=>h.length===4,'New current press works after recovery');
    await inputReady('Input remains armed before final held press');await admin(device,'contact 0 1');await wait(history,h=>h.length===5,'Final forwarded press is latched');
    await ok('integration_stop');await wait(history,h=>h.length===6,'Stop supplies one neutral release');
    assert.deepEqual((await history()).map(c=>c.line),['UFC_MASTER_CAUTION 1','UFC_MASTER_CAUTION 0','UFC_MASTER_CAUTION 1','UFC_MASTER_CAUTION 0','UFC_MASTER_CAUTION 1','UFC_MASTER_CAUTION 0']);
    assert.equal(await value(),off);assert.equal((await integration()).running,false);
    const released=(await ok('action_ownership',{moduleId})).moduleActions[moduleId].ownershipPage.ownership.find(o=>o.endpoint===action.endpoint);assert.equal(released.owned,false);assert.equal(released.fallbackStatus,'applied');
    const manual=await admin(device,`direct_action invoke ${actions.catalogueRevision} ${action.id} ${boolean?'1':'on'}`);assert.equal(manual.actionResult.ok,true,'Stop releases device ownership for manual control');
    await writeFile(resolve(directory,'result.json'),JSON.stringify({scope:'software simulation',provider:'dcs-bios',portableProfile:true,claims:true,boardLed:true,orderedCommands:(await history()).map(c=>c.line),heldStartup:true,heldRecovery:true,stopNeutral:true,liveDcs:false,hardware:false},null,2));
  } finally {await service.stop();await Promise.all(peers.reverse().map(p=>p.stop()));}
});
