import {parseArgs} from 'node:util';
import {createHash} from 'node:crypto';
import {createReadStream} from 'node:fs';
import {mkdir,readFile,realpath,stat,writeFile} from 'node:fs/promises';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {resolve,basename,join} from 'node:path';
import {fileURLToPath} from 'node:url';

// Actual-game qualification only. This script never launches a game, publishes
// shared memory, changes installation selections, or drives the game UI.
const games={
  'assetto-corsa':{name:'Assetto Corsa',appId:244210,images:['acs.exe','acs_x86.exe']},
  'assetto-corsa-competizione':{name:'Assetto Corsa Competizione',appId:805550,images:['AC2-Win64-Shipping.exe']},
  'assetto-corsa-evo':{name:'Assetto Corsa EVO',appId:3058630,images:['AssettoCorsaEVO.exe']},
};
const aliases={ac:'assetto-corsa',acc:'assetto-corsa-competizione',evo:'assetto-corsa-evo'};
const channels=['rpm','gear','speed_kmh','max_rpm','rev_ratio'],required=['rpm','gear','speed_kmh'];
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms)),execute=promisify(execFile);
const repository=fileURLToPath(new URL('../',import.meta.url));
const help=`Usage: node scripts/assetto-live.mjs <ac|acc|evo> [options]

Run capture after entering a real live driving session. It verifies native setup,
creates and applies a telemetry-only profile, records repeated delivered values,
and removes its profile afterward. No game or installation settings are changed.

  --mode capture|reapply|observe|pause|stale|exit|snapshot|cleanup  (default capture)
  --seconds 15             Observation duration (default 10)
  --interval-ms 500        Sample interval (100..5000)
  --timeout-ms 30000       Wait for setup/live or requested state
  --min-speed 0            Require speed >= value in a live capture
  --wait-speed             Start recording only after reaching positive min-speed
  --require-change         Require RPM span >=100 or speed span >=1 km/h
  --require-all            Require all five channels; otherwise redline is optional
  --require-game-closed    Exit mode also requires the whole game/menu to close
  --keep-profile           Keep the capture profile active for follow-up modes
  --profile-id UUID        Reuse that profile for reapply/observe/pause/stale/exit/cleanup
  --from evidence.json     Reuse profile metadata from a previous capture
  --out DIRECTORY          Evidence destination (default .research/assetto-live/...)
  --cdp-url URL            Running native Cockpit CDP (default localhost:9222)

Example:
  node scripts/assetto-live.mjs ac --seconds 15 --min-speed 5 --require-change --keep-profile
  node scripts/assetto-live.mjs ac --mode reapply --from <capture>/evidence.json --require-all
  node scripts/assetto-live.mjs ac --mode pause --from <capture>/evidence.json
  node scripts/assetto-live.mjs ac --mode exit --from <capture>/evidence.json
  node scripts/assetto-live.mjs ac --mode cleanup --from <capture>/evidence.json

Pause/stale/exit only observe the state you establish in the real game. They never
pause, suspend, close, or send inputs to it. Snapshot is completely read-only.
`;

function options(){
  const {positionals,values}=parseArgs({allowPositionals:true,options:{
    help:{type:'boolean',short:'h'},mode:{type:'string',default:'capture'},seconds:{type:'string',default:'10'},
    'interval-ms':{type:'string',default:'500'},'timeout-ms':{type:'string',default:'30000'},'min-speed':{type:'string',default:'0'},
    'require-change':{type:'boolean'},'require-all':{type:'boolean'},'wait-speed':{type:'boolean'},'require-game-closed':{type:'boolean'},'keep-profile':{type:'boolean'},
    'profile-id':{type:'string'},from:{type:'string'},out:{type:'string'},'cdp-url':{type:'string'},
  }});
  if(values.help){console.log(help);return null;}
  const gameId=aliases[positionals[0]]??positionals[0];
  if(positionals.length!==1||!games[gameId])throw Error('Choose ac, acc, or evo. Use --help for usage.');
  if(!['capture','reapply','observe','pause','stale','exit','snapshot','cleanup'].includes(values.mode))throw Error('Unsupported observation mode.');
  const number=(key,min,max)=>{const value=Number(values[key]);if(!Number.isFinite(value)||value<min||value>max)throw Error(`Invalid --${key}.`);return value;};
  const config={gameId,...values,seconds:number('seconds',1,600),interval:number('interval-ms',100,5000),timeout:number('timeout-ms',1000,180000),minSpeed:number('min-speed',0,2000)};
  if(config['wait-speed']&&config.minSpeed<=0)throw Error('--wait-speed requires a positive --min-speed.');
  if(config['wait-speed']&&!['capture','reapply','observe'].includes(config.mode))throw Error('--wait-speed applies only to capture, reapply, or observe.');
  return config;
}

