// Opt-in IPC-only acceptance in the existing native dev Cockpit. A private
// headless process and authored RREF UDP exercise production acquisition and
// processing; this does not qualify a real simulator or any physical output.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createSocket} from 'node:dgram';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';

const enabled=process.env.OSG_XPLANE_TELEMETRY_NATIVE==='1',execute=promisify(execFile);
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const near=(actual,expected)=>typeof actual==='number'&&Math.abs(actual-expected)<1e-5*Math.max(1,Math.abs(expected));
async function wait(read,label,timeout=30000){const end=Date.now()+timeout;while(Date.now()<end){const value=await read();if(value)return value;await sleep(150);}throw Error(label);}

async function connect(){
  let endpoint=process.env.OSG_NATIVE_PAGE_WS;
  if(!endpoint){
    const response=await fetch(`${process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224'}/json/list`,{signal:AbortSignal.timeout(5000)});
    endpoint=(await response.json()).find(target=>target.type==='page'&&/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(target.url)&&!target.url.includes('profile-activation-window'))?.webSocketDebuggerUrl;
  }
  assert.ok(endpoint,'Start the actual native dev Cockpit with CDP enabled.');
  const socket=new WebSocket(endpoint),pending=new Map();let sequence=0;
  await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});
  socket.addEventListener('message',event=>{const message=JSON.parse(event.data),operation=pending.get(message.id);if(!operation)return;clearTimeout(operation.timer);pending.delete(message.id);message.error?operation.reject(Error(message.error.message)):operation.resolve(message.result);});
  socket.addEventListener('close',()=>{for(const operation of pending.values()){clearTimeout(operation.timer);operation.reject(Error('Native page connection closed.'));}pending.clear();});
  return{
    async evaluate(expression){
      const result=await new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error('Native IPC timed out.'));},90000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method:'Runtime.evaluate',params:{expression,returnByValue:true,awaitPromise:true}}));});
      if(result.exceptionDetails)throw Error(result.exceptionDetails.exception?.description??result.exceptionDetails.text);return result.result.value;
    },close(){socket.close();},
  };
}

async function publisher(version){
  const socket=createSocket('udp4'),clients=new Map(),events=[],started=performance.now();let mode='live',frozenClock=0,maxSubscriptions=0;
  socket.on('message',(bytes,remote)=>{
    if(bytes.length!==413||bytes.subarray(0,5).toString()!=='RREF\0')return;
    const hz=bytes.readInt32LE(5),index=bytes.readInt32LE(9),path=bytes.subarray(13).toString().split('\0')[0],key=`${remote.address}:${remote.port}`;
    let client=clients.get(key);if(!client){client={remote,subscriptions:new Map()};clients.set(key,client);}
    if(hz===0)client.subscriptions.delete(index);else client.subscriptions.set(index,path);
    maxSubscriptions=Math.max(maxSubscriptions,client.subscriptions.size);
    if(events.length<40000)events.push({at:performance.now(),client:key,hz,index,path});
  });
  await new Promise((resolve,reject)=>{socket.once('error',reject);socket.bind(0,'127.0.0.1',resolve);});
  const values={
    'sim/version/xplane_internal_version':version*10000,'sim/time/is_in_replay':0,'sim/aircraft/engine/acf_num_engines':1,
    'sim/cockpit2/engine/indicators/engine_speed_rpm[0]':4500,'sim/flightmodel/position/groundspeed':20,
    'sim/flightmodel/position/true_airspeed':25,'sim/flightmodel/position/elevation':1234.5,
    'sim/flightmodel/forces/g_side':0,'sim/flightmodel/forces/g_nrml':3,'sim/flightmodel/forces/g_axil':4,
    'sim/flightmodel/position/local_vx':1.25,'sim/flightmodel/position/local_ax':2.25,
    'sim/flightmodel/forces/fnrml_total':-125.5,'sim/flightmodel/engine/ENGN_power[0]':12.5,
  };
  const value=path=>{
    const character=/^sim\/aircraft\/view\/acf_relative_path\[(\d+)\]$/.exec(path);
    if(character)return 'Aircraft/Authored/Physics.acf'.charCodeAt(Number(character[1]))||0;
    if(path==='sim/time/paused')return mode==='paused'?1:0;
    if(path==='sim/time/total_running_time_sec')return mode==='frozen'?frozenClock:1+(performance.now()-started)/1000;
    return values[path]??0;
  };
  const timer=setInterval(()=>{
    if(mode==='silent')return;
    for(const {remote,subscriptions} of clients.values()){
      const rows=[...subscriptions].filter(([,path])=>mode!=='partial'||path!=='sim/flightmodel/forces/g_nrml');
      // Bounded, separate datagrams deliberately exercise partial RREF packets.
      for(let start=0;start<rows.length;start+=60){
        const slice=rows.slice(start,start+60),bytes=Buffer.alloc(5+8*slice.length);bytes.write('RREF,');
        for(let i=0;i<slice.length;i++){bytes.writeInt32LE(slice[i][0],5+i*8);bytes.writeFloatLE(value(slice[i][1]),9+i*8);}
        socket.send(bytes,remote.port,remote.address,()=>{});
      }
    }
  },50);
  return{
    port:socket.address().port,events,clients,
    mode(next){if(next==='frozen')frozenClock=1+(performance.now()-started)/1000;mode=next;},
    subscribed(path){return [...clients.values()].some(client=>[...client.subscriptions.values()].includes(path));},
    stats(){
      const perClient=new Map();let maxRequestsIn95ms=0;
      for(const event of events){let window=perClient.get(event.client);if(!window){window=[];perClient.set(event.client,window);}while(window.length&&event.at-window[0]>=95)window.shift();window.push(event.at);maxRequestsIn95ms=Math.max(maxRequestsIn95ms,window.length);}
      return{requests:events.length,clients:clients.size,maxSubscriptions,maxRequestsIn95ms,maxHz:Math.max(0,...events.map(event=>event.hz))};
    },
    close(){clearInterval(timer);socket.close();},
  };
}

