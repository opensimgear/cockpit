// Real library cache -> native gateway/MCP -> image picker and crop editor.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';

test('imported library artwork can be selected, aligned, saved, reopened and edited over MCP',{
  skip:process.env.OSG_GAME_ARTWORK_NATIVE!=='1',timeout:180_000,
},async()=>{
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright';
  const {chromium}=require(module),{expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP('http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(c=>c.pages()).find(p=>/127\.0\.0\.1:142[01]/.test(p.url())&&!p.url().includes('profile-activation-window'));
  assert.ok(page);page.setDefaultTimeout(20_000);
  const artifacts=resolve('.research/game-artwork-native');await mkdir(artifacts,{recursive:true});
  const evidence={actualNativeCockpit:true,gameLaunches:false,checks:[]},errors=[];
  const onError=error=>errors.push(error.message);page.on('pageerror',onError);
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const native=async request=>{const result=await invoke('gateway_request',{request:{...request,clientId:'game-artwork-native-acceptance'}});assert.equal(result.ok,true,request.command+': '+result.error);return result.state;};
  const state=async()=>(await native({command:'game_get_state'})).games;
  let game,original,settings,credentialId,client;
  const setting=async(command,args={})=>{const current=await invoke('mcp_settings_get');return invoke(command,command==='mcp_settings_update'?{settings:{...args,expectedSettingsRevision:current.settingsRevision}}:{...args,expectedSettingsRevision:current.settingsRevision});};
  try{
    await page.reload();
    const before=await state();game=before.catalogue.find(game=>game.name==='Diablo II: Resurrected');assert.ok(game,'Imported Diablo II: Resurrected is required for this local acceptance check.');original=game.background??null;
    const choices=(await native({command:'game_artwork_list',gameId:game.id,limit:64})).gameArtwork;
    assert.ok(choices.items.length>1);assert.ok(choices.items.some(item=>/key art/i.test(item.label)));assert.ok(choices.items.some(item=>/install background/i.test(item.label)));
    evidence.game=game.name;evidence.choices=choices.items.map(({label})=>label);
    await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Games',exact:true}).click();
    const back=page.getByRole('button',{name:'All games',exact:true});if(await back.isVisible())await back.click();
    await page.getByLabel('Filter games by category',{exact:true}).selectOption('');
    await page.getByRole('button',{name:new RegExp('^Open '+game.name.replace(/[.*+?^${}()|[\]\\]/g,'\\$&')+' details')}).click();
    const open=()=>page.getByRole('region',{name:'Tile background',exact:true}).getByRole('button',{name:'Update image',exact:true}).click();
    const dialog=page.getByRole('dialog',{name:'Update tile background',exact:true}),crop=dialog.getByRole('region',{name:'Position tile background',exact:true});
    await open();await expect(dialog.getByRole('button',{name:'From library',exact:true})).toHaveAttribute('aria-pressed','true');
    await expect(dialog.locator('.artwork-option')).toHaveCount(choices.items.length+1+(original&&game.defaultBackground?.image!==original.image?1:0),{timeout:30_000});
    await expect(crop.getByRole('button',{name:'Save background',exact:true})).toBeEnabled();
    await page.screenshot({path:resolve(artifacts,'diablo-artwork-options.png')});
    // The original discovered image previously failed the 64 KiB request guard.
    const initialImage=await crop.getByAltText('Complete background',{exact:true}).getAttribute('src');assert.ok(initialImage.length>65_536);
    await crop.getByRole('button',{name:'Save background',exact:true}).click();await expect(dialog).toHaveCount(0);
    let saved=(await state()).catalogue.find(item=>item.id===game.id).background;assert.equal(saved.image,initialImage);assert.ok(saved.crop);
    evidence.checks.push('Original imported image over 64 KiB saves successfully.');
    await open();
    const alternate=choices.items.find(item=>/key art/i.test(item.label));
    const preview=(await native({command:'game_artwork_preview',gameId:game.id,artworkId:alternate.id})).gameArtwork.image;
    await expect(dialog.locator(`[data-artwork-id="${alternate.id}"]`)).toBeEnabled({timeout:30_000});
    const blocked=await dialog.locator(`[data-artwork-id="${alternate.id}"]`).evaluate(async button=>{button.click();await Promise.resolve();return button.closest('dialog').querySelector('.actions .primary-button').disabled;});
    assert.equal(blocked,true,'Saving the old crop is blocked while the new image loads.');
    await expect(crop.getByAltText('Complete background',{exact:true})).toHaveAttribute('src',preview);
    await expect(crop.getByRole('button',{name:'Save background',exact:true})).toBeEnabled();
    await crop.getByRole('button',{name:'Resize tile crop bottom right',exact:true}).press('ArrowLeft');
    await crop.getByRole('button',{name:'Move tile crop',exact:true}).press('ArrowDown');
    const bounds=await crop.evaluate(node=>{const frame=node.querySelector('.crop-frame').getBoundingClientRect(),image=node.querySelector('.crop-surface').getBoundingClientRect();return {ratio:frame.width/frame.height,inside:frame.x>=image.x-1&&frame.y>=image.y-1&&frame.right<=image.right+1&&frame.bottom<=image.bottom+1};});
    assert.ok(bounds.inside);assert.ok(Math.abs(bounds.ratio-16/9)<.02);
    await page.screenshot({path:resolve(artifacts,'diablo-selected-aligned.png')});
    await crop.getByRole('button',{name:'Save background',exact:true}).click();await expect(dialog).toHaveCount(0);
    saved=(await state()).catalogue.find(item=>item.id===game.id).background;assert.equal(saved.image,preview);
    await page.reload();await open();await expect(crop.getByAltText('Complete background',{exact:true})).toHaveAttribute('src',preview);
    await crop.getByRole('button',{name:'Fit image',exact:true}).click();await crop.getByRole('button',{name:'Cancel',exact:true}).click();
    assert.deepEqual((await state()).catalogue.find(item=>item.id===game.id).background,saved);
    evidence.checks.push('Library selection, loading guard, 16:9 alignment, save/reload and cancel preserve the selected artwork.');
    settings=await invoke('mcp_settings_get');const credential=await setting('mcp_credential_create',{name:'Artwork acceptance',scope:'write'});credentialId=credential.credential.id;
    const enabled=await setting('mcp_settings_update',{enabled:true,port:settings.port});
    client=new Client({name:'Artwork acceptance',version:'1'},{capabilities:{}});
    await client.connect(new StreamableHTTPClientTransport(new URL(enabled.endpoint),{requestInit:{headers:{Authorization:`Bearer ${credential.token}`}},reconnectionOptions:{maxRetries:0}}));
    const call=async(name,args)=>{const reply=await client.callTool({name,arguments:args});const body=reply.structuredContent??JSON.parse(reply.content.find(item=>item.type==='text').text);assert.equal(body.ok,true,JSON.stringify(body.error));return body;};
    const listed=(await call('game_artwork_list',{gameId:game.id,limit:1})).data;assert.equal(listed.items.length,1);assert.ok(listed.nextOffset>0);
    const loaded=(await call('game_artwork_preview',{gameId:game.id,artworkId:alternate.id})).data;assert.equal(loaded.image,preview);
    const args={operationId:randomUUID(),gameId:game.id,imageData:loaded.image,crop:saved.crop};
    const receipt=await call('game_background_save',args);assert.equal(receipt.data.gameId,game.id);assert.ok(JSON.stringify(receipt).length<2000);assert.deepEqual(await call('game_background_save',args),receipt);
    evidence.checks.push('Live MCP lists/pages artwork, loads exact selected image, saves a bounded receipt and replays the same operation.');
    assert.deepEqual(errors,[]);evidence.completed=true;
  }catch(error){evidence.failure=String(error);await page.screenshot({path:resolve(artifacts,'failure.png')}).catch(()=>{});throw error;}
  finally{
    if(client)await client.close().catch(()=>{});
    if(credentialId)await setting('mcp_credential_revoke',{id:credentialId}).catch(()=>{});
    if(settings)await setting('mcp_settings_update',{enabled:settings.enabled,port:settings.port}).catch(()=>{});
    if(game&&original!==undefined)await native(original?{command:'game_background_save',gameId:game.id,imageData:original.image,crop:original.crop}:{command:'game_background_save',gameId:game.id,path:null}).catch(()=>{});
    page.off('pageerror',onError);evidence.pageErrors=errors;
    await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');await browser.close();
  }
});
