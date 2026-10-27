// Exercises discovery against real installed libraries and Steam Store metadata.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';

test('platform genres become editable tags and user changes survive rescans',{
  skip:process.env.OSG_GAME_PLATFORM_TAGS_NATIVE!=='1',timeout:180_000,
},async()=>{
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright';
  const {chromium}=require(module),{expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:142[01]/.test(page.url())&&!page.url().includes('profile-activation-window'));
  assert.ok(page);page.setDefaultTimeout(20_000);
  const artifacts=resolve('.research/game-platform-tags-native');await mkdir(artifacts,{recursive:true});
  const evidence={actualNativeCockpit:true,gameLaunches:false,checks:[]};let game,automatic;
  const native=async request=>{
    const reply=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'platform-tags-acceptance'}}),request);
    assert.equal(reply.ok,true,request.command+': '+reply.error);return reply.state.games;
  };
  const state=()=>native({command:'game_get_state'});
  const rescan=async()=>{
    await native({command:'game_discovery_start'});
    await expect.poll(async()=>(await state()).discovery.status,{timeout:60_000}).toBe('ready');return state();
  };
  const openGame=async()=>{
    await page.reload();await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Games',exact:true}).click();
    const back=page.getByRole('button',{name:'All games',exact:true});if(await back.isVisible())await back.click();
    await page.locator(`[data-game-id="${game.id}"] .tile-details`).click();
  };
  try{
    await page.reload();const discovered=await rescan();
    const entry=discovered.discovery.entries.find(entry=>entry.tagIds?.length&&discovered.catalogue.some(game=>game.id===entry.gameId));
    assert.ok(entry,'Need an untagged installed Steam game with available Store genres.');
    game=discovered.catalogue.find(game=>game.id===entry.gameId);automatic=entry.tagIds;
    assert.match(entry.message,/Platform tags:/);
    await native({command:'game_discovery_apply',scanId:discovered.discovery.scanId,actions:discovered.discovery.entries.map(item=>({id:item.id,selected:item.id===entry.id}))});
    assert.deepEqual((await state()).catalogue.find(item=>item.id===game.id).tagIds,automatic);
    await openGame();const tags=(await state()).tags;
    for(const id of automatic)await expect(page.getByRole('button',{name:'Remove '+tags[id],exact:true})).toBeVisible();
    await page.screenshot({path:resolve(artifacts,'automatic-tags.png')});evidence.checks.push('Real Steam genre metadata is proposed before Apply and becomes editable badges.');

    const input=page.getByRole('combobox',{name:'Game tags',exact:true});
    const manual=Object.keys(tags).find(id=>!automatic.includes(id));assert.ok(manual);
    await input.fill(tags[manual]);await input.press('ArrowDown');await input.press('Enter');
    const edited=[...automatic,manual].sort();
    await expect.poll(async()=>(await state()).catalogue.find(item=>item.id===game.id).tagIds).toEqual(edited);
    let rescanned=await rescan();assert.deepEqual(rescanned.catalogue.find(item=>item.id===game.id).tagIds,edited);
    assert.ok(!rescanned.discovery.entries.some(entry=>entry.gameId===game.id&&entry.tagIds?.length));
    evidence.checks.push('Manual additions survive a rescan without automatic replacement.');

    await native({command:'game_tags_save',gameId:game.id,tagIds:[],expectedTagIds:edited});
    rescanned=await rescan();assert.deepEqual(rescanned.catalogue.find(item=>item.id===game.id).tagIds,[]);
    assert.ok(!rescanned.discovery.entries.some(entry=>entry.gameId===game.id&&entry.tagIds?.length));
    await page.reload();assert.deepEqual((await state()).catalogue.find(item=>item.id===game.id).tagIds,[]);
    evidence.checks.push('Explicitly clearing tags survives rescan and reload.');evidence.completed=true;
  }catch(error){evidence.failure=String(error);await page.screenshot({path:resolve(artifacts,'failure.png')}).catch(()=>{});throw error;}
  finally{
    // Leave the selected previously untagged game with its real platform defaults.
    if(game&&automatic){const current=(await state()).catalogue.find(item=>item.id===game.id);if(current)await native({command:'game_tags_save',gameId:game.id,tagIds:automatic,expectedTagIds:current.tagIds});}
    await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');await page.reload().catch(()=>{});await browser.close();
  }
});
