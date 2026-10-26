// Run prepare, restart the isolated native app, then run verify. Both phases
// use actual IPC/UI; the second requires a different native process identity.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile,writeFile} from 'node:fs/promises';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {resolve} from 'node:path';
const phase=process.env.OSG_CUSTOM_GAMES_RESTART_PHASE;
const enabled=process.platform==='win32'&&['prepare','verify'].includes(phase);
test('custom registration, artwork and profile context survive a native app restart',{skip:!enabled,timeout:90_000},async()=>{
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
  const {chromium}=require(module),{expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().includes(':1421'));assert.ok(page);
  const native=async request=>{const result=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'custom-games-restart-acceptance'}}),request);assert.equal(result.ok,true,`${request.command}: ${result.error}`);return result.state;};
  const games=async()=>(await native({command:'game_get_state'})).games;
  const fixture=resolve('.research/custom-games-native/game-a/CustomGame.exe');
  const evidencePath=resolve('.research/custom-games-native/restart-evidence.json');
  const command='$taskApps=@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -eq $env:OSG_ACCEPT_APP_PATH} | ForEach-Object {@{pid=$_.ProcessId;created=$_.CreationDate.ToUniversalTime().ToString("o")}}); ConvertTo-Json -InputObject $taskApps -Compress';
  const output=await promisify(execFile)('powershell.exe',['-NoProfile','-NonInteractive','-Command',command],{windowsHide:true,env:{...process.env,OSG_ACCEPT_APP_PATH:resolve('src-tauri/target/debug/osg-cockpit.exe')}});
  const instances=JSON.parse(output.stdout.trim());assert.equal(instances.length,1);
  try{
    await native({command:'profile_stop'});await native({command:'profile_selection_mode',mode:'manual'});
    if(phase==='prepare'){
      for(const game of (await games()).catalogue.filter(game=>game.name==='Custom restart acceptance'))await native({command:'game_custom_remove',gameId:game.id});
      const added=(await native({command:'game_custom_save',name:'Custom restart acceptance',executable:fixture})).games;
      const gameId=added.addedGameId;
      const background=(await native({command:'game_background_save',gameId,path:resolve('tests/fixtures/artwork/landscape.jpg'),positionX:17,positionY:83})).games.catalogue.find(game=>game.id===gameId).background;
      const document=(await native({command:'profile_create',name:'Custom restart acceptance'})).profiles.document;
      document.eligibility={gameIds:[gameId]};
      await native({command:'profile_editor_save',document,assignments:[],expectedRevision:document.revision});
      await writeFile(evidencePath,JSON.stringify({prepared:true,instance:instances[0],gameId,profileId:document.id,background},null,2)+'\n');
    }else{
      const evidence=JSON.parse(await readFile(evidencePath,'utf8'));
      assert.notDeepEqual(instances[0],evidence.instance,'Restart the isolated native app between prepare and verify.');
      const game=(await games()).catalogue.find(game=>game.id===evidence.gameId);assert.ok(game);assert.equal(game.name,'Custom restart acceptance');assert.deepEqual(game.background,evidence.background);
      assert.deepEqual((await native({command:'profile_get',id:evidence.profileId})).profiles.document.eligibility.gameIds,[game.id]);
      await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Games',exact:true}).click();
      const all=page.getByRole('button',{name:'All games',exact:true});if(await all.isVisible())await all.click();
      await page.getByRole('button',{name:'Open Custom restart acceptance details',exact:true}).click();
      const relocated=resolve('.research/custom-games-native/game-b/CustomGame.exe');
      await page.getByLabel('Game executable',{exact:true}).fill(relocated);
      await page.getByLabel('Game executable',{exact:true}).press('Enter');
      await expect.poll(async()=>(await games()).catalogue.find(item=>item.id===game.id)?.executable?.replaceAll('\\','/').endsWith('/game-b/CustomGame.exe')).toBe(true);
      assert.deepEqual((await native({command:'profile_get',id:evidence.profileId})).profiles.document.eligibility.gameIds,[game.id]);
      assert.deepEqual((await games()).catalogue.find(item=>item.id===game.id).background,evidence.background);
      await page.screenshot({path:resolve('.research/custom-games-native/restart-relocation.png'),fullPage:true});
      await page.getByRole('button',{name:'Remove',exact:true}).click();
      await page.getByRole('dialog',{name:'Remove Custom restart acceptance?',exact:true}).getByRole('button',{name:'Remove game',exact:true}).click();
      await expect.poll(async()=>(await games()).catalogue.some(item=>item.id===game.id)).toBe(false);
      assert.deepEqual((await native({command:'profile_get',id:evidence.profileId})).profiles.document.eligibility.gameIds,[game.id]);
      await writeFile(evidencePath,JSON.stringify({...evidence,verified:true,restartedInstance:instances[0],typedPathRelocation:true,referencedRemoval:true},null,2)+'\n');
    }
  }finally{await browser.close();}
});