async function connect(endpoint){
  const response=await fetch(`${endpoint.replace(/\/$/,'')}/json/list`,{signal:AbortSignal.timeout(10000)});
  if(!response.ok)throw Error(`Cannot list native Cockpit CDP targets: ${response.status}`);
  const targets=await response.json(),target=targets.find(t=>t.type==='page'&&t.url.includes(':1420')&&!t.url.includes('profile-activation-window'));
  if(!target)throw Error('No running native dev Cockpit target found on port 1420.');
  const socket=new WebSocket(target.webSocketDebuggerUrl),pending=new Map();let sequence=0;
  await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});
  socket.addEventListener('message',event=>{const value=JSON.parse(String(event.data)),item=pending.get(value.id);if(!item)return;pending.delete(value.id);clearTimeout(item.timer);value.error?item.reject(Error(value.error.message)):item.resolve(value.result);});
  socket.addEventListener('close',()=>{for(const item of pending.values()){clearTimeout(item.timer);item.reject(Error('Native Cockpit CDP connection closed.'));}pending.clear();});
  const send=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error(`${method} timed out.`));},60000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
  const native=async request=>{
    const expression=`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...request,clientId:'assetto-real-game-verification'})}})`;
    const result=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});
    if(result.exceptionDetails)throw Error(result.exceptionDetails.exception?.description??result.exceptionDetails.text);
    const value=result.result.value;if(!value?.ok)throw Error(`${request.command}: ${value?.error??'Invalid native response'}`);return value.state;
  };
  return {native,send,target,close:()=>socket.close()};
}

async function executableEvidence(game){
  const path=await realpath(game.executable),metadata=await stat(path),normalized=path.replaceAll('\\','/').toLowerCase();
  if(normalized.startsWith(repository.replaceAll('\\','/').toLowerCase())||/[\\/](?:\.research|\.tmp|fixtures)[\\/]/i.test(path)||metadata.size<1048576)throw Error('Live qualification requires an actual installed game executable, not an authored publisher.');
  const spec=games[game.id];if(!spec.images.some(image=>image.toLowerCase()===basename(path).toLowerCase()))throw Error('The selected executable is not this game’s driving runtime.');
  const hash=createHash('sha256');for await(const chunk of createReadStream(path))hash.update(chunk);
  const script=`$item=Get-Item -LiteralPath $env:OSG_ASSETTO_LIVE_EXECUTABLE; $processes=@(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath -eq $item.FullName } | ForEach-Object { [pscustomobject]@{pid=$_.ProcessId;parentPid=$_.ParentProcessId;path=$_.ExecutablePath;createdUtc=$_.CreationDate.ToUniversalTime().ToString('o')} }); [pscustomobject]@{fileVersion=$item.VersionInfo.FileVersion;productVersion=$item.VersionInfo.ProductVersion;processes=$processes} | ConvertTo-Json -Depth 5 -Compress`;
  const result=await execute('powershell.exe',['-NoProfile','-NonInteractive','-Command',script],{windowsHide:true,env:{...process.env,OSG_ASSETTO_LIVE_EXECUTABLE:path.startsWith('\\\\?\\')?path.slice(4):path},timeout:15000,maxBuffer:1048576});
  const native=JSON.parse(result.stdout.trim());
  return {path,fileBytes:metadata.size,modifiedUtc:metadata.mtime.toISOString(),sha256:hash.digest('hex'),steamAppId:game.steamAppId,...native};
}

