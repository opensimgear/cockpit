// Opt-in acceptance in the running native dev Cockpit. The authored SDK-layout
// publishers exercise acquisition, routing and inspection without a real game.
// The denial scenario also checks the visible process-monitor action.
import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {access,mkdir,readFile,rm,writeFile} from 'node:fs/promises';
import {resolve,sep} from 'node:path';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';
import {verifyScopedTelemetryDenial} from './scoped-telemetry-permission.mjs';

const enabled=process.env.OSG_ASSETTO_TELEMETRY_NATIVE==='1';
const denied=process.env.OSG_TELEMETRY_PERMISSION_DENIED==='1';
const execute=promisify(execFile),sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const specs=[['assetto-corsa','Assetto Corsa','acs.exe'],['assetto-corsa-competizione','Assetto Corsa Competizione','AC2-Win64-Shipping.exe'],['assetto-corsa-evo','Assetto Corsa EVO','AssettoCorsaEVO.exe']];
const permissionSpecs=[...specs,['assetto-corsa-rally','Assetto Corsa Rally','acr.exe']];
const near=(actual,expected)=>typeof expected==='number'?typeof actual==='number'&&Math.abs(actual-expected)<=1e-5*Math.max(1,Math.abs(expected)):actual===expected;
const normalized=value=>value.replace(/^\\\\\?\\/,'').replaceAll('/','\\').toLowerCase().replace(/\\+$/,'');

