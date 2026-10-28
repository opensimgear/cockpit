// Exercise the real Cockpit registry and native game watcher, without mocked IPC.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {execFile,spawn} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,writeFile,rename,unlink} from 'node:fs/promises';
import {resolve} from 'node:path';

const enabled=process.platform==='win32'&&process.env.OSG_GAME_SOURCES_NATIVE==='1';
const execute=promisify(execFile),delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
test('integration sources follow configured membership, installation and process state', {skip:!enabled,timeout:180_000},async()=>{
  const require=createRequire(import.meta.url),{chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().includes(':1420'));
  assert.ok(page,'Start the native development Cockpit with CDP.');
  const artifacts=resolve('.research/profile-game-sources-native'),root=resolve(artifacts,'X-Plane 11');
  const executable=resolve(root,'X-Plane.exe'),hidden=executable+'.missing';
  const send=async request=>page.evaluate(async request=>{
    const{gatewayRequest}=await import('/src/lib/gateway.ts');return gatewayRequest(request);
  },request);
  const games=async()=>(await send({command:'game_get_state'})).games.catalogue;
  const inventory=async()=>{
    const rows=[];let offset=0,total=1;
    while(offset<total){const page=(await send({command:'profile_resources_list',offset,limit:128})).profileResources;rows.push(...page.resources);total=page.total;offset+=page.resources.length;}
    return rows;
  };
  const wait=async(read,accept,label)=>{
    for(let attempt=0;attempt<80;attempt++){const value=await read();if(accept(value))return value;await delay(150);}
    assert.fail(`Timed out: ${label}`);
  };
  let child,added=false,fileHidden=false;
  try{
    await page.reload();
    const initialGames=await games(),sources=(await inventory()).filter(item=>item.resourceType==='game-integration');
    for(const source of sources){
      const game=initialGames.find(game=>game.id===source.attributes.gameId);
      assert.ok(game?.provider,`Orphan integration ${source.name}`);
      assert.equal(source.attributes.integrationKey,game.integrationKey);
    }
    assert.ok(!sources.some(source=>/Diablo/.test(source.name)));
    assert.ok(!initialGames.some(game=>game.id==='x-plane-11'),'Use an unused X-Plane 11 fixture slot.');
    await mkdir(resolve(root,'Resources/plugins'),{recursive:true});
    await writeFile(resolve(root,'Resources/plugins/DataRefs.txt'),'2 1110 Wed Jun 24 15:46:43 2026\n\nsim/flightmodel/position/indicated_airspeed float y knots Fixture speed\n');
    await execute('rustc',['scripts/fixtures/custom-game-native.rs','-o',executable],{windowsHide:true});
    await send({command:'game_add',name:'Source lifecycle acceptance',installPath:root,executable});added=true;
    let game=(await games()).find(game=>game.id==='x-plane-11');assert.ok(game?.installed);
    assert.equal(game.integrationKey,'x-plane-11');
    await send({command:'game_integration_remove',gameId:game.id});
    game=(await games()).find(game=>game.id==='x-plane-11');
    await wait(inventory,rows=>rows.some(row=>row.attributes.gameId==='x-plane-11'),'new source appears');
    const nativeStatus=()=>page.evaluate(async()=>{
      const{gatewayRequest}=await import('/src/lib/gateway.ts');const{gameIntegrationSourceChoices}=await import('/src/lib/profile-game-source.ts');
      const state=await gatewayRequest({command:'game_get_state'});return gameIntegrationSourceChoices(state.games.catalogue,[]).find(choice=>choice.game.id==='x-plane-11')?.status;
    });
    assert.equal(await nativeStatus(),'Configuration needed');
    await send({command:'game_integration_settings_save',gameId:game.id,expectedSetupRevision:game.integrationSetup.providerSetupRevision,
      connection:{source:'rref',host:'127.0.0.1',port:49000,listenPort:0,updateHz:10,freshnessMs:1500,version:11}});
    await wait(nativeStatus,status=>status==='Not running','configured stopped source');
    await unlink(executable.replace(/\.exe$/i,'.stop')).catch(()=>{});
    child=spawn(executable,[],{windowsHide:true,stdio:'ignore'});
    await wait(nativeStatus,status=>status==='Running','native process verified');
    await writeFile(executable.replace(/\.exe$/i,'.stop'),'stop');
    await wait(()=>Promise.resolve(child.exitCode),code=>code!==null,'fixture stopped');child=null;
    await wait(nativeStatus,status=>status==='Not running','stopped source');
    await rename(executable,hidden);fileHidden=true;
    await send({command:'game_save',gameId:game.id,name:'Source lifecycle acceptance'});
    await wait(nativeStatus,status=>status==='Missing','disk removal keeps missing source');
    assert.ok((await inventory()).some(source=>source.attributes.gameId===game.id));
    await send({command:'game_integration_remove',gameId:game.id});
    await send({command:'game_remove',gameId:game.id});added=false;
    assert.ok(!(await games()).some(current=>current.id===game.id));
    assert.ok(!(await inventory()).some(row=>row.attributes.gameId===game.id),'Source retirement is acknowledged with the configuration change');
    await delay(2100);
    assert.ok(!(await inventory()).some(row=>row.attributes.gameId===game.id),'Background refresh cannot resurrect the retired source');
    const nav=page.getByRole('navigation',{name:'Main navigation'});
    await nav.getByRole('button',{name:'Profiles',exact:true}).click();
    const editor=page.getByRole('button',{name:'Edit Default',exact:true});
    await editor.waitFor();await editor.click();
    const dropdown=page.getByRole('combobox',{name:'Game context',exact:true});
    await dropdown.waitFor();
    const options=await dropdown.locator('option').allTextContents();
    assert.ok(!options.some(option=>/Diablo|Source lifecycle acceptance|Unavailable|Offline/.test(option)));
    assert.equal(options.length,1+(await games()).filter(game=>game.provider).length);
    assert.ok(options.slice(1).every(option=>/ · (Running|Not running|Configuration needed|Missing)$/.test(option)));
    await page.screenshot({path:resolve(artifacts,'source-statuses.png')});
    await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify({native:true,sourceCount:sources.length,options,checks:['unsupported titles excluded','configuration needed','not running','running native process','missing disk installation retained','Cockpit removal retires source']},null,2));
  }finally{
    if(child){await writeFile(executable.replace(/\.exe$/i,'.stop'),'stop');await delay(600);}
    if(fileHidden)await rename(hidden,executable).catch(()=>{});
    if(added){await send({command:'game_integration_remove',gameId:'x-plane-11'}).catch(()=>{});await send({command:'game_remove',gameId:'x-plane-11'}).catch(()=>{});}
    await browser.close();
  }
});
