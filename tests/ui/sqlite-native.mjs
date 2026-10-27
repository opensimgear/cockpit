// Runs against the actual worktree dev Cockpit, never a mocked gateway.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';

test('native SQLite service is healthy through real Cockpit IPC', {
  skip: !process.env.OSG_SQLITE_NATIVE_ACCEPTANCE,
  timeout: 60_000,
}, async () => {
  const require = createRequire(import.meta.url);
  const module=process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
  const { chromium } = require(module),{expect}=require(module+'/test');
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9224');
  try {
    const page = browser.contexts().flatMap(context => context.pages()).find(page => page.url().startsWith(process.env.OSG_COCKPIT_DEV_URL ?? 'http://127.0.0.1:1421/'));
    assert.ok(page, 'The feature worktree dev Cockpit must be running.');
    await page.reload();
    await expect(page.getByRole('navigation', {name:'Main navigation'})).toBeVisible();
    const status = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('persistence_status'));
    assert.equal(status.healthy, true);
    assert.equal(status.sqliteVersion, '3.53.2');
    assert.equal(status.journalMode, 'delete');
    assert.equal(status.synchronous, 'full');
    assert.equal(status.busyTimeoutMs, 250);
    assert.match(status.path, /cockpit\.sqlite3$/);
    if (process.env.OSG_SQLITE_ACCEPTANCE_DB) assert.equal(status.path.replaceAll('\\','/'), process.env.OSG_SQLITE_ACCEPTANCE_DB.replaceAll('\\','/'));
    console.log(JSON.stringify({url:page.url(),persistence:status}));
  } finally { await browser.close(); }
});

test('profile UI saves into SQLite and survives a complete native process restart', {
  skip: !process.env.OSG_SQLITE_NATIVE_ACCEPTANCE || !process.env.OSG_SQLITE_RESTART_PHASE,
  timeout: 90_000,
}, async () => {
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
  const {chromium}=require(module),{expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224');
  try {
    const page=browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().startsWith(process.env.OSG_COCKPIT_DEV_URL??'http://127.0.0.1:1421/'));
    assert.ok(page);
    const native=async request=>{
      const result=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'sqlite-live-acceptance'}}),request);
      assert.equal(result.ok,true,`${request.command}: ${result.error}`);return result.state;
    };
    const status=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('persistence_status'));
    const evidencePath=resolve('.research/sqlite/profile-restart.json');
    await native({command:'profile_stop'});await native({command:'profile_selection_mode',mode:'manual'});
    if(process.env.OSG_SQLITE_RESTART_PHASE==='prepare') {
      const originalName=`SQLite acceptance ${Date.now()}`,name=`${originalName} saved`;
      const created=(await native({command:'profile_create',name:originalName})).profiles.document;
      await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Profiles',exact:true}).click();
      await page.getByRole('button',{name:`Edit ${originalName}`,exact:true}).click();
      await page.getByRole('button',{name:'Edit profile name',exact:true}).click();
      await page.getByLabel('Profile name',{exact:true}).fill(name);
      await page.getByLabel('Profile name',{exact:true}).press('Enter');
      await page.getByRole('button',{name:'Save',exact:true}).click();
      await expect.poll(async()=>(await native({command:'profile_get',id:created.id})).profiles.document.name).toBe(name);
      const saved=(await native({command:'profile_get',id:created.id})).profiles.document;
      assert.equal(saved.revision,2);
      const indexed=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('persistence_status'));
      assert.equal(indexed.profileCount,(await native({command:'profile_list'})).profiles.profiles.length);
      await mkdir(resolve('.research/sqlite'),{recursive:true});
      await writeFile(evidencePath,JSON.stringify({processId:status.processId,document:saved},null,2));
    } else {
      assert.equal(process.env.OSG_SQLITE_RESTART_PHASE,'verify');
      const evidence=JSON.parse(await readFile(evidencePath,'utf8'));
      assert.notEqual(status.processId,evidence.processId,'Restart the native Cockpit process between phases.');
      const saved=(await native({command:'profile_get',id:evidence.document.id})).profiles.document;
      assert.deepEqual(saved,evidence.document);
      await native({command:'profile_delete',id:saved.id,expectedRevision:saved.revision});
      await writeFile(evidencePath,JSON.stringify({...evidence,restartedProcessId:status.processId,verified:true,deleted:true},null,2));
    }
  } finally {await browser.close();}
});


