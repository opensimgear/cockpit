// Opt-in acceptance against the running native dev Cockpit; no IPC is mocked.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
const enabled=process.platform==='win32'&&process.env.OSG_CUSTOM_GAMES_NATIVE==='1';

test('native game categories support assignment, configuration, filtering and durable identity',{skip:!enabled,timeout:120_000},async()=>{
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
  const {chromium}=require(module),{expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().includes(process.env.OSG_NATIVE_APP_URL??'127.0.0.1:1421'));
  assert.ok(page,'Start this worktree’s native dev Cockpit first.');page.setDefaultTimeout(15_000);
  const artifacts=resolve('.research/game-categories-native');await mkdir(artifacts,{recursive:true});
  const errors=[],evidence={actualNativeCockpit:true,hardwareValidation:false,checks:[]};
  const pageError=error=>errors.push(error.message);page.on('pageerror',pageError);
  const native=async request=>{const reply=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'game-categories-native-acceptance'}}),request);assert.equal(reply.ok,true,reply.error);return reply.state.games;};
  const read=()=>native({command:'game_get_state'});
  const uiGames=async()=>{await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Games',exact:true}).click();const back=page.getByRole('button',{name:'All games',exact:true});if(await back.isVisible().catch(()=>false))await back.click();await page.getByLabel('Filter games by category',{exact:true}).selectOption('');};
  const manage=async()=>{await page.getByRole('button',{name:'Manage categories',exact:true}).click();return page.getByRole('dialog',{name:'Game categories',exact:true});};
  const inputNamed=async(dialog,name)=>{for(const input of await dialog.getByRole('textbox').all())if(await input.inputValue()===name)return input;throw new Error(`Missing category ${name}`);};
  const original=await read(),initialCategories=original.categories??{},initialDcs=original.catalogue.find(game=>game.id==='dcs').categoryId??null;
  const suffix=Date.now(),flight=`Flight acceptance ${suffix}`,racing=`Racing acceptance ${suffix}`,renamed=`Flying acceptance ${suffix}`,gameName=`Category acceptance ${suffix}`;
  let gameId;
  try{
    await uiGames();let dialog=await manage();await dialog.getByRole('button',{name:'Add category',exact:true}).click();await dialog.getByRole('textbox').last().fill(flight);await dialog.getByRole('button',{name:'Add category',exact:true}).click();await dialog.getByRole('textbox').last().fill(flight.toLowerCase());await expect(dialog.getByRole('button',{name:'Save categories',exact:true})).toBeDisabled();await dialog.getByRole('textbox').last().fill(racing);await dialog.getByRole('button',{name:'Save categories',exact:true}).click();await expect(dialog).toHaveCount(0);
    const categories=(await read()).categories,flightId=Object.keys(categories).find(id=>categories[id]===flight),racingId=Object.keys(categories).find(id=>categories[id]===racing);assert.ok(flightId&&racingId);
    await page.getByRole('button',{name:'Add game',exact:true}).click();dialog=page.getByRole('dialog',{name:'Add custom game',exact:true});await dialog.getByLabel('Custom game name',{exact:true}).fill(gameName);await dialog.getByLabel('Custom game category',{exact:true}).selectOption(flightId);await dialog.getByRole('button',{name:'Add game',exact:true}).click();await expect(dialog).toHaveCount(0);gameId=(await read()).catalogue.find(game=>game.name===gameName).id;
    await expect(page.getByLabel('Game category',{exact:true})).toHaveValue(flightId);const badge=await page.locator('.detail-launch .custom-badge').boundingBox(),title=await page.getByRole('heading',{name:gameName,exact:true}).boundingBox();assert.ok(badge.x>title.x+title.width,'Custom badge belongs on the right');
    await page.getByLabel('Game category',{exact:true}).selectOption(racingId);await expect.poll(async()=>(await read()).catalogue.find(game=>game.id===gameId).categoryId).toBe(racingId);
    await uiGames();await page.getByLabel('Filter games by category',{exact:true}).selectOption(racingId);await expect(page.locator('.game-tile')).toHaveCount(1);await expect(page.getByRole('button',{name:`Open ${gameName} details, installation not found`,exact:true})).toBeVisible();const tile=page.locator('.game-tile');const tileBadge=await tile.locator('.custom-art-badge').boundingBox(),art=await tile.locator('.game-art').boundingBox();assert.ok(tileBadge.x>art.x+art.width/2);
    await page.screenshot({path:resolve(artifacts,'filtered-games.png')});evidence.checks.push('Create categories; duplicate names rejected; assign on Add and details; right-hand Custom badges; list filter.');
    await uiGames();await page.getByRole('switch',{name:'Show all supported',exact:true}).check();await page.getByRole('button',{name:/^Open DCS World details/}).click();await page.getByLabel('Game category',{exact:true}).selectOption(flightId);await expect.poll(async()=>(await read()).catalogue.find(game=>game.id==='dcs').categoryId).toBe(flightId);
    dialog=await manage();await (await inputNamed(dialog,flight)).fill(renamed);await dialog.getByRole('button',{name:'Save categories',exact:true}).click();await expect(dialog).toHaveCount(0);await page.reload();await expect(page.getByLabel('Game category',{exact:true})).toHaveValue(flightId);await expect(page.getByLabel('Game category',{exact:true}).getByRole('option',{name:renamed,exact:true})).toHaveCount(1);assert.equal((await read()).catalogue.find(game=>game.id===gameId).categoryId,racingId);
    await uiGames();await page.getByLabel('Filter games by category',{exact:true}).selectOption(racingId);dialog=await manage();const racingInput=await inputNamed(dialog,racing);await racingInput.locator('..').locator('..').getByRole('button',{name:/Delete category/}).click();await dialog.getByRole('button',{name:'Save categories',exact:true}).click();await expect(dialog).toHaveCount(0);await expect(page.getByLabel('Filter games by category',{exact:true})).toHaveValue('');assert.equal((await read()).catalogue.find(game=>game.id===gameId).categoryId,null);
    await page.getByLabel('Filter games by category',{exact:true}).selectOption('uncategorized');await expect(page.getByRole('button',{name:`Open ${gameName} details, installation not found`,exact:true})).toBeVisible();await expect(page.getByRole('button',{name:/^Open DCS World details/})).toHaveCount(0);
    dialog=await manage();await (await inputNamed(dialog,renamed)).fill('Cancelled rename');await dialog.getByRole('button',{name:'Cancel',exact:true}).click();assert.equal((await read()).categories[flightId],renamed);
    evidence.checks.push('Built-in assignment, category rename preserves identity, reload durability, category deletion clears assignments/filter, Uncategorized filter, Cancel.');
    assert.deepEqual(errors,[]);
  }catch(error){evidence.failure=String(error);await page.screenshot({path:resolve(artifacts,'failure.png')}).catch(()=>{});throw error;}
  finally{
    if(gameId)await native({command:'game_custom_remove',gameId}).catch(()=>{});
    await native({command:'game_category_save',gameId:'dcs',categoryId:initialDcs}).catch(()=>{});
    const current=await read();await native({command:'game_categories_save',categories:initialCategories,expectedCategories:current.categories??{}}).catch(()=>{});
    page.off('pageerror',pageError);evidence.pageErrors=errors;await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');await browser.close();
  }
});