function samplesFrom(inspection){
  return Object.fromEntries(channels.map(id=>{
    const port=inspection?.ports?.find(p=>p.endpoint?.nodeId===`observed-${id}`&&p.endpoint.portId==='out');
    return [id,{status:port?.sample?.status??'missing',value:port?.sample?.status==='known'?port.sample.value?.value:null,observedUs:port?.observedUs??null}];
  }));
}
export function readyForObservation(row,config){
  const unavailable=['pause','stale','exit'].includes(config.mode),requested=config['require-all']?channels:required;
  if(!requested.every(id=>unavailable?row.channels[id].status!=='known':row.channels[id].status==='known'&&Number.isFinite(row.channels[id].value)))return false;
  if(unavailable&&row.vehicle)return false;
  if(config.mode==='exit'&&(row.processVerified||(config['require-game-closed']&&row.detected)))return false;
  if(!unavailable&&config['wait-speed']&&row.channels.speed_kmh.value<config.minSpeed)return false;
  return true;
}
export function summarize(rows,mode,config){
  const channelSummary=Object.fromEntries(channels.map(id=>{
    const known=rows.map(row=>row.channels[id]).filter(sample=>sample.status==='known'&&Number.isFinite(sample.value));
    return [id,{knownSamples:known.length,totalSamples:rows.length,minimum:known.length?Math.min(...known.map(s=>s.value)):null,maximum:known.length?Math.max(...known.map(s=>s.value)):null,distinctValues:new Set(known.map(s=>s.value)).size}];
  }));
  const live=rows.filter(row=>required.every(id=>row.channels[id].status==='known'&&Number.isFinite(row.channels[id].value)));
  const withdrawalChannels=config['require-all']?channels:required;
  const stopped=rows.filter(row=>withdrawalChannels.every(id=>row.channels[id].status!=='known'));
  const ratioErrors=live.filter(row=>row.channels.max_rpm.status==='known'&&row.channels.rev_ratio.status==='known'&&Math.abs(row.channels.rev_ratio.value-row.channels.rpm.value/row.channels.max_rpm.value)>0.000001);
  const problems=[];
  if(['capture','reapply','observe'].includes(mode)){
    if(live.length<2)problems.push('Fewer than two samples delivered live RPM, gear, and speed.');
    if(live.length<Math.ceil(rows.length*0.8))problems.push('Live RPM, gear, and speed were available in fewer than 80% of recorded samples.');
    if(!live.some(row=>row.vehicle?.id))problems.push('No live car identity accompanied the channel samples.');
    if(config['require-all']&&rows.filter(row=>channels.every(id=>row.channels[id].status==='known'&&Number.isFinite(row.channels[id].value))).length<Math.max(2,Math.ceil(rows.length*0.8)))problems.push('All five channels were required but not delivered together in at least 80% of samples.');
    if(!live.some(row=>row.channels.speed_kmh.value>=config.minSpeed))problems.push(`Speed did not reach ${config.minSpeed} km/h.`);
    if(config['require-change']&&(channelSummary.rpm.maximum-channelSummary.rpm.minimum<100)&&(channelSummary.speed_kmh.maximum-channelSummary.speed_kmh.minimum<1))problems.push('No material RPM or speed change was recorded (minimum spans: 100 RPM or 1 km/h).');
    if(ratioErrors.length)problems.push('Reported rev ratio did not equal RPM / maximum RPM.');
  }else if(['pause','stale','exit'].includes(mode)){
    if(stopped.length<2)problems.push('Fewer than two samples withdrew all requested live channels.');
    if(stopped.length!==rows.length)problems.push('Live channel values remained known during an unavailable-state observation.');
    if(stopped.some(row=>row.vehicle))problems.push('A current vehicle survived the unavailable telemetry state.');
    if(mode==='exit'&&rows.some(row=>row.processVerified))problems.push('The driving runtime remained verified during exit observation.');
    if(mode==='exit'&&config['require-game-closed']&&rows.some(row=>row.detected))problems.push('The whole application was required to close, but its game/menu process remains detected.');
  }
  return {passed:problems.length===0,problems,channelSummary,liveSamples:live.length,withdrawnSamples:stopped.length,...(mode==='exit'?{exitScope:config['require-game-closed']?'whole-application':'driving-runtime',applicationDetected:rows.some(row=>row.detected),drivingRuntimeVerified:rows.some(row=>row.processVerified)}:{})};
}

export async function verifyLiveSetup(native,gameId,catalogue,wait,evidence){
  // Retire the old owner before setup so Apply can capture a restarted process.
  await native({command:'profile_selection_mode',mode:'manual'});
  await native({command:'profile_stop'});
  await native({command:'game_integration_setup',gameId});
  const configured=await wait(async()=>{const {game}=await catalogue();return game.integrationSetup.state!=='setting-up'&&game;},'Native integration setup did not finish.');
  evidence.setup=configured.integrationSetup;
  if(!configured.integrationSetup.verified)throw Error('Native game build remains unverified: '+configured.integrationSetup.message);
  await native({command:'game_integration_check',gameId});
  const checked=await wait(async()=>{const {game}=await catalogue();return game.integrationSetup.state!=='setting-up'&&game;},'Native integration check did not finish.');
  evidence.checkedSetup=checked.integrationSetup;
  if(!checked.integrationSetup.verified)throw Error('Native verification failed: '+checked.integrationSetup.message);
}

