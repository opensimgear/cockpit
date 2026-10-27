// Real native discovery, Wikidata cache, editable badges and read-only MCP.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createServer} from 'node:net';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';

test('Wikidata fallback supplies reusable metadata while manual tags remain authoritative',{
  skip:process.env.OSG_WIKIDATA_NATIVE!=='1',timeout:240_000,
},async()=>{
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright';
  const {chromium}=require(module),{expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:142[01]/.test(page.url())&&!page.url().includes('profile-activation-window'));
  assert.ok(page);page.setDefaultTimeout(20_000);
  const directory=resolve('.research/wikidata-native');await mkdir(directory,{recursive:true});
  const evidence={actualNativeCockpit:true,gameLaunches:false,checks:[]};let game,original,client,credentialId,settings;
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const request=async value=>{const reply=await invoke('gateway_request',{request:{...value,clientId:'wikidata-native-acceptance'}});assert.equal(reply.ok,true,value.command+': '+reply.error);return reply.state;};
  const state=async()=>(await request({command:'game_get_state'})).games;
  const scan=async()=>{
    const current=await state();if(current.discovery?.status!=='scanning')await request({command:'game_discovery_start'});
    await expect.poll(async()=>(await state()).discovery.status,{timeout:90_000}).not.toBe('scanning');return state();
  };
  const setting=async(command,args={})=>{const current=await invoke('mcp_settings_get');return invoke(command,command==='mcp_settings_update'?{settings:{...args,expectedSettingsRevision:current.settingsRevision}}:{...args,expectedSettingsRevision:current.settingsRevision});};
  try{
    await page.reload();await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Games',exact:true}).click();
    let current=await scan();
    const entry=current.discovery.entries.find(entry=>entry.tagSource==='wikidata'&&entry.tagIds?.length);
    assert.ok(entry,'Need a discovered title with a unique Wikidata match and fallback tags.');
    if(current.discovery.status==='ready'){
      const result=await invoke('gateway_request',{request:{command:'game_discovery_apply',scanId:current.discovery.scanId,actions:current.discovery.entries.map(item=>({id:item.id,selected:item.action!=='remove'})),clientId:'wikidata-native-acceptance'}});
      if(!result.ok)assert.equal((await state()).discovery.status,'applied');
    }
    current=await state();game=current.catalogue.find(game=>game.id===entry.gameId);assert.ok(game);original=game.tagIds;
    assert.ok(original.length);const metadata=(await request({command:'game_metadata_get',gameId:game.id})).gameMetadata;
    assert.equal(metadata.status,'matched');assert.equal(metadata.sourceName,'Wikidata');
    const record=metadata.results.find(result=>result.lookup.status==='matched').lookup.metadata;
    assert.match(record.entityId,/^Q[1-9][0-9]*$/);assert.ok(record.genres.length);
    for(const field of ['platforms','developers','publishers','releaseDates','externalIds','genreAncestors'])assert.ok(Array.isArray(record[field]));
    evidence.entityId=record.entityId;evidence.platform=entry.library;
    evidence.checks.push('Actual fallback tags come from a uniquely resolved Wikidata entity, with rich metadata and provenance available separately.');

    await page.reload();await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Games',exact:true}).click();
    const back=page.getByRole('button',{name:'All games',exact:true});if(await back.isVisible())await back.click();
    await page.locator(`[data-game-id="${game.id}"] .tile-details`).click();
    const tags=(await state()).tags,manual=Object.keys(tags).find(id=>!original.includes(id));assert.ok(manual);
    const input=page.getByRole('combobox',{name:'Game tags',exact:true});await input.fill(tags[manual]);await input.press('ArrowDown');await input.press('Enter');
    const edited=[...original,manual].sort();await expect.poll(async()=>(await state()).catalogue.find(item=>item.id===game.id).tagIds).toEqual(edited);
    current=await scan();assert.deepEqual(current.catalogue.find(item=>item.id===game.id).tagIds,edited);
    await request({command:'game_tags_save',gameId:game.id,tagIds:[],expectedTagIds:edited});
    current=await scan();assert.deepEqual(current.catalogue.find(item=>item.id===game.id).tagIds,[]);
    assert.ok(!current.discovery.entries.some(entry=>entry.gameId===game.id&&entry.tagIds?.length));
    assert.equal((await request({command:'game_metadata_get',gameId:game.id})).gameMetadata.status,'matched');
    evidence.checks.push('Manual additions and explicit clearing survive rescans while metadata remains available.');

    settings=await invoke('mcp_settings_get');const socket=createServer();await new Promise(resolve=>socket.listen(0,'127.0.0.1',resolve));const port=socket.address().port;await new Promise(resolve=>socket.close(resolve));
    const credential=await setting('mcp_credential_create',{name:'Wikidata acceptance',scope:'read'});credentialId=credential.credential.id;
    const enabled=await setting('mcp_settings_update',{enabled:true,port});client=new Client({name:'Wikidata acceptance',version:'1'},{capabilities:{}});
    await client.connect(new StreamableHTTPClientTransport(new URL(enabled.endpoint),{requestInit:{headers:{Authorization:`Bearer ${credential.token}`}},reconnectionOptions:{maxRetries:0}}));
    const reply=await client.callTool({name:'game_metadata_get',arguments:{gameId:game.id}}),body=reply.structuredContent??JSON.parse(reply.content.find(item=>item.type==='text').text);
    assert.equal(body.ok,true);assert.equal(body.data.status,'matched');assert.equal(body.data.license,'CC0');
    evidence.checks.push('Read-only MCP credentials retrieve cached general metadata without a write operation or network refresh.');evidence.completed=true;
    await page.screenshot({path:resolve(directory,'editable-tags.png')});
  }catch(error){evidence.failure=String(error);await page.screenshot({path:resolve(directory,'failure.png')}).catch(()=>{});throw error;}
  finally{
    if(client)await client.close().catch(()=>{});if(credentialId)await setting('mcp_credential_revoke',{id:credentialId}).catch(()=>{});
    if(settings)await setting('mcp_settings_update',{enabled:settings.enabled,port:settings.port}).catch(()=>{});
    if(game&&original){const current=(await state()).catalogue.find(item=>item.id===game.id);if(current)await request({command:'game_tags_save',gameId:game.id,tagIds:original,expectedTagIds:current.tagIds});}
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');await page.reload().catch(()=>{});await browser.close();
  }
});
