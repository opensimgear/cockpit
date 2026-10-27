// Actual native Cockpit + MCP. Restores the taxonomy and game assignments; no games launch.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';

test('game tags support multiple chips, editable taxonomy, filtering, persistence and MCP replay',{
  skip:process.env.OSG_GAME_TAGS_NATIVE!=='1',timeout:180_000,
},async()=>{
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright';
  const {chromium}=require(module),{expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(c=>c.pages()).find(p=>/127\.0\.0\.1:142[01]/.test(p.url())&&!p.url().includes('profile-activation-window'));
  assert.ok(page);page.setDefaultTimeout(20_000);
  const artifacts=resolve('.research/game-tags-native');await mkdir(artifacts,{recursive:true});
  const evidence={actualNativeCockpit:true,gameLaunches:false,checks:[]};let original,game,client,credentialId,settings;
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const native=async request=>{const reply=await invoke('gateway_request',{request:{...request,clientId:'game-tags-native-acceptance'}});assert.equal(reply.ok,true,request.command+': '+reply.error);return reply.state.games;};
  const state=()=>native({command:'game_get_state'});
  const setTags=async tagIds=>{const current=(await state()).catalogue.find(item=>item.id===game.id);return native({command:'game_tags_save',gameId:game.id,tagIds,expectedTagIds:current.tagIds});};
  const uiGames=async()=>{await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Games',exact:true}).click();const back=page.getByRole('button',{name:'All games',exact:true});if(await back.isVisible())await back.click();};
  const openGame=async()=>{await uiGames();await page.locator(`[data-game-id="${game.id}"] .tile-details`).click();};
  const input=page.getByRole('combobox',{name:'Game tags',exact:true});
  const choose=async(field,label)=>{await field.fill(label);await field.press('ArrowDown');await field.press('Enter');};
  const manage=async()=>{await page.getByRole('button',{name:'Manage tags',exact:true}).click();const dialog=page.getByRole('dialog',{name:'Game tags',exact:true});await expect(dialog).toBeVisible();return dialog;};
  const saveDialog=async dialog=>{await dialog.getByRole('button',{name:'Save',exact:true}).click();await expect(dialog).toHaveCount(0);};
  const setting=async(command,args={})=>{const current=await invoke('mcp_settings_get');return invoke(command,command==='mcp_settings_update'?{settings:{...args,expectedSettingsRevision:current.settingsRevision}}:{...args,expectedSettingsRevision:current.settingsRevision});};
  try{
    await page.reload();original=await state();assert.ok(Object.keys(original.tags).length>=47);
    for(const key of ['racing','flight','simulation','fps','rpg','rts'])assert.ok(original.tags[key]);
    game=original.catalogue.find(item=>item.name==='SteamVR')??original.catalogue[0];assert.ok(game);
    await setTags([]);await openGame();
    await choose(input,'Racing');await expect.poll(async()=>(await state()).catalogue.find(item=>item.id===game.id).tagIds).toContain('racing');
    await input.fill('Simulation');await page.getByRole('option',{name:'Simulation',exact:true}).click();await expect.poll(async()=>(await state()).catalogue.find(item=>item.id===game.id).tagIds).toEqual(['racing','simulation']);
    await expect(page.getByRole('button',{name:'Remove Racing',exact:true})).toBeVisible();await expect(page.getByRole('button',{name:'Remove Simulation',exact:true})).toBeVisible();
    await input.fill('');await input.press('Backspace');await expect.poll(async()=>(await state()).catalogue.find(item=>item.id===game.id).tagIds.length).toBe(1);
    await choose(input,'Simulation');await expect.poll(async()=>(await state()).catalogue.find(item=>item.id===game.id).tagIds).toEqual(['racing','simulation']);
    await page.screenshot({path:resolve(artifacts,'two-tags.png')});evidence.checks.push('Editable starter taxonomy; searchable chips; Enter adds and Backspace removes without changing another tag.');

    await uiGames();await page.getByRole('button',{name:'Filter games',exact:true}).click();
    const filters=page.getByRole('region',{name:'Game filters',exact:true}),filterInput=filters.getByRole('combobox',{name:'Tags',exact:true});
    await choose(filterInput,'Simulation');
    const matching=(await state()).catalogue.filter(item=>item.tagIds.includes('simulation')).map(item=>item.id).sort();
    await expect.poll(async()=>(await page.locator('.game-tile').evaluateAll(tiles=>tiles.map(tile=>tile.dataset.gameId))).sort()).toEqual(matching);
    await filters.getByRole('button',{name:'Clear',exact:true}).click();await page.locator(`[data-game-id="${game.id}"] .tile-details`).click();
    evidence.checks.push('The game list filters by tag membership.');

    const renamed='Motorsport acceptance '+Date.now();let dialog=await manage();
    await dialog.getByRole('button',{name:'Rename Racing',exact:true}).click();
    await dialog.getByRole('textbox',{name:'New name for Racing',exact:true}).fill(renamed);await dialog.getByRole('textbox').press('Enter');await expect(dialog.locator('[data-tag-id="racing"]')).toBeVisible();await saveDialog(dialog);
    let current=await state();assert.equal(current.tags.racing,renamed);assert.deepEqual(current.catalogue.find(item=>item.id===game.id).tagIds,['racing','simulation']);
    await page.reload();await expect(input).toBeVisible();await expect(page.getByRole('button',{name:'Remove '+renamed,exact:true})).toBeVisible();
    dialog=await manage();await dialog.getByRole('button',{name:'Delete '+renamed,exact:true}).click();
    const confirmation=page.getByRole('alertdialog'),affected=(await state()).catalogue.filter(item=>item.tagIds.includes('racing')).length;
    await expect(confirmation).toContainText(`from ${affected} ${affected===1?'game':'games'}`);await confirmation.getByRole('button',{name:'Cancel',exact:true}).click();await expect(dialog.locator('[data-tag-id="racing"]')).toBeVisible();
    await dialog.getByRole('button',{name:'Delete '+renamed,exact:true}).click();await confirmation.getByRole('button',{name:'Delete',exact:true}).click();
    const managerInput=dialog.getByRole('textbox',{name:'Tags',exact:true});await managerInput.fill('Simulation');await managerInput.press('Enter');await expect(dialog.getByRole('alert')).toContainText('distinct names');
    const newName='Acceptance tag '+Date.now();await managerInput.fill(newName);await managerInput.press('Enter');const tagId=await dialog.getByRole('button',{name:'Rename '+newName,exact:true}).locator('..').getAttribute('data-tag-id');await saveDialog(dialog);
    current=await state();assert.ok(!Object.hasOwn(current.tags,'racing'));assert.deepEqual(current.catalogue.find(item=>item.id===game.id).tagIds,['simulation']);
    evidence.checks.push('Multiline badges can be added and renamed; deleting shows the affected game count and requires confirmation; IDs survive reload and other tags stay assigned.');

    settings=await invoke('mcp_settings_get');const credential=await setting('mcp_credential_create',{name:'Tag acceptance',scope:'write'});credentialId=credential.credential.id;
    const enabled=await setting('mcp_settings_update',{enabled:true,port:settings.port});client=new Client({name:'Tag acceptance',version:'1'},{capabilities:{}});
    await client.connect(new StreamableHTTPClientTransport(new URL(enabled.endpoint),{requestInit:{headers:{Authorization:`Bearer ${credential.token}`}},reconnectionOptions:{maxRetries:0}}));
    const call=async(name,args)=>{const reply=await client.callTool({name,arguments:args}),body=reply.structuredContent??JSON.parse(reply.content.find(item=>item.type==='text').text);assert.equal(body.ok,true,JSON.stringify(body.error));return body;};
    const taxonomy=(await call('game_tags_list',{})).data.tags;assert.equal(taxonomy.simulation,'Simulation');assert.ok(taxonomy[tagId]);
    const next={...taxonomy,[tagId]:'MCP edited tag'};await call('game_tag_definitions_save',{operationId:randomUUID(),tags:next,expectedTags:taxonomy});
    const args={operationId:randomUUID(),gameId:game.id,tagIds:['simulation',tagId],expectedTagIds:['simulation']};
    const receipt=await call('game_tags_save',args);assert.deepEqual(receipt.data.tagIds,[tagId,'simulation'].sort());assert.ok(JSON.stringify(receipt).length<3000);assert.deepEqual(await call('game_tags_save',args),receipt);
    await page.reload();await expect(page.getByRole('button',{name:'Remove MCP edited tag',exact:true})).toBeVisible();
    evidence.checks.push('Actual MCP lists and edits taxonomy, assigns multiple tags, returns a bounded receipt and replays an operation without applying it twice.');
    await page.screenshot({path:resolve(artifacts,'mcp-tags.png')});evidence.completed=true;
  }catch(error){evidence.failure=String(error);await page.screenshot({path:resolve(artifacts,'failure.png')}).catch(()=>{});throw error;}
  finally{
    if(client)await client.close().catch(()=>{});if(credentialId)await setting('mcp_credential_revoke',{id:credentialId}).catch(()=>{});
    if(settings)await setting('mcp_settings_update',{enabled:settings.enabled,port:settings.port}).catch(()=>{});
    if(original){const current=await state();await native({command:'game_tag_definitions_save',tags:original.tags,expectedTags:current.tags});if(game)await setTags(game.tagIds);}
    await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');await page.reload().catch(()=>{});await browser.close();
  }
});
