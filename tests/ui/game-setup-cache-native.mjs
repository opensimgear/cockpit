// Opt-in regression against the running native Cockpit. Setup writes only to
// the private fixture folder; no simulator or profile is started.
import test from 'node:test';
import assert from 'node:assert/strict';
import {cp,mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

const normalize=value=>value.replace(/^\\\\\?\\/,'').replaceAll('\\','/').toLowerCase();

test('native folder checks preserve the configured installation and separate alternative results',{
  skip:process.env.OSG_GAME_SETUP_CACHE_NATIVE!=='1',timeout:120000,
},async()=>{
  const directory=resolve('.research/reactive/game-setup-cache-native',String(Date.now()));
  const ready=resolve(directory,'ready'),broken=resolve(directory,'broken');
  await mkdir(resolve(ready,'Scripts'),{recursive:true});await mkdir(broken,{recursive:true});
  const page=await nativePage(),cleanupErrors=[];let before;
  const request=async value=>{
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'game-setup-cache-native'})}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;
  };
  const game=async()=>(await request({command:'game_get_state'})).games.catalogue.find(game=>game.id==='dcs');
  const save=async(userFolder,connection,aircraftModule)=>request({command:'game_integration_settings_save',
    gameId:'dcs',expectedSetupRevision:(await game()).integrationSetup.providerSetupRevision,
    userFolder,connection,aircraftModule:aircraftModule??null});
  try{
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    const initial=await game();before=initial.integrationSetup;
    assert.ok(initial.selectedInstallationId);
    assert.ok(before.userFolder&&before.connection?.cataloguePath,'An existing DCS-BIOS catalogue is the read-only fixture source.');
    await cp(resolve(before.connection.cataloguePath,'../..'),resolve(ready,'Scripts/DCS-BIOS'),{recursive:true,errorOnExist:true,force:false});
    await writeFile(resolve(ready,'Scripts/Export.lua'),"-- private setup cache fixture\npackage.path = package.path .. ';' .. lfs.writedir() .. 'Scripts/?.lua'\ndofile(lfs.writedir() .. [[Scripts\\DCS-BIOS\\BIOS.lua]])\n");
    await request({command:'game_integration_setup',gameId:'dcs',userFolder:ready});
    const prepared=await until(async()=>{
      const value=await game();return value.integrationSetup.state!=='setting-up'&&value;
    },Boolean,30000);
    assert.equal(prepared.integrationSetup.state,'ready',JSON.stringify(prepared.integrationSetup));
    const connection={...before.connection,cataloguePath:resolve(broken,'Scripts/DCS-BIOS/doc/json')};
    await save(broken,connection,before.aircraftModule);
    await request({command:'game_integration_check',gameId:'dcs',userFolder:ready});
    const checked=(await game()).integrationSetup;
    assert.notEqual(checked.state,'ready');assert.equal(normalize(checked.userFolder),normalize(broken));
    assert.equal(normalize(checked.connection.cataloguePath),normalize(connection.cataloguePath));
    assert.equal(checked.checkPreview?.state,'ready');
    assert.equal(normalize(checked.checkPreview.userFolder),normalize(ready));
    assert.equal(normalize(checked.checkPreview.connection.cataloguePath),normalize(resolve(ready,'Scripts/DCS-BIOS/doc/json')));
    await new Promise(resolve=>setTimeout(resolve,2200));
    const refreshed=(await game()).integrationSetup;
    assert.notEqual(refreshed.state,'ready');assert.deepEqual(refreshed.checkPreview,checked.checkPreview);
    await request({command:'game_integration_check',gameId:'dcs',userFolder:broken});
    const cleared=(await game()).integrationSetup;
    assert.equal(cleared.checkPreview,undefined);assert.notEqual(cleared.state,'ready');
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify({at:new Date().toISOString(),
      actualNativeCockpit:true,noSimulatorLaunch:true,noPhysicalOutputs:true,physicalLatencyQualified:false,
      readyFolderSetup:prepared.integrationSetup,checked,refreshed,cleared},null,2)+'\n');
  }finally{
    try{
      if(before?.connection)try{
        await save(before.userFolder,before.connection,before.aircraftModule);
        await request({command:'game_integration_check',gameId:'dcs'});
      }catch(error){cleanupErrors.push(String(error));}
      await writeFile(resolve(directory,'cleanup.json'),JSON.stringify({cleanupErrors},null,2)+'\n');
    }finally{page.close();}
    assert.deepEqual(cleanupErrors,[],'Restore the original provider settings.');
  }
});
