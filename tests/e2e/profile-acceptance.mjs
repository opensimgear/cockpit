// Ordinary native unified profile + production C gateway/peripheral. CI isolated host only.
import test from 'node:test';
import assert from 'node:assert/strict';
import {randomBytes} from 'node:crypto';
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
    await writeFile(resolve(directory,'result.json'),JSON.stringify({scope:'software simulation',hardware:false,qualification:false,unifiedProfile:true,capturedBaseline:true,endpointConflict:true,pause:true,stop:true,noPersistentConfigWrites:true,restartWithoutOwner:true},null,2));
  }finally{await service.stop();await Promise.all(peers.reverse().map(peer=>peer.stop()));}
});
