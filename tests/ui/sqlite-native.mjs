// Runs against the actual worktree dev Cockpit, never a mocked gateway.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {resolve} from 'node:path';

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
    assert.equal(status.schemaVersion, 3);
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
      await page.getByRole('button',{name:'Save profile',exact:true}).click();
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
