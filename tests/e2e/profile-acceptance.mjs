// Ordinary native unified profile + production C gateway/peripheral. CI isolated host only.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash,randomBytes} from 'node:crypto';
import {once} from 'node:events';
import {setterProfile} from './profile-graph-fixtures.mjs';
import {mkdir,mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {processClient} from './process-client.mjs';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function wait(operation,predicate,label,limit=10000){const end=Date.now()+limit;let current;do{current=await operation();if(predicate(current))return current;await sleep(40);}while(Date.now()<end);assert.fail(`${label}: ${JSON.stringify(current)}`);}

test('unified physical Setter uses exact native pins, captured baseline and owned cleanup through production C nodes',{timeout:120000},async()=>{
  const launch=JSON.parse(await readFile(process.env.OSG_E2E_MANIFEST,'utf8'));
  await mkdir(launch.artifacts,{recursive:true});const directory=await mkdtemp(resolve(launch.artifacts,'unified-profile-'));
  const env={...process.env,OSG_LOCAL_GATEWAYS_ONLY:'1',OSG_SIMULATION_NAMESPACE:`profile-${randomBytes(8).toString('hex')}`,OSG_DCS_BIOS_CONFIG:resolve(directory,'old-anchor.json'),OSG_GAMES_CONFIG:resolve(directory,'old-games.json')};
  const uid=randomBytes(12).toString('hex'),gatewayId=`stm32:${uid}`,moduleId=randomBytes(16).toString('hex');
  let service=processClient(launch.serviceBinary,[],env);const peers=[];let sequence=0;
  const request=(command,extra={})=>service.request({command,clientId:'unified-profile-native-acceptance',...extra});
  const ok=async(command,extra={})=>{const reply=await request(command,extra);assert.equal(reply.ok,true,`${command}: ${reply.error}`);return reply.state;};
  const admin=async(device,command)=>{const reply=await device.line(`sim ${++sequence} ${command}`);assert.equal(reply.ok,true,reply.error);return reply.simulation;};
  try{
    const gateway=processClient(launch.gatewayBinary,['--identity',uid,'--state-dir',resolve(directory,'gateway')],env);peers.push(gateway);
    const ports=await wait(()=>ok('ports'),state=>state.ports.some(port=>port.serialNumber===uid),'gateway discovery');
    await ok('connect',{gatewayId,port:ports.ports.find(port=>port.serialNumber===uid).path});
    const definition=JSON.parse(await readFile(resolve(launch.deviceRepository,'simulation/configs/output-only.json'),'utf8'));definition.identity=moduleId;
    const path=resolve(directory,'output.json');await writeFile(path,JSON.stringify(definition));
    const device=processClient(launch.python,[resolve(launch.deviceRepository,'simulation/run.py'),'--config',path,'--gateway',uid,'--state-dir',resolve(directory,'device'),'--output-dir',launch.deviceBuild,'--no-build'],env);peers.push(device);
    const online=await wait(()=>ok('status',{gatewayId}),state=>state.modules.some(module=>module.id===moduleId&&module.online)&&!state.layoutPending,'output-only enrollment');
    const module=online.modules.find(module=>module.id===moduleId);assert.equal(module.enabled,false);assert.equal(module.published,false);
    const described=await ok('action_describe',{gatewayId,moduleId});assert.equal(described.moduleActions[moduleId].ownership.fallbackPolicy,'capture-at-claim');
    await ok('action_invoke',{gatewayId,moduleId,boot:module.boot,catalogueRevision:described.moduleActions[moduleId].catalogueRevision,actionId:'led.rgb.set_color',values:['112233']});
    const before=await admin(device,'inspect');
    const resource=await wait(async()=>{
      const page=(await ok('profile_resources_list',{offset:0,limit:128})).profileResources;
      return page.resources.find(resource=>resource.attributes.moduleId===moduleId&&resource.attributes.gatewayId===gatewayId);
    },Boolean,'native resource catalogue');
    const details=await wait(async()=> (await ok('profile_resources_inspect',{resourceId:resource.id,offset:0,limit:128})).profileResources,
      details=>details.ports.some(port=>port.descriptor.id==='action:led.rgb.set_color'&&port.current),'current native action catalogue');
    const action=details.ports.find(port=>port.descriptor.id==='action:led.rgb.set_color'&&port.current);assert.ok(action);
    const blank=JSON.parse(await readFile(resolve(launch.root,'tests/fixtures/profiles/blank.json'),'utf8'));
    const authored=setterProfile(blank,resource,action,'445566');
    const saved=(await ok('profile_editor_save',{...authored,expectedRevision:null})).profiles.document;
    const applied=(await ok('profile_apply',{id:saved.id,expectedRevision:1})).profiles.profileSession;
    assert.equal(applied.state,'running');const owner=applied.owner;
    await wait(()=>admin(device,'direct_action state'),state=>state.actions.states.find(action=>action.id==='led.rgb.set_color').values[0]==='445566','owned setter actual device readback');
    const busy=await admin(device,'direct_action invoke 1 led.rgb.set_color ABCDEF');assert.equal(busy.actionResult.error,'outputBusy');
    const after=await admin(device,'inspect');assert.equal(after.module.generation,before.module.generation,'profile output must not change persistent device configuration');
    for(const field of ['settingsErases','settingsPrograms','moduleReconnects','descriptorHex'])assert.deepEqual(after[field],before[field],field);
    assert.deepEqual(after.module.config,before.module.config);assert.equal(after.module.owner,0,'Profile output never acquires a persistent configuration writer');
    for(const command of ['mapping_virtual_start','mapping_preview_start','integration_start','integration_save','dcs_start','game_activate','controller_visibility_apply','controller_visibility_launch'])assert.equal((await request(command,{})).ok,false,command);
    await ok('profile_pause',{paused:true});
    await wait(()=>admin(device,'direct_action state'),state=>state.actions.states.find(action=>action.id==='led.rgb.set_color').values[0]==='112233','Pause restores actual captured prior output');
    assert.equal((await request('profile_runtime_inspect',{owner})).ok,false);
    await ok('profile_pause',{paused:false});const fresh=(await ok('profile_apply',{id:saved.id,expectedRevision:1})).profiles.profileSession.owner;assert.notDeepEqual(fresh,owner);
    await wait(()=>admin(device,'direct_action state'),state=>state.actions.states.find(action=>action.id==='led.rgb.set_color').values[0]==='445566','fresh Apply output');
    await ok('profile_stop');
    await wait(()=>admin(device,'direct_action state'),state=>state.actions.states.find(action=>action.id==='led.rgb.set_color').values[0]==='112233','Stop exact captured baseline restore');
    assert.equal((await request('profile_runtime_inspect',{owner:fresh})).ok,false);
    await service.stop();service=processClient(launch.serviceBinary,[],env);
    assert.equal((await ok('profile_status')).profiles.profileSession.owner,null);
    assert.deepEqual((await ok('profile_export',{id:saved.id})).profiles.document,saved);

    // This is a second, active-run lifecycle. The clean Stop/restart above is
    // retained; no graceful shutdown request or Drop may stand in for a crash.
    const routeOf=state=>{
      const current=state.modules.find(module=>module.id===moduleId);
      assert.ok(current?.online,'The original module must be online');
      const positiveU32=(value,label)=>assert.ok(Number.isInteger(value)&&value>0&&value<=0xffffffff,`${label} must be a present positive u32`);
      positiveU32(state.epoch,'Gateway epoch');positiveU32(current.lease,'CAN lease');positiveU32(current.boot,'Module boot');
      assert.ok(Number.isInteger(current.node)&&current.node>0&&current.node<=32,'Assigned CAN node must be 1..32');
      // This production module runtime starts at generation1; unknown/null/0
      // cannot become matching missing values in the exact recovery oracle.
      positiveU32(current.configurationGeneration,'Module configuration generation');
      return {gatewayId,epoch:state.epoch,moduleId,node:current.node,lease:current.lease,
        boot:current.boot,configurationGeneration:current.configurationGeneration};
    };
    const publicationOf=state=>{
      const current=state.modules.find(module=>module.id===moduleId);
      assert.equal(state.layoutPending,false);
      assert.equal(current?.enabled,false);assert.equal(current?.published,false);
      return {layoutPending:state.layoutPending,enabled:current.enabled,published:current.published};
    };
    const actualRgb=state=>state.actions.states.find(action=>action.id==='led.rgb.set_color').values[0];
    const ownershipOf=state=>{
      assert.equal(state.actionResult.ok,true);
      const entry=state.actionResult.ownership.find(entry=>entry.actionId==='led.rgb.set_color');
      assert.ok(entry,'The production ownership page must identify the actual RGB endpoint');
      return entry;
    };
    const crash={scope:'production-C software simulation; actual native service SIGKILL',hardware:false,qualification:false,
      publicationOverride:false,eventInputs:false,macro:false,recoveredActiveLeaseRelease:false,stages:[]};
    const checkpoint=async(stage,witness)=>{
      crash.stages.push({stage,...witness});
      await writeFile(resolve(directory,'active-crash.json'),JSON.stringify(crash,null,2));
    };
    const reconnect=async()=>{
      const current=await wait(()=>ok('ports'),state=>state.ports.some(port=>port.serialNumber===uid),'same gateway rediscovery');
      await ok('connect',{gatewayId,port:current.ports.find(port=>port.serialNumber===uid).path});
      return wait(()=>ok('status',{gatewayId}),state=>state.modules.some(module=>module.id===moduleId&&module.online)&&!state.layoutPending,'same output module after service restart');
    };
    const connected=await reconnect(),crashRoute=routeOf(connected),publication=publicationOf(connected);
    await ok('action_describe',{gatewayId,moduleId});
    const active=(await ok('profile_apply',{id:saved.id,expectedRevision:1})).profiles.profileSession;
    assert.equal(active.state,'running');assert.ok(active.owner);assert.equal(active.appliedSnapshot.profileId,saved.id);
    const owned=await wait(()=>admin(device,'direct_action ownership 0'),state=>actualRgb(state)==='445566'&&ownershipOf(state).owned===true,'active owned output before forced service death');
    const lease=ownershipOf(owned);assert.ok(Number.isInteger(lease.leaseId)&&lease.leaseId>0);
    assert.deepEqual(lease.fallback,['112233']);assert.equal(lease.fallbackStatus,'none');assert.ok(lease.remainingMs>0);
    assert.deepEqual(routeOf(await ok('status',{gatewayId})),crashRoute);
    publicationOf(await ok('status',{gatewayId}));
    const journalPath=resolve(directory,'profile-session-v1.json'),journalBytes=await readFile(journalPath),journal=JSON.parse(journalBytes);
    assert.equal(journal.pending,null);assert.deepEqual(journal.active.owner,active.owner);
    assert.deepEqual(journal.active.attempted,['overrides','acquire','baseline','visibility','start']);
    const receipts=journal.active.receipts.filter(receipt=>receipt.kind==='physical-action-lease');
    assert.equal(receipts.length,1);assert.equal(receipts[0].state.schemaVersion,1);
    assert.match(receipts[0].state.sealedHex,/^[0-9a-f]+$/);
    const journalSha256=createHash('sha256').update(journalBytes).digest('hex');
    await checkpoint('active-before-kill',{owner:active.owner,appliedSnapshot:active.appliedSnapshot,route:crashRoute,publication,
      actual:actualRgb(owned),ownership:lease,journalSha256,receiptKinds:journal.active.receipts.map(receipt=>receipt.kind)});

    const died=once(service.child,'exit');assert.equal(service.child.kill('SIGKILL'),true);
    const [exitCode,signal]=await died;assert.equal(signal,'SIGKILL');assert.equal(exitCode,null);
    await checkpoint('forced-death',{exitCode,signal,hostPid:service.child.pid});
    // No host exists during this observation. The real C clock/lease watchdog
    // runs normally: no sim advance, shortened TTL, reconnect or action write.
    const expired=await wait(()=>admin(device,'direct_action ownership 0'),state=>{
      const entry=ownershipOf(state);assert.equal(entry.leaseId,lease.leaseId,'No replacement Claim while the host is dead');
      return entry.owned===false;
    },'firmware watchdog after active host SIGKILL');
    const assertRestored=state=>{
      const entry=ownershipOf(state);assert.equal(entry.owned,false);assert.equal(entry.leaseId,lease.leaseId);
      assert.equal(entry.remainingMs,0);assert.equal(entry.fallbackStatus,'applied');
      assert.deepEqual(entry.fallback,['112233']);assert.equal(actualRgb(state),'112233');
    };
    assertRestored(expired);
    assert.ok(Number.isInteger(expired.clockMs)&&expired.clockMs>=0&&expired.clockMs<=0xffffffff,'Production simulator clock must be a present u32');
    assert.deepEqual(await readFile(journalPath),journalBytes,'A killed host cannot clean or replace the active journal');
    await checkpoint('watchdog-restored-while-host-dead',{clockMs:expired.clockMs,actual:actualRgb(expired),ownership:ownershipOf(expired)});

    // Same directory, environment and inherited CI DBus/SecretService session.
    // Construction is passive; explicit Recover is the only cleanup operation.
    service=processClient(launch.serviceBinary,[],env);
    const interrupted=(await ok('profile_status')).profiles.profileSession;
    assert.equal(interrupted.state,'recovery-needed');assert.equal(interrupted.owner,null);assert.equal(interrupted.appliedSnapshot,null);
    assert.match(interrupted.fault,/previous profile run needs explicit recovery/i);
    assert.deepEqual(await readFile(journalPath),journalBytes,'Cold startup must preserve exact durable cleanup evidence');
    assert.equal((await request('profile_runtime_inspect',{owner:active.owner})).ok,false);
    const denied=await request('profile_apply',{id:saved.id,expectedRevision:1});assert.equal(denied.ok,false);
    assert.match(denied.error,/Recover the interrupted profile session before applying/i);
    const returned=await reconnect();assert.deepEqual(routeOf(returned),crashRoute,'This case requires the original exact CAN route');
    assert.deepEqual(publicationOf(returned),publication);
    await ok('action_describe',{gatewayId,moduleId});
    const beforeRecover=await admin(device,'direct_action ownership 0');assertRestored(beforeRecover);
    await checkpoint('cold-recovery-needed',{status:interrupted,route:routeOf(returned),publication:publicationOf(returned),
      actual:actualRgb(beforeRecover),ownership:ownershipOf(beforeRecover),journalSha256});
    const recovered=(await ok('profile_recover')).profiles.profileSession;
    assert.equal(recovered.state,'stopped');assert.equal(recovered.owner,null);assert.equal(recovered.appliedSnapshot,null);assert.equal(recovered.fault,null);
    const empty=JSON.parse(await readFile(journalPath));assert.equal(empty.active,null);assert.equal(empty.pending,null);
    assert.deepEqual((await ok('profile_export',{id:saved.id})).profiles.document,saved);
    assert.equal((await request('profile_runtime_inspect',{owner:active.owner})).ok,false);
    await ok('profile_recover');
    const afterRecover=await admin(device,'direct_action ownership 0');assertRestored(afterRecover);
    // A full advertised lease period of passive readback catches productive
    // restart/recovery reactivation. The public monotonically allocated lease
    // ID must not change; the native no-request replay tests cover wire attempts.
    const maxLeaseMs=described.moduleActions[moduleId].ownership.maxLeaseMs;
    assert.equal(maxLeaseMs,5000,'The real advertised endpoint lease budget is unchanged');
    assert.ok(Number.isInteger(afterRecover.clockMs)&&afterRecover.clockMs>=0&&afterRecover.clockMs<=0xffffffff-maxLeaseMs,'The full observation period must fit the simulator u32 clock');
    const observedUntil=afterRecover.clockMs+maxLeaseMs;
    const quiet=await wait(async()=>{
      const state=await admin(device,'direct_action ownership 0');assertRestored(state);
      assert.ok(Number.isInteger(state.clockMs)&&state.clockMs>=afterRecover.clockMs&&state.clockMs<=0xffffffff,'Actual monotonic simulator clock');
      return state;
    },state=>state.clockMs>=observedUntil,'no productive output replay after explicit cold Recover');
    const finalStatus=(await ok('profile_status')).profiles.profileSession;assert.equal(finalStatus.state,'stopped');assert.equal(finalStatus.owner,null);
    const finalGateway=await ok('status',{gatewayId});assert.deepEqual(routeOf(finalGateway),crashRoute);assert.deepEqual(publicationOf(finalGateway),publication);
    const finalDevice=await admin(device,'inspect');
    for(const field of ['settingsErases','settingsPrograms','moduleReconnects','descriptorHex'])assert.deepEqual(finalDevice[field],before[field],field);
    assert.deepEqual(finalDevice.module.config,before.module.config);assert.equal(finalDevice.module.owner,0);
    await checkpoint('explicit-cold-recovery-complete',{status:finalStatus,route:routeOf(finalGateway),publication:publicationOf(finalGateway),
      actual:actualRgb(quiet),ownership:ownershipOf(quiet),passiveObservationMs:quiet.clockMs-afterRecover.clockMs,
      noNewClaim:true,noProductiveReplay:true,journalCleared:true,noPersistentConfigWrites:true});
    await writeFile(resolve(directory,'result.json'),JSON.stringify({scope:'software simulation',hardware:false,qualification:false,unifiedProfile:true,capturedBaseline:true,endpointConflict:true,pause:true,stop:true,noPersistentConfigWrites:true,restartWithoutOwner:true,activeHostSigkill:true,watchdogRestorationWhileHostDead:true,explicitColdRecovery:true,noProductiveReplay:true,activeLeaseReleaseAfterRestart:false,heldInputReplayTested:false,publicationOverrideTested:false},null,2));
  }finally{await service.stop();await Promise.all(peers.reverse().map(peer=>peer.stop()));}
});