test('game metadata, artwork, provider and output preferences survive native restart in SQLite', {
  skip: !process.env.OSG_SQLITE_NATIVE_ACCEPTANCE || !process.env.OSG_SQLITE_GAMES_RESTART_PHASE,
  timeout: 120_000,
}, async () => {
  const require=createRequire(import.meta.url), module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
  const {chromium}=require(module), {expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224');
  try {
    const page=browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().startsWith(process.env.OSG_COCKPIT_DEV_URL??'http://127.0.0.1:1421/'));
    assert.ok(page);
    const native=async request=>{
      const reply=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'sqlite-games-acceptance'}}),request);
      assert.equal(reply.ok,true,request.command+': '+reply.error); return reply.state;
    };
    const games=async()=>(await native({command:'game_get_state'})).games;
    const desktop=async()=>(await native({command:'profile_desktop_status'})).desktop;
    const status=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('persistence_status'));
    const evidencePath=resolve('.research/sqlite/game-restart.json');
    await native({command:'profile_stop'}); await native({command:'profile_selection_mode',mode:'manual'});
    if(process.env.OSG_SQLITE_GAMES_RESTART_PHASE==='prepare') {
      const name='SQLite game '+Date.now();
      const desktopBefore=await desktop();
      await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Games',exact:true}).click();
      const back=page.getByRole('button',{name:'All games',exact:true}); if(await back.isVisible())await back.click();
      await page.getByRole('button',{name:'Add game',exact:true}).click();
      const dialog=page.getByRole('dialog',{name:'Add custom game',exact:true});
      await dialog.getByLabel('Custom game name',{exact:true}).fill(name);
      await dialog.getByRole('button',{name:'Add game',exact:true}).click();
      await expect(dialog).toHaveCount(0);
      const game=(await games()).catalogue.find(game=>game.name===name); assert.ok(game);
      const renamed=name+' saved'; await page.getByLabel('Game name',{exact:true}).fill(renamed);await page.getByLabel('Game name',{exact:true}).press('Enter');
      await expect.poll(async()=>(await games()).catalogue.find(item=>item.id===game.id)?.name).toBe(renamed);
      const background=(await native({command:'game_background_save',gameId:game.id,path:resolve('tests/fixtures/artwork/landscape.jpg'),positionX:17,positionY:83})).games.catalogue.find(item=>item.id===game.id).background;
      const xp=(await games()).catalogue.find(game=>game.id==='x-plane-12');
      const connection={...xp.integrationSetup.connection,freshnessMs:1900};
      await native({command:'game_integration_settings_save',gameId:xp.id,expectedSetupRevision:xp.integrationSetup.providerSetupRevision,connection});
      await native({command:'profile_desktop_configure',enabled:true,scope:{kind:'desktop-wide'}});
      const settings=await desktop(); assert.equal(settings.enabled,true);assert.deepEqual(settings.scope,{kind:'desktop-wide'});
      await writeFile(evidencePath,JSON.stringify({processId:status.processId,gameId:game.id,name:renamed,background,connection,desktopBefore},null,2));
      // The stale source is deliberately invalid; reopening must use SQLite.
      const directory=dirname(status.path);
      await writeFile(resolve(directory,'game-installations-v1.json'),'retired source must never replace SQLite');
      await writeFile(resolve(directory,'games.integration-setup.json'),'retired provider source');
      await writeFile(resolve(directory,'desktop-input-v1.json'),'retired output preference source');
    } else {
      assert.equal(process.env.OSG_SQLITE_GAMES_RESTART_PHASE,'verify');
      const evidence=JSON.parse(await readFile(evidencePath,'utf8'));assert.notEqual(status.processId,evidence.processId);
      const current=await games(),game=current.catalogue.find(item=>item.id===evidence.gameId);assert.ok(game);assert.equal(game.name,evidence.name);assert.deepEqual(game.background,evidence.background);
      assert.deepEqual(current.catalogue.find(item=>item.id==='x-plane-12').integrationSetup.connection,evidence.connection);
      const settings=await desktop();assert.equal(settings.enabled,true);assert.deepEqual(settings.scope,{kind:'desktop-wide'});
      await native({command:'profile_desktop_configure',enabled:evidence.desktopBefore.enabled,scope:evidence.desktopBefore.scope});
      await native({command:'game_custom_remove',gameId:game.id});
      assert.equal((await native({command:'profile_status'})).profiles.profileSession.state,'stopped');
      await writeFile(evidencePath,JSON.stringify({...evidence,verified:true,restartedProcessId:status.processId,staleSourcesIgnored:true},null,2));
    }
  } finally {await browser.close();}
});


test('the native UI and MCP widget source repository survives a process restart', {
  skip: !process.env.OSG_SQLITE_NATIVE_ACCEPTANCE || !process.env.OSG_SQLITE_WIDGET_RESTART_PHASE,
  timeout: 60_000,
}, async () => {
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
  const {chromium}=require(module),browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224');
  try {
    const page=browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().startsWith(process.env.OSG_COCKPIT_DEV_URL??'http://127.0.0.1:1421/'));assert.ok(page);
    const native=async request=>{const reply=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'sqlite-widget-acceptance'}}),request);assert.equal(reply.ok,true,reply.error);return reply.state;};
    const status=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('persistence_status'));
    const evidencePath=resolve('.research/sqlite/widget-restart.json');
    if(process.env.OSG_SQLITE_WIDGET_RESTART_PHASE==='prepare') {
      const initial=await native({command:'widget_get',id:'toggle'});
      const bundle=structuredClone(initial.widgets??initial.widgetLibrary??initial);
      assert.ok(Array.isArray(bundle.definitions));
      const source=bundle.definitions[0];source.id='sqlite-widget-'+Date.now();source.title='SQLite source acceptance';
      await native({command:'widget_save',bundle:{definitions:bundle.definitions},expectedRevision:bundle.revision});
      const saved=await native({command:'widget_get',id:source.id});
      await writeFile(evidencePath,JSON.stringify({processId:status.processId,id:source.id,saved},null,2));
      await writeFile(resolve(dirname(status.path),'widgets-v1.json'),'retired widget file');
    } else {
      assert.equal(process.env.OSG_SQLITE_WIDGET_RESTART_PHASE,'verify');
      const evidence=JSON.parse(await readFile(evidencePath,'utf8'));assert.notEqual(status.processId,evidence.processId);
      assert.deepEqual(await native({command:'widget_get',id:evidence.id}),evidence.saved);
      await writeFile(evidencePath,JSON.stringify({...evidence,verified:true,restartedProcessId:status.processId},null,2));
    }
  } finally {await browser.close();}
});