test('full installed X-Plane metadata supplies selected physics and canonical channels to native processing',{skip:!enabled,timeout:600000},async t=>{
  assert.equal(process.platform,'win32');
  const directory=resolve('.research/xplane-telemetry-native',String(Date.now()));await mkdir(directory,{recursive:true});
  const cdp=await connect();t.after(()=>cdp.close());
  await wait(async()=>{try{return await cdp.evaluate("typeof window.__TAURI_INTERNALS__?.invoke==='function'");}catch{return false;}},'Native Cockpit IPC did not initialize',60000);
  const evidence={actualNativeCockpit:true,authoredRrefPublisher:true,actualSimulatorQualified:false,ipcOnly:true,noPhysicalOutputs:true,games:[],checks:[]};
  const request=async value=>{
    await writeFile(resolve(directory,'last-request.json'),JSON.stringify({command:value.command,gameId:value.gameId,offset:value.offset,at:new Date().toISOString()})+'\n');
    const reply=await cdp.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'expanded-xplane-native-acceptance'})}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;
  };
  const gameState=async()=>(await request({command:'game_get_state'})).games;
  const initial=await gameState(),initialProfiles=(await request({command:'profile_status'})).profiles;
  assert.ok(!initial.detectedGames?.some(game=>game.gameId.startsWith('x-plane-')),'Close real X-Plane processes before starting a private native fixture.');
  const sourcePath=resolve(process.env.OSG_XPLANE_DATAREFS??'C:/Program Files (x86)/Steam/steamapps/common/X-Plane 12/Resources/plugins/DataRefs.txt');
  const installed=await readFile(sourcePath,'utf8');assert.ok(installed.split('\n').filter(line=>line.startsWith('sim/')).length>5000,'Use a complete installed SDK DataRefs.txt, rather than a reduced test catalogue.');
  evidence.metadataSource={path:sourcePath,header:installed.split(/\r?\n/)[0]};
  const processSource=resolve(directory,'XPlaneFixture.cs');
  await writeFile(processSource,'using System; using System.IO; using System.Threading; public static class XPlaneTelemetryFixture { public static void Main(string[] args) { if(args.Length!=1)return; var until=DateTime.UtcNow.AddMinutes(10); while(DateTime.UtcNow<until&&!File.Exists(args[0]))Thread.Sleep(50); } }');
  if(['faulted','recovery-needed'].includes(initialProfiles.profileSession.state))await request({command:'profile_recover'});
  await request({command:'profile_selection_mode',mode:'manual'});await request({command:'profile_stop'});
  try{
    for(const version of [11,12]){
      const gameId=`x-plane-${version}`,name=`X-Plane ${version}`,working=resolve(directory,gameId),executable=resolve(working,'X-Plane.exe'),stop=resolve(working,'stop');
      await mkdir(resolve(working,'Resources/plugins'),{recursive:true});
      // Only the private copy changes its installation version header. Both
      // fixtures retain the full audited installed numeric SDK definitions.
      await writeFile(resolve(working,'Resources/plugins/DataRefs.txt'),installed.replace(/^[^\r\n]*/,`2 ${version}00 Authored native acceptance installation`));
      await execute('C:\\Windows\\Microsoft.NET\\Framework64\\v4.0.30319\\csc.exe',['/nologo','/target:winexe',`/out:${executable}`,processSource],{windowsHide:true});
      const beforeState=await gameState(),before=beforeState.manualInstallations?.[gameId]??null,beforeGame=beforeState.catalogue.find(game=>game.id===gameId);
      const telemetry=await publisher(version),record={gameId,name,previousExecutable:before,previousConnection:beforeGame.integrationSetup.connection,originallyInstalled:beforeGame.installed,phases:{}};evidence.games.push(record);
      let child,exited,profile,failed=false;
      const checkpoint=async phase=>{record.lastPhase=phase;console.log(`${name}: ${phase}`);await writeFile(resolve(directory,'progress.json'),JSON.stringify(evidence,null,2)+'\n');};
      const stopFixture=async()=>{if(!child)return;const owned=child;await writeFile(stop,'stop');const timer=setTimeout(()=>owned.kill(),5000);try{await exited;}finally{clearTimeout(timer);child=null;}};
      const inspect=async(query={})=>{const report=(await request({command:'game_telemetry_inspect',gameId,offset:0,limit:128,...query})).gameTelemetry;const game=report.games.find(game=>game.id===gameId);assert.equal(game.name,name);return game;};
      const passive=async(id,expected)=>wait(async()=>{const game=await inspect({search:id}),channel=game.channels.find(channel=>channel.id===id);return channel?.status==='known'&&near(channel.value,expected)&&channel;},`${name} passive ${id} unavailable`);
      const expected={'engine_1_rpm':4500,'engine.rpm':4500,'vehicle.speed_mps':20,'aircraft.true_airspeed_mps':25,'aircraft.altitude_msl_m':1234.5,'vehicle.g_force.magnitude':5,'sim/flightmodel/forces/g_side':0,'sim/flightmodel/forces/g_nrml':3,'sim/flightmodel/forces/g_axil':4,'sim/flightmodel/position/local_vx':1.25,'sim/flightmodel/forces/fnrml_total':-125.5,'sim/flightmodel/engine/ENGN_power[0]':12.5};
      const nodeFor=id=>`value-${Object.keys(expected).indexOf(id)}`;
      const samples=async()=>{const session=(await request({command:'profile_status'})).profiles.profileSession;if(session.owner?.profileId!==profile?.id)return null;return(await request({command:'profile_runtime_inspect',owner:session.owner,offset:0,limit:64})).profileInspection;};
      const known=(snapshot,id,expected)=>snapshot.ports.some(port=>port.endpoint.nodeId===nodeFor(id)&&port.endpoint.portId==='out'&&port.sample.status==='known'&&near(port.sample.value.value,expected));
      const unavailable=(snapshot,id)=>snapshot.ports.some(port=>port.endpoint.nodeId===nodeFor(id)&&port.endpoint.portId==='out'&&port.sample.status!=='known');
      const live=()=>wait(async()=>{const snapshot=await samples();return snapshot&&Object.entries(expected).every(([id,value])=>known(snapshot,id,value))&&snapshot;},`${name} selected physics/canonical values did not reach processing`);
      const withdrawn=label=>wait(async()=>{const snapshot=await samples();return snapshot&&Object.keys(expected).every(id=>unavailable(snapshot,id))&&snapshot;},`${name} ${label} retained known processing values`);
      try{
        await request({command:'game_installation',gameId,executable});
        await request({command:'game_integration_setup',gameId,connection:{host:'127.0.0.1',port:telemetry.port,listenPort:0,updateHz:20,freshnessMs:500}});
        record.setup=await wait(async()=>{const game=(await gameState()).catalogue.find(game=>game.id===gameId);return game.integrationSetup?.state==='ready'&&game.integrationSetup;},`${name} installation setup never completed`);
        child=spawn(executable,[stop],{cwd:working,windowsHide:true,stdio:'ignore'});exited=new Promise(resolve=>{child.once('exit',(code,signal)=>resolve({code,signal}));child.once('error',error=>resolve({error:String(error)}));});
        await new Promise((resolve,reject)=>{child.once('spawn',resolve);child.once('error',reject);});
        await wait(async()=>{const game=(await gameState()).catalogue.find(game=>game.id===gameId);return game.integrationContext.processVerified&&game.currentVehicle?.id==='Aircraft/Authored/Physics.acf';},`${name} native process and advancing aircraft were not observed`);
        record.passiveSummary=await inspect({search:'sim/flightmodel/position/local_vx'});assert.ok(record.passiveSummary.total===1);assert.ok(record.passiveSummary.groups.some(group=>group.id==='physics'&&group.total>1000));
        record.passiveVelocity=await passive('sim/flightmodel/position/local_vx',1.25);
        record.passiveAcceleration=await passive('sim/flightmodel/position/local_ax',2.25);
        await wait(()=>!telemetry.subscribed('sim/flightmodel/position/local_vx')&&telemetry.events.some(event=>event.hz===0&&event.path==='sim/flightmodel/position/local_vx'),'Changing the passive page did not cancel its prior dataref');
        await checkpoint('full metadata and on-demand passive subscription replacement verified');
        const resources=(await request({command:'profile_resources_list',offset:0,limit:128})).profileResources.resources,resource=resources.find(resource=>resource.attributes.gameId===gameId);assert.ok(resource);
        const pages=new Map();let total;
        const portAt=async offset=>{if(!pages.has(offset)){const page=(await request({command:'profile_resources_inspect',resourceId:resource.id,offset,limit:1})).profileResources;total??=page.total;assert.equal(page.total,total);pages.set(offset,page.ports[0]);}return pages.get(offset);};
        await portAt(0);assert.ok(total>60000,'Complete installed SDK component metadata must be available');record.portCount=total;
        const select=async id=>{const target=`channel:${id}`;let low=0,high=total-1;while(low<=high){const middle=Math.floor((low+high)/2),port=await portAt(middle),observed=port.descriptor.id;if(observed===target)return port;if(observed<target)low=middle+1;else high=middle-1;}throw Error(`${name} missing native port ${id}`);};
        const selected=[];for(const id of Object.keys(expected))selected.push(await select(id));
        for(const [id,unit] of [['engine.rpm','rpm'],['vehicle.speed_mps','m/s'],['aircraft.true_airspeed_mps','m/s'],['aircraft.altitude_msl_m','m'],['vehicle.g_force.magnitude','g']])assert.equal(selected.find(port=>port.descriptor.id===`channel:${id}`).descriptor.contract.schema.unit,unit);
        const created=(await request({command:'profile_create',name:`Expanded ${name} ${Date.now()}`})).profiles.document;profile={id:created.id,revision:created.revision};record.profile=profile;
        created.eligibility={gameIds:[gameId]};created.resources.push({id:'telemetry',name:'Selected native physics',kind:'game',required:true,ports:selected.map((port,index)=>({...port.descriptor,id:`field-${index}`}))});
        for(const [index,port] of selected.entries()){const id=port.descriptor.id.slice(8),contract=port.descriptor.contract;created.processingNodes.push({id:nodeFor(id),name:id,operation:{kind:'identity'},inputs:[{id:'in',name:'Input',direction:'input',contract}],outputs:[{id:'out',name:'Output',direction:'output',contract}]});created.bindings.push({id:`route-${index}`,from:{nodeId:'telemetry',portId:`field-${index}`},to:{nodeId:nodeFor(id),portId:'in'}});}
        const assignments=[{resourceId:'telemetry',sourceId:resource.id,ports:selected.map((port,index)=>({profilePortId:`field-${index}`,observedPortId:port.descriptor.id,expectedContractHash:port.contractHash}))}];
        const saved=(await request({command:'profile_editor_save',document:created,assignments,expectedRevision:created.revision})).profiles.document;profile.revision=saved.revision;
        await request({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,gameId,expectedActiveOwner:null});
        record.phases.live=await live();await checkpoint('dynamic scalar/array physics and canonical processing verified');
        telemetry.mode('partial');record.phases.partial=await wait(async()=>{const snapshot=await samples();return snapshot&&known(snapshot,'engine.rpm',4500)&&known(snapshot,'vehicle.speed_mps',20)&&unavailable(snapshot,'sim/flightmodel/forces/g_nrml')&&unavailable(snapshot,'vehicle.g_force.magnitude')&&snapshot;},`${name} partial data retained missing physics or invalidated independent channels`);
        telemetry.mode('live');record.phases.partialRecovery=await live();
        telemetry.mode('paused');record.phases.paused=await withdrawn('pause');telemetry.mode('live');record.phases.resumed=await live();
        telemetry.mode('frozen');record.phases.frozen=await withdrawn('frozen simulator clock');telemetry.mode('live');record.phases.rearmed=await live();
        telemetry.mode('silent');record.phases.stale=await withdrawn('silent UDP timeout');telemetry.mode('live');record.phases.reconnected=await live();
        record.unboundAfterApply=await passive('sim/flightmodel/position/local_ax',2.25);assert.ok(!Object.hasOwn(expected,'sim/flightmodel/position/local_ax'));
        await checkpoint('partial isolation, pause, clock freeze, UDP timeout and recovery verified');
        await stopFixture();record.phases.exited=await withdrawn('process exit');
        record.passiveExited=await wait(async()=>{const game=await inspect({search:'sim/flightmodel/position/local_ax'});return game.status!=='live'&&game.channels.every(channel=>channel.status!=='known'&&channel.value===null)&&game;},`${name} exit retained passive values`);
        record.transport=telemetry.stats();assert.ok(record.transport.maxSubscriptions<200,'A reader must subscribe only guards and selected components, never the whole catalogue');assert.equal(record.transport.maxHz,20);assert.ok(record.transport.maxRequestsIn95ms<=100,JSON.stringify(record.transport));
        evidence.checks.push(`${name}: full installed metadata, on-demand passive RREF replacement, selected scalar/array physics and canonical processing; partial, pause, clock freeze, UDP timeout and exit withdrawal/recovery pass.`);
        await checkpoint('native exit withdrawal and bounded subscription pacing verified');
      }catch(error){failed=true;record.error=String(error);throw error;}
      finally{
        const cleanupErrors=[],cleanup=async(label,operation)=>{try{await operation();}catch(error){cleanupErrors.push(`${label}: ${error}`);}};
        await cleanup('stop fixture',stopFixture);if(profile){await cleanup('stop profile',()=>request({command:'profile_stop'}));await cleanup('delete profile',()=>request({command:'profile_delete',id:profile.id,expectedRevision:profile.revision}));}
        await cleanup('restore installation',()=>request({command:'game_installation',gameId,executable:before}));
        await cleanup('restore provider settings',async()=>{
          if(!beforeGame.installed){await request({command:'game_integration_remove',gameId});return;}
          const current=(await gameState()).catalogue.find(game=>game.id===gameId);
          await request({command:'game_integration_settings_save',gameId,connection:beforeGame.integrationSetup.connection,expectedSetupRevision:current.integrationSetup.providerSetupRevision});
        });
        record.transport=telemetry.stats();record.subscriptionEvents=telemetry.events;telemetry.close();
        if(cleanupErrors.length){record.cleanupErrors=cleanupErrors;if(!failed)throw Error(cleanupErrors.join('; '));}
      }
    }
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');console.log(JSON.stringify({directory,checks:evidence.checks}));
  }catch(error){await writeFile(resolve(directory,'failure.json'),JSON.stringify({...evidence,error:String(error),state:await gameState().catch(()=>null)},null,2)+'\n');throw error;}
  finally{await request({command:'profile_selection_mode',mode:initialProfiles.session.selectionMode});}
});
