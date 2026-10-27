// Runs only against a fresh isolated test DB, verified through native persistence_status.
// Never point this at the shared dev database: the scenario clears its own test events.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve,relative,isAbsolute} from 'node:path';
import {randomUUID} from 'node:crypto';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';

test('Activity captures native changes, searches, filters, paginates, persists settings and supports MCP',{
  skip:process.env.OSG_ACTIVITY_NATIVE!=='1',timeout:240_000,
},async()=>{
  const artifacts=resolve('.research/activity-native'),expectedDatabase=process.env.OSG_ACTIVITY_TEST_DATABASE;
  assert.ok(expectedDatabase,'An explicit isolated Activity test database is required.');
  const child=relative(artifacts,resolve(expectedDatabase));
  assert.ok(child&&!child.startsWith('..')&&!isAbsolute(child),'Database must be inside .research/activity-native.');
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright';
  const {chromium}=require(module),{expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9232');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:142[01]/.test(page.url())&&!page.url().includes('profile-activation-window'));
  assert.ok(page,'Run the native Cockpit with the isolated database.');page.setDefaultTimeout(20_000);
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const storage=await invoke('persistence_status');
  assert.equal(resolve(storage.path).toLowerCase(),resolve(expectedDatabase).toLowerCase(),'Refuse to change a shared or unexpected database.');
  await mkdir(artifacts,{recursive:true});
  const evidence={actualNativeCockpit:true,isolatedDatabase:storage.path,checks:[]},errors=[];
  page.on('pageerror',error=>errors.push(error.message));
  const native=async request=>{const result=await invoke('gateway_request',{request:{...request,clientId:'activity-native-acceptance'}});assert.equal(result.ok,true,request.command+': '+result.error);return result.state;};
  const list=async(activityFilter={})=>(await native({command:'activity_query',activityFilter})).activity;
  const navigation=page.getByRole('navigation',{name:'Main navigation'}),activity=()=>navigation.getByRole('button',{name:'Activity',exact:true}).click();
  const log=page.getByRole('region',{name:'Activity log',exact:true}),unique='Activity acceptance '+Date.now();
  let profile,game,client;
  const setting=async(command,args={})=>{const current=await invoke('mcp_settings_get');return invoke(command,command==='mcp_settings_update'?{settings:{...args,expectedSettingsRevision:current.settingsRevision}}:{...args,expectedSettingsRevision:current.settingsRevision});};
  try{
    await page.reload();assert.equal((await native({command:'activity_settings_get'})).activitySettings.retentionDays,7);
    profile=(await native({command:'profile_create',name:unique})).profiles.document;assert.ok(profile);
    let document=(await native({command:'profile_get',id:profile.id})).profiles.document;document.name=unique+' updated';
    profile=(await native({command:'profile_editor_save',document,expectedRevision:document.revision,assignments:[]})).profiles.document;
    await native({command:'profile_apply',id:profile.id,expectedRevision:profile.revision});await native({command:'profile_stop'});
    const installation=resolve(artifacts,'game-'+Date.now());await mkdir(installation);
    const executable=resolve(installation,'ActivityFixture.exe');await writeFile(executable,'Activity test fixture. Never executed.');
    const games=(await native({command:'game_add',name:unique+' game',installPath:installation,executable})).games;
    game=games.catalogue.find(item=>item.name===unique+' game');assert.ok(game);
    await native({command:'game_save',gameId:game.id,name:unique+' game updated'});
    await expect.poll(async()=>(await list({search:unique})).total).toBeGreaterThan(3);
    const recorded=await list({entityId:profile.id});
    for(const pattern of [/activat|profile_apply/,/stopped|profile_stop/,/updated|save/])assert.ok(recorded.events.some(event=>pattern.test(event.eventType)));
    assert.ok((await list({entityId:game.id})).events.length>0);
    await activity();await expect(log.locator('.log-event')).not.toHaveCount(0);await page.reload();await expect(log.locator('.log-event')).not.toHaveCount(0);
    evidence.checks.push('Native profile create/update/activate/stop and game add/update are recorded and survive UI reload.');

    const search=page.getByRole('searchbox',{name:'Search activity',exact:true});await search.fill(unique);await expect(log).toContainText(unique);
    await page.getByRole('button',{name:'Filter activity',exact:true}).click();const filters=page.getByRole('region',{name:'Activity filters',exact:true});
    await filters.getByRole('checkbox',{name:'Profiles',exact:true}).check();await filters.getByLabel('Activity time',{exact:true}).selectOption('hour');
    await filters.getByLabel('Activity entity ID',{exact:true}).fill(profile.id);
    await expect.poll(async()=>[...new Set(await log.locator('.event-category').allTextContents())]).toEqual(['Profiles']);
    await log.locator('.log-event summary').first().click();await expect(log.locator('.event-detail').first()).toContainText(profile.id);
    await page.screenshot({path:resolve(artifacts,'search-filters.png')});
    await search.fill('no-match-'+randomUUID());await expect(log.getByText('No matching activity',{exact:true})).toBeVisible();
    await search.fill('');await filters.getByRole('button',{name:'Clear',exact:true}).click();await page.getByRole('button',{name:'Filter activity',exact:true}).click();
    evidence.checks.push('Compact rows expand details; full text, category, time and entity filters work; empty results and clearing filters work.');

    await page.getByRole('button',{name:'Settings',exact:true}).click();const settings=page.getByRole('region',{name:'Activity retention',exact:true});
    await settings.getByRole('spinbutton',{name:'Activity retention days',exact:true}).fill('14');await settings.getByRole('button',{name:'Save',exact:true}).click();
    await expect(settings.getByRole('status')).toContainText('Retention saved');await page.reload();await expect(settings.getByRole('spinbutton',{name:'Activity retention days',exact:true})).toHaveValue('14');
    evidence.checks.push('Seven-day default and edited retention survive reload.');

    const mcpSettings=await invoke('mcp_settings_get'),credential=await setting('mcp_credential_create',{name:'Activity test fixture',scope:'write'});
    const enabled=await setting('mcp_settings_update',{enabled:true,port:mcpSettings.port});client=new Client({name:'Activity acceptance',version:'1'},{capabilities:{}});
    await client.connect(new StreamableHTTPClientTransport(new URL(enabled.endpoint),{requestInit:{headers:{Authorization:`Bearer ${credential.token}`}},reconnectionOptions:{maxRetries:0}}));
    const call=async(name,args={})=>{const response=await client.callTool({name,arguments:args}),body=response.structuredContent??JSON.parse(response.content.find(item=>item.type==='text').text);assert.equal(body.ok,true,JSON.stringify(body.error));return body.data;};
    for(const name of ['activity_list','activity_clear','activity_settings_get','activity_settings_save'])assert.ok((await client.listTools()).tools.some(tool=>tool.name===name));
    assert.ok((await call('activity_list',{search:unique,limit:2})).events.length>0);
    await call('activity_settings_save',{operationId:randomUUID(),retentionDays:7});assert.equal((await call('activity_settings_get')).retentionDays,7);
    const clearing={operationId:randomUUID()};await call('activity_clear',clearing);
    profile=(await native({command:'profile_rename',id:profile.id,name:unique+' after clear',expectedRevision:profile.revision})).profiles.document;
    await call('activity_clear',clearing);assert.ok((await list({search:unique})).total>0,'An old clear replay preserves later events.');
    evidence.checks.push('Actual MCP lists/searches activity, saves retention and replays clear idempotently.');

    for(let index=0;index<102;index++)profile=(await native({command:'profile_rename',id:profile.id,name:unique+' page '+index,expectedRevision:profile.revision})).profiles.document;
    await activity();await expect(log.getByRole('button',{name:'Older',exact:true})).toBeEnabled();await log.getByRole('button',{name:'Older',exact:true}).click();await expect(log).toContainText('Live updates paused on older pages');await log.getByRole('button',{name:'Newer',exact:true}).click();
    await page.getByRole('button',{name:'Clear all',exact:true}).click();let dialog=page.getByRole('dialog',{name:'Clear all activity?',exact:true});await dialog.getByRole('button',{name:'Cancel',exact:true}).click();assert.ok((await list()).total>0);
    await page.getByRole('button',{name:'Clear all',exact:true}).click();dialog=page.getByRole('dialog',{name:'Clear all activity?',exact:true});await dialog.getByRole('button',{name:'Clear all',exact:true}).click();await expect(dialog).toHaveCount(0);assert.equal((await list()).total,0);await expect(log.getByText('No activity recorded yet',{exact:true})).toBeVisible();
    evidence.checks.push('More than 100 real metadata changes paginate; clear-all cancellation works and confirmation deletes only isolated test history.');
    await page.screenshot({path:resolve(artifacts,'cleared.png')});assert.deepEqual(errors,[]);evidence.completed=true;
  }catch(error){evidence.failure=String(error);await page.screenshot({path:resolve(artifacts,'failure.png')}).catch(()=>{});throw error;}
  finally{if(client)await client.close().catch(()=>{});await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');await browser.close();}
});
