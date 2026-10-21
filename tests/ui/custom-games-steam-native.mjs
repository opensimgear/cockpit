// Explicit real installed-game launch acceptance. No hardware/input injection.
// Existing matching processes are rejected; cleanup addresses only newly
// observed exact executable/PID/creation identities from this run.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {randomUUID} from 'node:crypto';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
const execute=promisify(execFile),enabled=process.platform==='win32'&&process.env.OSG_STEAM_GAMES_NATIVE==='1';
async function processes(executable,owned=[]){
  const command=`$ErrorActionPreference='Stop'; $taskTarget=$env:OSG_ACCEPT_GAME_EXE.Replace('\\?\\','').ToLowerInvariant(); $taskOwned=ConvertFrom-Json $env:OSG_ACCEPT_GAME_OWNERS; $taskItems=@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -and $_.ExecutablePath.Replace('\\?\\','').ToLowerInvariant() -eq $taskTarget}); foreach($taskItem in $taskItems){$taskCreated=$taskItem.CreationDate.ToUniversalTime().ToString('o'); if($taskOwned | Where-Object {$_.pid -eq $taskItem.ProcessId -and $_.created -eq $taskCreated}){Stop-Process -Id $taskItem.ProcessId -Force -ErrorAction Stop}}; ConvertTo-Json -InputObject @($taskItems | ForEach-Object {@{pid=$_.ProcessId;created=$_.CreationDate.ToUniversalTime().ToString('o');executable=$_.ExecutablePath}}) -Compress`;
  const result=await execute('powershell.exe',['-NoProfile','-NonInteractive','-Command',command],{windowsHide:true,env:{...process.env,OSG_ACCEPT_GAME_EXE:executable.replace(/^\\\\\?\\/,''),OSG_ACCEPT_GAME_OWNERS:JSON.stringify(owned)},maxBuffer:1024*1024});
  return JSON.parse(result.stdout.trim()||'[]');
}
test('two installed Steam custom games launch through Steam and activate their native profile contexts',{skip:!enabled,timeout:420_000},async()=>{
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';const {chromium}=require(module),{expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().includes(':1421'));assert.ok(page);
  const directory=resolve('.research/custom-games-native');await mkdir(directory,{recursive:true});
  const evidencePath=resolve(directory,process.env.OSG_STEAM_ACCEPT_EVIDENCE_NAME??'steam-evidence.json');
  const evidence={actualNativeCockpit:true,actualSteamLaunches:false,hardwareValidation:false,gameplayValidation:false,games:[]};
  const native=async request=>{const result=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'custom-steam-native-acceptance'}}),request);assert.equal(result.ok,true,`${request.command}: ${result.error}`);return result.state;};
  const games=async()=>(await native({command:'game_get_state'})).games;
  const status=async()=>(await native({command:'profile_status'})).profiles.profileSession;
  try{
    await native({command:'profile_stop'});await native({command:'profile_selection_mode',mode:'manual'});await native({command:'game_library_scan'});
    const candidates=(await games()).libraryCandidates;
    for(const name of (process.env.OSG_STEAM_ACCEPT_NAMES??'Liftoff|Darkhaven Demo').split('|')){
      const candidate=candidates.find(game=>game.client==='steam'&&game.name===name);assert.ok(candidate,`Install ${name} before this explicit game acceptance.`);
      const executable=candidate.executableCandidates.find(path=>/shipping/i.test(path))??candidate.executableCandidates.find(path=>!/[\\/](?:launcher|server)[^\\/]*\.exe$/i.test(path))??candidate.executableCandidates[0];assert.ok(executable,`Resolve ${name}'s runtime executable.`);
      assert.deepEqual(await processes(executable),[],`${name} is already running; no pre-existing game will be stopped.`);
      const imported=(await native({command:'game_library_import',candidateId:candidate.id,executable})).games;
      const gameId=imported.addedGameId,game=imported.catalogue.find(game=>game.id===gameId);assert.ok(game);assert.match(gameId,/^custom-/);assert.equal(game.steamAppId,Number(candidate.appId));
      const profiles=(await native({command:'profile_list'})).profiles.profiles;const base=(await native({command:'profile_get',id:profiles.find(profile=>profile.name==='Default').id})).profiles.document;
      const document={...base,id:randomUUID(),revision:1,name:`Steam acceptance ${name}`,eligibility:{gameIds:[gameId]},resources:[],processingNodes:[],bindings:[],virtualOutputs:[],desktopOutputs:[],vehicleTabs:[],editor:{...base.editor,positions:[],groups:[],collapsedPortGroups:[]}};
      await native({command:'profile_editor_save',document,assignments:[],expectedRevision:null});
      await native({command:'profile_selection_mode',mode:'automatic'});
      let owned=[],record={name,gameId,executable,steamAppId:game.steamAppId,profileId:document.id,attemptedAt:new Date().toISOString()};
      evidence.games.push(record);
      try{
        const launched=await native({command:'profile_game_launch',gameId,profileId:document.id,rememberChoice:true});record.launch=launched.profiles?.launch??launched.profiles?.profileSession;
        await expect.poll(async()=>(await games()).detectedGames.some(game=>game.gameId===gameId),{timeout:60_000}).toBe(true);
        await expect.poll(async()=>(await status()).owner?.profileId,{timeout:60_000}).toBe(document.id);
        // Hardware-first launch preserves its journal owner's immutable identity;
        // the admitted selected context is reported by the launch coordinator.
        assert.equal(launched.profiles.launch.selectedGameId,gameId);assert.equal(launched.profiles.launch.gameId,gameId);
        owned=await processes(executable);assert.ok(owned.length);record.processes=owned;record.owner=(await status()).owner;
        record.selectedGameId=launched.profiles.launch.selectedGameId;record.profileContext=(await games()).catalogue.find(game=>game.id===gameId).profileContext;
        assert.equal(record.profileContext.status,'verified');
        const contextGuard=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'custom-steam-native-acceptance'}}),{command:'game_custom_remove',gameId});
        assert.equal(contextGuard.ok,false);assert.match(contextGuard.error,/Deactivate the selected game profile/);record.appliedContextGuard=contextGuard.error;
        await page.screenshot({path:resolve(directory,`steam-${candidate.appId}.png`),fullPage:true});
      }catch(error){record.failure=String(error);throw error;}
      finally{
        const steamLog=await readFile(resolve(process.env['ProgramFiles(x86)']??'C:/Program Files (x86)','Steam/logs/console_log.txt'),'utf8').catch(()=> '');
        record.steamDiagnostics=steamLog.split('\n').filter(line=>line.includes(`AppID ${game.steamAppId},`)||line.includes(`-applaunch ${game.steamAppId}`)).slice(-20);
        await native({command:'profile_stop'});await native({command:'profile_selection_mode',mode:'manual'});
        if(!owned.length)owned=await processes(executable);
        record.cleanupProcesses=owned;
        await writeFile(evidencePath,JSON.stringify(evidence,null,2)+'\n');
        await processes(executable,owned);
        await expect.poll(async()=>(await games()).detectedGames.some(game=>game.gameId===gameId),{timeout:30_000}).toBe(false);
        record.exited=true;
      }
    }
  }catch(error){evidence.failure=String(error);throw error;}
  finally{evidence.actualSteamLaunches=evidence.games.length>=2&&evidence.games.every(game=>game.owner&&game.exited&&!game.failure);await writeFile(evidencePath,JSON.stringify(evidence,null,2)+'\n');await browser.close();}
});
