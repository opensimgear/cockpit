// Fresh installation metadata and unified launch boundary; no obsolete game profile is activated.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile,mkdir,mkdtemp,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {processClient} from './process-client.mjs';
test('Games persists only fresh installation metadata and rejects retired profile activation',{timeout:30000},async()=>{
  const launch=JSON.parse(await readFile(process.env.OSG_E2E_MANIFEST,'utf8'));await mkdir(launch.artifacts,{recursive:true});const directory=await mkdtemp(resolve(launch.artifacts,'game-metadata-'));
  const old=resolve(directory,'games.json');await writeFile(old,'obsolete game profiles must remain ignored');
  const executable=resolve(directory,'DCS.exe');await writeFile(executable,'Installation metadata fixture; this is never executed.');
  const env={...process.env,OSG_LOCAL_GATEWAYS_ONLY:'1',OSG_GAMES_CONFIG:old,OSG_DCS_BIOS_CONFIG:resolve(directory,'old-anchor.json')};
  let service=processClient(launch.serviceBinary,[],env);
  const request=(command,extra={})=>service.request({command,clientId:'unified-game-metadata-acceptance',...extra});
  const ok=async(command,extra={})=>{const reply=await request(command,extra);assert.equal(reply.ok,true,reply.error);return reply.state;};
  try{
    const initial=(await ok('game_get_state')).games;assert.equal(initial.cleanup.phase,'idle');
    for(const retired of ['session','profiles','defaultProfiles'])assert.equal(Object.hasOwn(initial,retired),false,`Retired Games field ${retired} must stay absent`);
    assert.equal((await ok('profile_status')).profiles.profileSession.owner,null);
    for(const command of ['game_profile_save','game_profile_delete','game_profile_default','game_activate','game_launch','game_capture','dcs_start','integration_start'])assert.equal((await request(command,{gameId:'dcs',gameProfile:{resume:true}})).ok,false,command);
    await ok('game_installation',{gameId:'dcs',executable});
    const installed=(await ok('game_get_state')).games.catalogue.find(game=>game.id==='dcs');assert.equal(installed.executable,executable);assert.equal(installed.installed,true);
    assert.equal(installed.integrationContext.processVerified,false);
    assert.equal((await request('profile_game_launch',{gameId:'dcs',owner:{session:'browser'}})).ok,false,'Browser owner/process proof cannot launch');
    await service.stop();service=processClient(launch.serviceBinary,[],env);
    assert.equal((await ok('game_get_state')).games.catalogue.find(game=>game.id==='dcs').executable,executable);
    assert.equal((await ok('profile_status')).profiles.profileSession.owner,null);
    assert.equal(await readFile(old,'utf8'),'obsolete game profiles must remain ignored');
  }finally{await service.stop();}
});
