// The already-running native dev Cockpit supplies every response. No IPC,
// catalogue, profile storage or process-watcher method is intercepted.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,readFile,writeFile,copyFile,unlink} from 'node:fs/promises';
import {resolve} from 'node:path';
const execute=promisify(execFile), enabled=process.platform==='win32'&&process.env.OSG_CUSTOM_GAMES_NATIVE==='1';
const artifacts=resolve('.research/custom-games-native'), delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));

test('custom games, backgrounds and installed library imports use the actual native cockpit', {skip:!enabled,timeout:300_000},async()=>{
  const require=createRequire(import.meta.url), module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
  const {chromium}=require(module), {expect}=require(module+'/test');
  await mkdir(artifacts,{recursive:true});
  const fixtureA=resolve(artifacts,'game-a/CustomGame.exe'),fixtureB=resolve(artifacts,'game-b/CustomGame.exe');
  await mkdir(resolve(artifacts,'game-a'),{recursive:true});await mkdir(resolve(artifacts,'game-b'),{recursive:true});
  await execute('rustc',['scripts/fixtures/custom-game-native.rs','-o',fixtureA],{windowsHide:true});await copyFile(fixtureA,fixtureB);
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:142[01]|tauri\.localhost/.test(page.url())&&!/profile-activation-window/.test(page.url()));
  assert.ok(page,'Start the native dev Cockpit with CDP before this test.');page.setDefaultTimeout(20_000);
  const errors=[],evidence={actualNativeCockpit:true,hardwareValidation:false,checks:[],commands:[]};page.on('pageerror',error=>errors.push(error.message));
  const invoke=async request=>{evidence.commands.push(request.command);return page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'custom-games-native-acceptance'}}),request);};
  const native=async request=>{const result=await invoke(request);assert.equal(result.ok,true,`${request.command}: ${result.error}`);return result.state;};
  const games=async()=>(await native({command:'game_get_state'})).games;
  const status=async()=>(await native({command:'profile_status'})).profiles.profileSession;
  const nav=page.getByRole('navigation',{name:'Main navigation'});
  const uiGames=async()=>{await nav.getByRole('button',{name:'Games',exact:true}).click();const all=page.getByRole('button',{name:'All games',exact:true});if(await all.isVisible().catch(()=>false))await all.click();};
  const uiDetails=async name=>{await uiGames();await page.getByRole('button',{name:`Open ${name} details`,exact:true}).click();};
  let gameId,profileId,fixture;
  const stopFixture=async executable=>{await writeFile(executable.replace(/\.exe$/i,'.stop'),'stop');};
  const waitFixtureExit=child=>child.exitCode!==null?Promise.resolve():new Promise(resolve=>child.once('exit',resolve));
  const startFixture=async executable=>{await unlink(executable.replace(/\.exe$/i,'.stop')).catch(()=>{});const child=spawn(executable,[],{windowsHide:true,stdio:'ignore'});await expect.poll(async()=>{try{return (await readFile(executable.replace(/\.exe$/i,'.ready'),'utf8'))===String(child.pid);}catch{return false;}},{timeout:15_000}).toBe(true);return child;};
  try{
    await page.reload();assert.equal(await page.evaluate(()=>!!window.__TAURI_INTERNALS__?.invoke),true);
    await native({command:'profile_stop'});await native({command:'profile_selection_mode',mode:'manual'});
    for(const old of (await games()).catalogue.filter(game=>game.id.startsWith('custom-')&&/custom-games-native[\\/]game-a[\\/]CustomGame\.exe$/i.test(game.executable??'')))await native({command:'game_custom_remove',gameId:old.id});
    const name=`Custom acceptance ${Date.now()}`;
    const initialCount=(await games()).catalogue.length;
    await uiGames();await page.getByRole('button',{name:'Add game',exact:true}).click();
    await page.getByRole('dialog',{name:'Add custom game',exact:true}).getByRole('button',{name:'Cancel',exact:true}).click();
    assert.equal((await games()).catalogue.length,initialCount);
    await uiGames();await page.getByRole('button',{name:'Add game',exact:true}).click();
    const dialog=page.getByRole('dialog',{name:'Add custom game',exact:true});await expect(dialog).toBeVisible();
    await dialog.getByLabel('Custom game executable',{exact:true}).fill(fixtureA);await expect(dialog.getByLabel('Custom game name',{exact:true})).toHaveValue('CustomGame');await dialog.getByLabel('Custom game name',{exact:true}).fill(name);await expect(dialog.getByLabel('Custom game name',{exact:true})).toHaveValue(name);await dialog.getByRole('button',{name:'Add game',exact:true}).click();
    await expect(dialog).toHaveCount(0);await expect(page.getByRole('heading',{name,exact:true})).toBeVisible();
    const game=(await games()).catalogue.find(game=>game.name===name);assert.ok(game);gameId=game.id;
    assert.match(gameId,/^custom-/);assert.equal(game.provider,null);assert.equal(game.integrationSetup,undefined);
    await expect(page.getByRole('region',{name:'Game integration setup',exact:true})).toHaveCount(0);
    evidence.checks.push('Add executable/name in native UI; no integration setup or fake provider.');
    const source=resolve(artifacts,'background-source.jpg');await copyFile('tests/fixtures/artwork/landscape.jpg',source);
    for(const target of [{id:gameId,name},{id:'dcs',name:'DCS World'}]){
      if(target.id!==gameId){await uiGames();const show=page.getByRole('switch',{name:'Show all supported',exact:true});if(await show.isVisible())await show.check();await page.getByRole('button',{name:/Open DCS World details/}).click();}
      const tileBackground=page.getByRole('region',{name:'Tile background',exact:true});await tileBackground.getByRole('button',{name:'Update image',exact:true}).click();const backgrounds=page.getByRole('dialog',{name:'Update tile background',exact:true});await backgrounds.getByLabel('Tile background image path',{exact:true}).fill(source);await backgrounds.getByRole('button',{name:'Use image',exact:true}).click();
      const cropDialog=page.getByRole('region',{name:'Position tile background',exact:true});await expect(cropDialog).toBeVisible();await cropDialog.getByRole('button',{name:'Save background',exact:true}).click();await expect(cropDialog).toHaveCount(0);
      await expect.poll(async()=>!!(await games()).catalogue.find(game=>game.id===target.id)?.background).toBe(true);
      await tileBackground.getByRole('button',{name:'Update image',exact:true}).click();await expect(cropDialog.getByRole('button',{name:'Resize tile crop bottom right',exact:true})).toBeVisible();await cropDialog.getByRole('button',{name:'Resize tile crop bottom right',exact:true}).press('ArrowLeft');await cropDialog.getByRole('button',{name:'Save background',exact:true}).click();await expect(cropDialog).toHaveCount(0);
      assert.ok((await games()).catalogue.find(game=>game.id===target.id)?.background?.crop.width<1);
    }
    await unlink(source);await page.reload();assert.ok((await games()).catalogue.find(game=>game.id===gameId).background.image.startsWith('data:image/jpeg;base64,'));
    evidence.checks.push('Built-in/custom backgrounds, crop persistence, source removal and reload.');
    const originalBackground=(await games()).catalogue.find(game=>game.id===gameId).background;
    const invalidImage=resolve(artifacts,'invalid-image.jpg');await writeFile(invalidImage,'not an image');
    const invalid=await invoke({command:'game_background_save',gameId,path:invalidImage});assert.equal(invalid.ok,false);
    assert.deepEqual((await games()).catalogue.find(game=>game.id===gameId).background,originalBackground);
    await uiDetails(name);await page.getByRole('region',{name:'Tile background',exact:true}).getByRole('button',{name:'Update image',exact:true}).click();await page.getByRole('dialog',{name:'Update tile background',exact:true}).getByRole('button',{name:'Reset to default',exact:true}).click();
    await expect.poll(async()=>(await games()).catalogue.find(game=>game.id===gameId)?.background??null).toBeNull();
    assert.ok((await games()).catalogue.find(game=>game.id==='dcs').background,'Reset preserves another game sharing the same image.');
    await uiDetails('DCS World');await page.getByRole('region',{name:'Tile background',exact:true}).getByRole('button',{name:'Update image',exact:true}).click();await page.getByRole('dialog',{name:'Update tile background',exact:true}).getByRole('button',{name:'Reset to default',exact:true}).click();
    await expect.poll(async()=>(await games()).catalogue.find(game=>game.id==='dcs')?.background??null).toBeNull();
    evidence.checks.push('Invalid image keeps the previous background; reset works for built-in/custom games and preserves shared assets.');
    await nav.getByRole('button',{name:'Profiles',exact:true}).click();await page.getByRole('button',{name:'Add',exact:true}).click();
    await page.getByRole('button',{name:'Edit profile name',exact:true}).click();await page.getByLabel('Profile name',{exact:true}).fill(name);await page.getByLabel('Profile name',{exact:true}).press('Enter');
    await page.getByLabel('Game context',{exact:true}).selectOption(gameId);await page.getByRole('button',{name:'Save',exact:true}).click();
    await expect(page.getByRole('button',{name:'Save',exact:true})).toBeDisabled();
    const saved=(await native({command:'profile_list'})).profiles.profiles.find(profile=>profile.name===name);assert.ok(saved);profileId=saved.id;
    const document=(await native({command:'profile_get',id:profileId})).profiles.document;
    assert.deepEqual(document.eligibility.gameIds,[gameId]);assert.equal(document.resources.filter(resource=>resource.kind==='game').length,0);
    for(const [newName,path] of [[`${name} renamed`,fixtureB],[name,fixtureA]]){
      await uiDetails((await games()).catalogue.find(game=>game.id===gameId).name);
      await page.getByLabel('Game name',{exact:true}).fill(newName);await page.getByLabel('Game name',{exact:true}).press('Enter');await expect.poll(async()=>(await games()).catalogue.find(game=>game.id===gameId)?.name).toBe(newName);
      await page.getByLabel('Game executable',{exact:true}).fill(path);await page.getByLabel('Game executable',{exact:true}).press('Enter');await expect.poll(async()=>(await games()).catalogue.find(game=>game.id===gameId)?.executable?.replace(/^\\\\\?\\/,'')).toBe(path);
      await expect(page.getByRole('heading',{name:newName,exact:true})).toBeVisible();
      assert.equal((await games()).catalogue.find(game=>game.name===newName)?.id,gameId);
      assert.deepEqual((await native({command:'profile_get',id:profileId})).profiles.document.eligibility.gameIds,[gameId]);
    }
    const missingPath=await invoke({command:'game_custom_save',gameId,name,executable:`${fixtureA}.missing.exe`});assert.equal(missingPath.ok,false);
    assert.ok((await games()).catalogue.find(game=>game.id===gameId).executable.endsWith('CustomGame.exe'));
    evidence.checks.push('Add/Cancel is passive; rename and relocation retain identity/profile references; missing executable edits preserve valid state.');
    await native({command:'profile_apply',id:profileId,expectedRevision:saved.revision,gameId});
    await expect.poll(async()=>(await status()).owner?.profileId,{timeout:30_000}).toBe(profileId);
    const blocked=await invoke({command:'game_custom_remove',gameId});assert.equal(blocked.ok,false);assert.match(blocked.error,/Deactivate/);
    await native({command:'profile_stop'});evidence.checks.push('Custom profile context saves without integration resource, manually activates, and active registration removal is guarded.');
    const unrelated=await startFixture(fixtureB);await delay(1300);assert.ok(!(await games()).detectedGames.some(game=>game.gameId===gameId));await stopFixture(fixtureB);await waitFixtureExit(unrelated);
    fixture=await startFixture(fixtureA);await expect.poll(async()=>(await games()).detectedGames.some(game=>game.gameId===gameId),{timeout:20_000}).toBe(true);
    await native({command:'profile_game_rule_save',gameId,profileId,priority:10,enabled:true});await native({command:'profile_selection_mode',mode:'automatic'});
    await expect.poll(async()=>(await status()).owner?.profileId,{timeout:30_000}).toBe(profileId);
    assert.equal((await status()).owner.gameId,gameId);await stopFixture(fixtureA);await waitFixtureExit(fixture);fixture=null;
    await expect.poll(async()=>(await games()).detectedGames.some(game=>game.gameId===gameId),{timeout:20_000}).toBe(false);
    await native({command:'profile_stop'});await native({command:'profile_selection_mode',mode:'manual'});
    await unlink(fixtureA.replace(/\.exe$/i,'.stop')).catch(()=>{});
    await native({command:'profile_apply',id:profileId,expectedRevision:saved.revision,gameId});
    await native({command:'profile_game_launch',gameId,profileId,rememberChoice:true});
    await expect.poll(async()=>(await games()).detectedGames.some(game=>game.gameId===gameId),{timeout:20_000}).toBe(true);
    await stopFixture(fixtureA);await expect.poll(async()=>(await games()).detectedGames.some(game=>game.gameId===gameId),{timeout:20_000}).toBe(false);
    await native({command:'profile_stop'});evidence.checks.push('Exact-path native process detection, wrong-path rejection, external Automatic, exit and Cockpit launch.');
    await uiGames();await page.getByRole('button',{name:'Import from library',exact:true}).click();
    const library=page.getByRole('dialog',{name:'Import from game library',exact:true});await expect(library).toBeVisible();await expect(library.getByRole('status')).toHaveCount(0,{timeout:60_000});
    const discovered=await games();evidence.libraryCandidates=discovered.libraryCandidates?.map(({id,client,name,kind,executableCandidates})=>({id,client,name,kind,candidates:executableCandidates.length}));
    const candidates=discovered.libraryCandidates.filter(game=>game.client==='steam'&&!game.gameId&&game.kind!=='tool').slice(0,2);assert.equal(candidates.length,2,'Need two actual installed Steam titles for import acceptance.');
    for(const candidate of candidates){await library.getByRole('checkbox',{name:`Import ${candidate.name}`,exact:true}).check();const path=candidate.executableCandidates.find(path=>/shipping|win64|game\.exe/i.test(path))??candidate.executableCandidates[0];if(path)await library.getByLabel(`Executable for ${candidate.name}`,{exact:true}).fill(path);}
    await library.getByRole('button',{name:'Add selected games (2)',exact:true}).click();await expect(library).toHaveCount(0,{timeout:60_000});
    let imported=(await games()).catalogue.filter(game=>candidates.some(candidate=>game.library?.appId===candidate.appId||game.librarySource?.appId===candidate.appId));
    if(imported.length!==2){const all=(await games()).catalogue;imported=all.filter(game=>candidates.some(candidate=>candidate.name===game.name));}
    assert.equal(imported.length,2);evidence.imported=imported.map(({id,name,executable,source,background,defaultBackground})=>({id,name,executable,source,artwork:!!(background||defaultBackground)}));
    for(const candidate of candidates){const before=(await games()).catalogue.length;await native({command:'game_library_import',candidateId:candidate.id,executable:candidate.executableCandidates[0]??null});assert.equal((await games()).catalogue.length,before);}
    evidence.checks.push('Actual installed Steam preview and two selected imports; reimport does not duplicate registrations.');
    await uiGames();await page.screenshot({path:resolve(artifacts,'games-library.png'),fullPage:true});
    await native({command:'game_custom_remove',gameId});assert.deepEqual((await native({command:'profile_get',id:profileId})).profiles.document.eligibility.gameIds,[gameId]);
    const missing=await invoke({command:'profile_apply',id:profileId,expectedRevision:saved.revision,gameId});assert.equal(missing.ok,false);
    evidence.checks.push('Removal preserves saved profile references while fresh activation rejects the missing registration.');
    assert.deepEqual(errors,[]);
  }catch(error){evidence.failure=String(error);await page.screenshot({path:resolve(artifacts,'failure.png'),fullPage:true}).catch(()=>{});throw error;}
  finally{
    await stopFixture(fixtureA).catch(()=>{});await stopFixture(fixtureB).catch(()=>{});if(fixture)await new Promise(resolve=>{fixture.once('exit',resolve);setTimeout(resolve,2000);});
    await native({command:'profile_stop'}).catch(()=>{});await native({command:'profile_selection_mode',mode:'manual'}).catch(()=>{});
    evidence.pageErrors=errors;await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');await browser.close();
  }
});