async function main(config){
  const output=resolve(config.out??join(repository,'.research','assetto-live',`${new Date().toISOString().replaceAll(':','-')}-${config.gameId}-${config.mode}`));
  await mkdir(output,{recursive:true});
  const evidence={actualGame:true,authoredNativePublishers:false,gameId:config.gameId,mode:config.mode,startedAt:new Date().toISOString(),configuration:config,output,samples:[]};
  const previous=config.from?JSON.parse(await readFile(resolve(config.from),'utf8')):null;
  let connection,profile,created=false;
  const save=()=>writeFile(join(output,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');
  try{
    if(process.platform!=='win32')throw Error('Actual Assetto game qualification requires Windows.');
    connection=await connect(config['cdp-url']??process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
    const {native}=connection;evidence.cockpitTarget={url:connection.target.url,title:connection.target.title};
    const catalogue=async()=>{const state=(await native({command:'game_get_state'})).games;return {state,game:state.catalogue.find(g=>g.id===config.gameId)};};
    const wait=async(fn,label)=>{const until=Date.now()+config.timeout;while(Date.now()<until){const value=await fn();if(value)return value;await sleep(250);}throw Error(label);};
    const initial=await catalogue();if(!initial.game?.installed)throw Error('The selected actual game installation was not found.');
    evidence.initialGame=initial.game;
    evidence.executable=await executableEvidence(initial.game);
    const current=()=>native({command:'profile_status'}).then(state=>state.profiles.profileSession);
    evidence.initialProfile=await current();
    const profileId=config['profile-id']??previous?.profile?.id;
    if(config.mode==='snapshot'){
      evidence.finalGame=initial.game;evidence.finalProfile=evidence.initialProfile;evidence.summary={passed:true,readOnly:true};return;
    }
    if(config.mode==='capture'||config.mode==='reapply'){
      if(!evidence.executable.processes?.length)throw Error('The selected driving runtime is not running. Enter a live driving session first.');
      let existingDocument;
      if(config.mode==='reapply'){
        const id=profileId??evidence.initialProfile.owner?.profileId;
        if(!id)throw Error('Reapply needs --from <capture>/evidence.json or --profile-id.');
        existingDocument=(await native({command:'profile_get',id})).profiles.document;
        if(!existingDocument.name.startsWith('Live Assetto verification ')||!existingDocument.eligibility.gameIds.includes(config.gameId))throw Error('Reapply requires this runner’s telemetry profile for the same game.');
      }
      await verifyLiveSetup(native,config.gameId,catalogue,wait,evidence);
      if(config.mode==='capture'){
      const resources=(await native({command:'profile_resources_list',offset:0,limit:128})).profileResources.resources;
      const resource=resources.find(row=>row.attributes.gameId===config.gameId);if(!resource)throw Error('This game has no native profile integration resource.');
      const ports=(await native({command:'profile_resources_inspect',resourceId:resource.id,offset:0,limit:128})).profileResources.ports;
      if(ports.length!==5||ports.some(p=>p.descriptor.direction!=='output'))throw Error('Expected five read-only native telemetry ports.');
      const document=(await native({command:'profile_create',name:`Live Assetto verification ${games[config.gameId].name} ${Date.now()}`})).profiles.document;
      profile={id:document.id,revision:document.revision};created=true;evidence.profile=profile;
      document.eligibility={gameIds:[config.gameId]};
      document.resources.push({id:'telemetry',name:'Actual game telemetry',kind:'game',required:true,ports:ports.map(p=>({...p.descriptor,id:p.descriptor.id.replace('channel:','')}))});
      for(const p of ports){const id=p.descriptor.id.replace('channel:',''),contract=p.descriptor.contract;
        document.processingNodes.push({id:`observed-${id}`,name:`Observed ${id}`,operation:{kind:'identity'},inputs:[{id:'in',name:'Input',direction:'input',contract}],outputs:[{id:'out',name:'Output',direction:'output',contract}]});
        document.bindings.push({id:`route-${id}`,from:{nodeId:'telemetry',portId:id},to:{nodeId:`observed-${id}`,portId:'in'}});
      }
      const assignments=[{resourceId:'telemetry',sourceId:resource.id,ports:ports.map(p=>({profilePortId:p.descriptor.id.replace('channel:',''),observedPortId:p.descriptor.id,expectedContractHash:p.contractHash}))}];
      const saved=(await native({command:'profile_editor_save',document,assignments,expectedRevision:document.revision})).profiles.document;
      profile.revision=saved.revision;evidence.profileDocument=saved;evidence.assignments=assignments;
      }else{
        profile={id:existingDocument.id,revision:existingDocument.revision};
        evidence.profile=profile;evidence.profileDocument=existingDocument;evidence.reapplied=true;
      }
      await native({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,gameId:config.gameId,expectedActiveOwner:null});
    }else{
      const active=await current(),id=profileId??active.owner?.profileId;
      if(!id)throw Error('No capture profile was supplied or is active. Use --from <capture>/evidence.json.');
      const document=(await native({command:'profile_get',id})).profiles.document;
      if(!document.name.startsWith('Live Assetto verification ')||!document.eligibility.gameIds.includes(config.gameId))throw Error('Use a telemetry profile created by this live verification runner for the same game.');
      profile={id:document.id,revision:document.revision};evidence.profile=profile;
      if(config.mode==='cleanup'){
        if(active.owner?.profileId===profile.id)await native({command:'profile_stop'});
        await native({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});
        evidence.summary={passed:true,removedProfile:profile.id};return;
      }
    }
    const row=async()=>{
      const {state,game}=await catalogue(),status=await current();
      if(status.owner?.profileId!==profile.id)throw Error('The capture profile is no longer the applied owner.');
      const inspection=(await native({command:'profile_runtime_inspect',owner:status.owner,offset:0,limit:64})).profileInspection;
      return {at:new Date().toISOString(),profileState:status.state,owner:status.owner,vehicle:game.currentVehicle,processVerified:game.integrationContext?.processVerified,detected:state.detectedGames.some(g=>g.gameId===config.gameId),integrationContext:game.integrationContext,channels:samplesFrom(inspection),inspection};
    };
    await wait(async()=>{const value=await row();return readyForObservation(value,config)&&value;},`The requested ${config.mode} telemetry state was not observed.`);
    const until=Date.now()+config.seconds*1000;
    do{const value=await row();evidence.samples.push(value);console.log(JSON.stringify({gameId:config.gameId,mode:config.mode,at:value.at,vehicle:value.vehicle,channels:value.channels}));await save();await sleep(config.interval);}while(Date.now()<until);
    evidence.finalGame=(await catalogue()).game;evidence.finalProfile=await current();
    evidence.summary=summarize(evidence.samples,config.mode,config);
    evidence.finalExecutable=await executableEvidence(evidence.finalGame);
    if(evidence.executable.sha256!==evidence.finalExecutable.sha256)throw Error('The actual game executable changed during capture.');
    await connection.send('Page.captureScreenshot',{format:'png'}).then(shot=>writeFile(join(output,'cockpit.png'),Buffer.from(shot.data,'base64'))).catch(error=>{evidence.screenshotError=error.message;});
    if(!evidence.summary.passed)throw Error(evidence.summary.problems.join(' '));
  }catch(error){evidence.error=error.stack??String(error);evidence.summary??={passed:false};evidence.summary.passed=false;throw error;}
  finally{
    if(connection&&created&&!config['keep-profile']){
      try{const status=(await connection.native({command:'profile_status'})).profiles.profileSession;if(status.owner?.profileId===profile.id)await connection.native({command:'profile_stop'});await connection.native({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});evidence.profileRemoved=true;}catch(error){evidence.cleanupError=String(error);evidence.summary??={passed:false};evidence.summary.passed=false;evidence.error??='Capture profile cleanup failed: '+error.message;process.exitCode=1;}
    }
    evidence.completedAt=new Date().toISOString();await save();connection?.close();
    console.log(JSON.stringify({output,profile:evidence.profile,summary:evidence.summary,error:evidence.error?.split('\n')[0]}));
  }
}

if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  try{const config=options();if(config)await main(config);}catch(error){console.error(error.message);process.exitCode=1;}
}