test(denied?'Assetto discovery fallback preserves the scoped telemetry permission gate':'expanded Assetto physics and canonical channels reach native processing and independent passive inspection',{skip:!enabled,timeout:300000},async t=>{
  assert.equal(process.platform,'win32');
  const connection=await nativePage(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224',process.env.OSG_NATIVE_PAGE_WS);
  t.after(()=>connection.close());
  await until(()=>connection.evaluate("typeof window.__TAURI_INTERNALS__?.invoke==='function'"),Boolean,60000);
  const directory=resolve('.research/assetto-telemetry-native',String(Date.now()));await mkdir(directory,{recursive:true});
  const evidence={actualNativeCockpit:true,authoredNativePublishers:true,ipcOnly:!denied,permissionDenial:denied,noPhysicalOutputs:true,observedAt:new Date().toISOString(),games:[],checks:[]};
  const native=async request=>{
    await writeFile(resolve(directory,'last-request.json'),JSON.stringify({command:request.command,gameId:request.gameId,resourceId:request.resourceId,offset:request.offset,at:new Date().toISOString()})+'\n');
    const response=await connection.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...request,clientId:'expanded-assetto-native-acceptance'})}})`);
    assert.equal(response.ok,true,`${request.command}: ${response.error}`);return response.state;
  };
  const wait=async(fn,label,timeout=25000)=>{const until=Date.now()+timeout;while(Date.now()<until){const result=await fn();if(result)return result;await sleep(150);}throw Error(label);};
  const games=async()=>(await native({command:'game_get_state'})).games;
  const inspect=async(gameId,query={})=>{
    const report=(await native({command:'game_telemetry_inspect',gameId,offset:0,limit:128,...query})).gameTelemetry;
    assert.equal(report.schemaVersion,1);assert.equal(report.games.length,7);
    const game=report.games.find(game=>game.id===gameId);assert.ok(game,`Missing named game ${gameId}`);
    return game;
  };
  const passive=async(gameId,id,expected)=>wait(async()=>{
    const game=await inspect(gameId,{search:id});const channel=game.channels.find(channel=>channel.id===id);
    return channel?.status==='known'&&near(channel.value,expected)&&{game,channel};
  },`${gameId} passive ${id} did not report ${JSON.stringify(expected)}`);
  const portsFor=async resourceId=>{
    const rows=[];let total=1;
    while(rows.length<total){const result=(await native({command:'profile_resources_inspect',resourceId,offset:rows.length,limit:128})).profileResources;total=result.total;assert.ok(result.ports.length||rows.length===total);rows.push(...result.ports);}
    assert.equal(rows.length,total);return rows;
  };
  const initial=await games(),initialProfiles=(await native({command:'profile_status'})).profiles;
  assert.ok(!initial.detectedGames?.some(game=>game.gameId.startsWith('assetto-corsa')),'Close real Assetto games before starting private fixture publishers.');
  await native({command:'profile_selection_mode',mode:'manual'});await native({command:'profile_stop'});

  try{
    for(const [gameId,name,basename] of denied ? permissionSpecs : specs){
      const working=resolve(directory,gameId);await mkdir(working,{recursive:true});
      if(gameId==='assetto-corsa'){
        const car=resolve(working,'content/cars/osg_expanded_car');await mkdir(resolve(car,'ui'),{recursive:true});await mkdir(resolve(car,'skins/default'),{recursive:true});
        await writeFile(resolve(car,'ui/ui_car.json'),JSON.stringify({name:'Authored Expanded Car'}));await writeFile(resolve(car,'data.acd'),'Authored presence fixture');await writeFile(resolve(car,'skins/default/livery.dds'),'Authored presence fixture');
      }
      const executable=resolve(working,basename),stop=resolve(working,'stop');
      await execute('C:\\Windows\\Microsoft.NET\\Framework64\\v4.0.30319\\csc.exe',['/nologo','/target:winexe',`/out:${executable}`,resolve('scripts/fixtures/assetto-telemetry/ExpandedProgram.cs')],{windowsHide:true});
      const previous=(await games()).catalogue.find(game=>game.id===gameId);
      const layout=gameId==='assetto-corsa-evo'?{physics:800,graphics:3940,static:208}:gameId==='assetto-corsa-competizione'?{physics:800,graphics:1588,static:820}:{physics:580,graphics:296,static:684};
      const record={gameId,name,layout,phases:{}};evidence.games.push(record);
      record.previousInstallation=previous?.selectedInstallationId;
      const checkpoint=async phase=>{record.lastPhase=phase;console.log(`${name}: ${phase}`);await writeFile(resolve(directory,'progress.json'),JSON.stringify(evidence,null,2)+'\n');};
      let child,exited,profile,installation,failed=false;
      async function stopFixture(){if(!child)return;const owned=child;await writeFile(stop,'stop');const timer=setTimeout(()=>owned.kill(),5000);try{await exited;}finally{clearTimeout(timer);child=null;}}
      const mode=async value=>{await writeFile(stop+'.mode',value);await wait(()=>readFile(stop+'.ack','utf8').then(actual=>actual===value,()=>false),`${name} publisher did not acknowledge ${value}`);};
      const profileSamples=async()=>{
        const session=(await native({command:'profile_status'})).profiles.profileSession;
        if(session.owner?.profileId!==profile?.id)return null;
        return (await native({command:'profile_runtime_inspect',owner:session.owner,offset:0,limit:64})).profileInspection;
      };
      let expected={};
      const liveProcessing=async()=>wait(async()=>{
        const snapshot=await profileSamples();
        return snapshot&&Object.entries(expected).every(([id,value])=>snapshot.ports.some(port=>port.endpoint.nodeId===`value-${id}`&&port.endpoint.portId==='out'&&port.sample.status==='known'&&near(port.sample.value.value,value)))&&snapshot;
      },`${name} expanded/canonical selected values did not reach processing`);
      const withdrawn=async label=>wait(async()=>{
        const snapshot=await profileSamples();
        return snapshot&&Object.keys(expected).every(id=>snapshot.ports.some(port=>port.endpoint.nodeId===`value-${id}`&&port.endpoint.portId==='out'&&port.sample.status!=='known'))&&snapshot;
      },`${name} ${label} retained known processing values`);
      try{
        const registered=(await native({command:'game_add',name:previous?.name??name,installPath:working,executable})).games;
        assert.equal(registered.addedGameId,gameId);
        installation=registered.catalogue.find(game=>game.id===gameId)?.installations.find(item=>normalized(item.installPath)===normalized(working));
        assert.ok(installation,`${gameId}: private installation registration`);
        await native({command:'game_installation_select',gameId,installationId:installation.id});
        child=spawn(executable,[gameId,stop],{windowsHide:true,stdio:'ignore'});
        exited=new Promise(resolve=>{child.once('exit',(code,signal)=>resolve({code,signal}));child.once('error',error=>resolve({error:String(error)}));});
        await new Promise((resolve,reject)=>{child.once('spawn',resolve);child.once('error',reject);});
        await wait(async()=>{if(child.exitCode!==null||child.signalCode!==null)throw Error(await readFile(stop+'.error','utf8').catch(()=>`Publisher exited ${child.exitCode}`));return access(stop+'.ready').then(()=>true,()=>false);},`${name} publisher never initialized`);
        await native({command:'game_integration_setup',gameId,connection:{freshnessMs:500}});
        const ready=await wait(async()=>{const game=(await games()).catalogue.find(game=>game.id===gameId);return game.integrationSetup?.state!=='setting-up'&&game;},`${name} setup never completed`);
        if(denied){
          assert.equal(ready.integrationSetup.verified,false,'Scoped permission denial must not verify shared memory');
          record.permission=await verifyScopedTelemetryDenial({page:connection,request:native,gameId});
          evidence.checks.push(`${name}: fallback discovered the process while scoped denial withheld telemetry and displayed Allow.`);
          continue;
        }
        assert.equal(ready.integrationSetup.verified,true,JSON.stringify(ready.integrationSetup));record.setup=ready.integrationSetup;
        await checkpoint('native setup verified');
        const unbound=await passive(gameId,'physics.wheelAngularSpeed.3',99.25);assert.equal(unbound.game.name,name);
        const summary=await inspect(gameId);assert.ok(summary.total>200);assert.ok(summary.groups.some(group=>group.id==='physics'&&group.total>100));record.passiveSummary=summary;
        const grouped=await inspect(gameId,{group:'physics',search:'accG'});assert.equal(grouped.total,3);assert.ok(grouped.channels.every(channel=>channel.category==='physics'));
        const metadata=gameId==='assetto-corsa-evo'?['graphics.driver_name','Authored Driver']:['static.playerName','Authored Driver'];
        record.unbound={angularSpeed:unbound.channel,metadata:(await passive(gameId,...metadata)).channel};
        if(gameId==='assetto-corsa-evo'){
          record.unbound.tyre=(await passive(gameId,'graphics.tyre_lf.tyre_temperature_c',91.5)).channel;
          record.unbound.guid=(await passive(gameId,'graphics.focused_car_id_a','18446744073709551615')).channel;
        }else record.unbound.timing=(await passive(gameId,'graphics.currentTime','00:42.123')).channel;
        if(gameId==='assetto-corsa-competizione'){
          const unsupported=await inspect(gameId,{search:'physics.mz.0'});const field=unsupported.channels.find(channel=>channel.id==='physics.mz.0');assert.equal(field.status,'unavailable');assert.equal(field.value,null);record.unused=field;
        }
        await checkpoint('passive unbound fields and named game verified');
        const resources=(await native({command:'profile_resources_list',offset:0,limit:128})).profileResources.resources;
        const resource=resources.find(resource=>resource.attributes.gameId===gameId);assert.ok(resource);
        const allPorts=await portsFor(resource.id);assert.ok(allPorts.length>200);assert.ok(allPorts.every(port=>port.descriptor.direction==='output'));
        for(const [id,unit] of [['engine.rpm','rpm'],['vehicle.speed_mps','m/s'],['vehicle.g_force.magnitude','g'],['controls.throttle','ratio']]){
          const port=allPorts.find(port=>port.descriptor.id===`channel:${id}`);assert.ok(port,`${name} missing canonical ${id}`);
          assert.equal(port.descriptor.contract.schema.type,'number');assert.equal(port.descriptor.contract.schema.unit,unit);
        }
        assert.equal(allPorts.find(port=>port.descriptor.id==='channel:rpm').descriptor.contract.schema.type,'integer','The legacy RPM contract must remain unchanged');
        expected={rpm:4500,speed_kmh:72,gear:2,max_rpm:9000,rev_ratio:0.5,'engine.rpm':4500,'vehicle.speed_mps':20,'vehicle.g_force.magnitude':5,'controls.throttle':0.25,'physics.accG.0':0,'physics.accG.1':3,'physics.accG.2':4,'physics.velocity.0':1.25,'physics.suspensionTravel.2':0.125,'physics.finalFF':-0.75};
        const fuel=gameId==='assetto-corsa-competizione'?['vehicle.fuel_kg','kg']:['vehicle.fuel_l','L'];
        const fuelPort=allPorts.find(port=>port.descriptor.id===`channel:${fuel[0]}`);assert.ok(fuelPort);assert.equal(fuelPort.descriptor.contract.schema.unit,fuel[1]);expected[fuel[0]]=35.5;
        if(gameId==='assetto-corsa-competizione')assert.ok(!allPorts.some(port=>port.descriptor.id==='channel:vehicle.fuel_l'),'Fuel mass must never masquerade as fuel volume');
        if(gameId==='assetto-corsa-evo'){expected['physics.mz.0']=-15.5;expected['graphics.is_rpm_limiter_on']=true;}
        await checkpoint('complete port catalogue and canonical contracts verified');
        const selected=Object.keys(expected).map(id=>{const port=allPorts.find(port=>port.descriptor.id===`channel:${id}`);assert.ok(port,`${name} missing ${id}`);return port;});
        const created=(await native({command:'profile_create',name:`Expanded ${name} ${Date.now()}`})).profiles.document;profile={id:created.id,revision:created.revision};record.profile=profile;
        await checkpoint('temporary profile created');
        created.eligibility={gameIds:[gameId]};
        created.resources.push({id:'telemetry',name:'Authored native physics',kind:'game',required:true,ports:selected.map(port=>({...port.descriptor,id:port.descriptor.id.slice(8)}))});
        for(const port of selected){const id=port.descriptor.id.slice(8),contract=port.descriptor.contract;
          created.processingNodes.push({id:`value-${id}`,name:`Observed ${id}`,operation:{kind:'identity'},inputs:[{id:'in',name:'Input',direction:'input',contract}],outputs:[{id:'out',name:'Output',direction:'output',contract}]});
          created.bindings.push({id:`route-${id}`,from:{nodeId:'telemetry',portId:id},to:{nodeId:`value-${id}`,portId:'in'}});
        }
        const assignments=[{resourceId:'telemetry',sourceId:resource.id,ports:selected.map(port=>({profilePortId:port.descriptor.id.slice(8),observedPortId:port.descriptor.id,expectedContractHash:port.contractHash}))}];
        assert.ok(!assignments[0].ports.some(port=>port.observedPortId==='channel:physics.wheelAngularSpeed.3'),'Passive proof must include an unbound field');
        const saved=(await native({command:'profile_editor_save',document:created,assignments,expectedRevision:created.revision})).profiles.document;profile.revision=saved.revision;
        await checkpoint('processing graph saved before apply');
        await native({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,gameId,expectedActiveOwner:null});
        record.phases.live=await liveProcessing();
        await checkpoint('selected native and canonical processing verified');
        await mode('invalid');
        record.phases.invalid=await wait(async()=>{const snapshot=await profileSamples();return snapshot&&snapshot.ports.some(port=>port.endpoint.nodeId==='value-engine.rpm'&&port.endpoint.portId==='out'&&port.sample.status==='known')&&['value-physics.accG.1','value-vehicle.g_force.magnitude'].every(id=>snapshot.ports.some(port=>port.endpoint.nodeId===id&&port.endpoint.portId==='out'&&port.sample.status!=='known'))&&snapshot;},`${name} invalid component poisoned valid fields or retained canonical magnitude`);
        await passive(gameId,'engine.rpm',4500);const bad=await inspect(gameId,{search:'physics.accG.1'});assert.equal(bad.channels.find(channel=>channel.id==='physics.accG.1').value,null);
        await mode('live');record.phases.recovered=await liveProcessing();
        await checkpoint('invalid-field isolation and recovery verified');
        await mode('paused');record.phases.paused=await withdrawn('pause');
        record.passivePaused=await wait(async()=>{const game=await inspect(gameId,{search:'engine.rpm'});return game.status!=='live'&&game.channels.every(channel=>channel.status!=='known'&&channel.value===null)&&game;},`${name} passive pause retained values`);
        await mode('live');record.phases.resumed=await liveProcessing();
        await checkpoint('pause withdrawal and resume verified');
        await mode('frozen');record.phases.stale=await withdrawn('frozen packet timeout');
        record.passiveStale=await wait(async()=>{const game=await inspect(gameId,{search:'engine.rpm'});return game.status!=='live'&&game.channels.every(channel=>channel.status!=='known'&&channel.value===null)&&game;},`${name} passive frozen packets retained values`);
        await mode('live');record.phases.rearmed=await liveProcessing();
        await checkpoint('stale packet withdrawal and rearm verified');
        const stillUnbound=await passive(gameId,'physics.wheelAngularSpeed.3',99.25);record.unboundAfterApply=stillUnbound.channel;
        await stopFixture();
        record.passiveExited=await wait(async()=>{const game=await inspect(gameId,{search:'engine.rpm'});return game.status!=='live'&&game.channels.every(channel=>channel.status!=='known'&&channel.value===null)&&game;},`${name} process exit retained passive values`);
        evidence.checks.push(`${name}: expanded native and canonical channels reach real profile processing; passive unbound fields, naming/groups, bad-field isolation, pause/stale withdrawal and resume/rearm pass.`);
        await checkpoint('process exit withdrawal verified');
      }catch(error){
        failed=true;record.error=String(error);throw error;
      }finally{
        const cleanupErrors=[];
        const cleanup=async(label,operation)=>{try{await operation();}catch(error){cleanupErrors.push(`${label}: ${error}`);}};
        await cleanup('stop fixture',stopFixture);
        if(profile){await cleanup('stop profile',()=>native({command:'profile_stop'}));await cleanup('delete profile',()=>native({command:'profile_delete',id:profile.id,expectedRevision:profile.revision}));}
        if(installation)await cleanup('restore installation',async()=>{
          if(previous?.selectedInstallationId)await native({command:'game_installation_select',gameId,installationId:previous.selectedInstallationId});
          assert.ok(working.startsWith(directory+sep));
          assert.equal(normalized(installation.installPath),normalized(working));
          await rm(working,{recursive:true,force:true});
          if(!previous){
            await native({command:'game_remove',gameId});
            assert.ok(!(await games()).catalogue.some(game=>game.id===gameId));
            return;
          }
          const scan=async paths=>{
            await native({command:'game_discovery_start',paths});
            return wait(async()=>{const state=(await games()).discovery;return state.status==='ready'&&state;},'Fixture removal discovery did not finish',90000);
          };
          await wait(async()=>(await games()).discovery?.status!=='scanning','Existing discovery did not finish',90000);
          let discovery=await scan([]);
          if(!discovery.libraries.some(library=>library.id==='disk'))discovery=await scan([directory]);
          const removal=discovery.entries.find(item=>item.action==='remove'&&item.gameId===gameId&&item.installationId===installation.id);
          assert.ok(removal,'Discovery must offer removal of exactly the private installation.');
          await native({command:'game_discovery_apply',scanId:discovery.scanId,actions:discovery.entries.map(item=>({id:item.id,selected:item.id===removal.id}))});
          assert.deepEqual((await games()).catalogue.find(game=>game.id===gameId)?.installations,previous.installations);
        });
        if(cleanupErrors.length){record.cleanupErrors=cleanupErrors;if(!failed)throw Error(cleanupErrors.join('; '));}
      }
    }
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');console.log(JSON.stringify({directory,checks:evidence.checks}));
  }catch(error){await writeFile(resolve(directory,'failure.json'),JSON.stringify({...evidence,error:String(error),state:await games().catch(()=>null)},null,2)+'\n');throw error;}
  finally{await native({command:'profile_selection_mode',mode:initialProfiles.session.selectionMode});}
});
