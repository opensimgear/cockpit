// Actual isolated dev Cockpit: no browser/native gateway mocks.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {resolve} from 'node:path';

test('designer source, draft and compiled assets use native SQLite across process restart',{
  skip:!process.env.OSG_SQLITE_NATIVE_ACCEPTANCE || !process.env.OSG_SQLITE_DESIGNER_RESTART_PHASE,
  timeout:180_000,
},async()=>{
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
  const {chromium}=require(module),{expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224');
  const evidencePath=resolve('.research/sqlite/designer-restart.json');
  try {
    const page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith(process.env.OSG_COCKPIT_DEV_URL??'http://127.0.0.1:1421/'));assert.ok(page);
    await page.evaluate(()=>history.replaceState(null,'','/'));
    await page.reload();
    const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
    const status=await invoke('persistence_status');assert.equal(status.healthy,true);
    const snapshot=()=>invoke('persistence_workspace',{request:{operation:'read'}});
    const retired=async(write=false)=>page.evaluate(write=>new Promise((resolve,reject)=>{
      const open=indexedDB.open('opensimgear-cockpit-companion-designer',1);
      open.onupgradeneeded=()=>open.result.createObjectStore('workspace');open.onerror=()=>reject(open.error);
      open.onsuccess=()=>{const db=open.result,tx=db.transaction('workspace',write?'readwrite':'readonly'),store=tx.objectStore('workspace');const r=write?store.put({retiredEvidence:'SQLite acceptance'},'snapshot'):store.get('snapshot');tx.oncomplete=()=>{db.close();resolve(r.result);};tx.onerror=()=>reject(tx.error);};
    }),write);
    if(process.env.OSG_SQLITE_DESIGNER_RESTART_PHASE==='prepare') {
      await retired(true);
      const sourceName='SQLite source '+Date.now(),panelName='SQLite panel '+Date.now();
      await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Companions',exact:true}).click();
      await page.locator('section[aria-label="Widgets"]').getByRole('button',{name:'New',exact:true}).click();
      const dialog=page.locator('dialog.generic-designer');await expect(dialog).toBeVisible();
      await dialog.getByLabel('Widget name',{exact:true}).fill(sourceName);await dialog.getByLabel('Widget name',{exact:true}).press('Tab');
      await dialog.getByRole('button',{name:'Compile',exact:true}).click();
      await expect(dialog.locator('.buildbar [role="status"]')).toHaveText('Compilation complete',{timeout:90_000});
      await dialog.getByRole('button',{name:'Save',exact:true}).click();await expect(dialog).not.toBeVisible();
      await expect(page.getByRole('button',{name:'Edit design '+sourceName,exact:true})).toBeVisible();
      const withSource=await snapshot();assert.ok(withSource.value.sources.some(s=>s.title===sourceName));
      await page.locator('section[aria-label="Panels"]').getByRole('button',{name:'New',exact:true}).click();
      await page.getByLabel('Panel name',{exact:true}).fill(panelName);await page.getByLabel('Panel name',{exact:true}).press('Tab');
      await page.locator('[aria-label="Panel configuration"]').getByRole('button',{name:sourceName,exact:true}).click();
      const designer=page.locator('.compiled-designer');
      await designer.getByRole('button',{name:'Compile',exact:true}).click();
      await expect(designer.getByRole('status')).toHaveText('Compilation complete',{timeout:90_000});
      await designer.getByRole('button',{name:'Save',exact:true}).click();
      await expect.poll(async()=>(await snapshot()).value.drafts.some(d=>d.title===panelName),{timeout:30_000}).toBe(true);
      await expect(designer.getByText('Saved · Browser target',{exact:true})).toBeVisible({timeout:30_000});
      const saved=await snapshot(),draft=saved.value.drafts.find(d=>d.title===panelName);
      assert.ok(saved.value.publications[draft.id].at(-1).assets.length>0);
      assert.deepEqual(await retired(),{retiredEvidence:'SQLite acceptance'});
      await mkdir(resolve('.research/sqlite'),{recursive:true});
      await writeFile(evidencePath,JSON.stringify({processId:status.processId,sourceName,panelName,workspace:saved.value},null,2));
      console.log(JSON.stringify({phase:'prepare',processId:status.processId,revision:saved.revision,panelId:draft.id,publicationHash:saved.value.publications[draft.id].at(-1).publicationHash}));
    } else {
      const previous=JSON.parse(await readFile(evidencePath,'utf8'));assert.notEqual(status.processId,previous.processId);
      const saved=await snapshot();assert.deepEqual(saved.value,previous.workspace);
      await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Companions',exact:true}).click();
      await expect(page.getByRole('button',{name:'Edit design '+previous.sourceName,exact:true})).toBeVisible();
      await page.getByRole('button',{name:'Edit '+previous.panelName,exact:true}).click();
      await expect(page.getByLabel('Panel name',{exact:true})).toHaveValue(previous.panelName);
      await expect(page.locator('.compiled-designer').getByRole('button',{name:'Try',exact:true})).toBeEnabled();
      assert.deepEqual(await retired(),{retiredEvidence:'SQLite acceptance'});
      console.log(JSON.stringify({phase:'verify',previousProcessId:previous.processId,processId:status.processId,revision:saved.revision,exactWorkspaceRetained:true,retiredIndexedDbIgnored:true}));
    }
  } finally {await browser.close();}
});
