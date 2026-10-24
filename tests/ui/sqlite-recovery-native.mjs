// Actual native dev Cockpit: inert profile, forced process exit, explicit recovery.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {randomUUID} from 'node:crypto';
import {readFile,writeFile,mkdir,access} from 'node:fs/promises';
import {dirname,resolve} from 'node:path';
import {recoveryRecord} from './sqlite-recovery-record.mjs';
const enabled=process.env.OSG_SQLITE_RECOVERY_ACCEPTANCE==='1';
const phase=process.env.OSG_SQLITE_RECOVERY_PHASE??'prepare';
const artifacts=resolve('.research/sqlite/recovery-native');
test('SQLite profile recovery survives native process termination without replay',{skip:!enabled,timeout:45000},async()=>{
  assert.ok(['prepare','verify'].includes(phase));
  const require=createRequire(import.meta.url);
  const {chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224');
  const origin=process.env.OSG_NATIVE_DEV_ORIGIN??'http://127.0.0.1:1421';
  const page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith(origin+'/')&&!p.url().includes('profile-activation-window'));
  assert.ok(page,'The running dev Cockpit must be available.');
  const configRoot=dirname(resolve(process.env.OSG_DCS_BIOS_CONFIG));
  const database=resolve(configRoot,'cockpit.sqlite3');
  const native=async request=>{const response=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'sqlite-recovery-native'}}),request);assert.equal(response.ok,true,response.error);return response.state;};
  const status=async()=>(await native({command:'profile_status'})).profiles.profileSession;
  await mkdir(artifacts,{recursive:true});
  try{
    if(phase==='prepare'){
      assert.notEqual((await status()).state,'recovery-needed','Resolve previous acceptance evidence first.');
      await native({command:'profile_stop'});
      const retiredPath=resolve(configRoot,'profile-session-v1.json');
      const retired=await readFile(retiredPath,'utf8').catch(error=>{if(error.code==='ENOENT')return null;throw error;});
      const document=JSON.parse(await readFile(new URL('../fixtures/profiles/blank.json',import.meta.url),'utf8'));
      document.id=randomUUID();document.name='SQLite restart recovery acceptance';
      const saved=(await native({command:'profile_editor_save',document,assignments:[],expectedRevision:null})).profiles.document;
      await native({command:'profile_apply',id:saved.id,expectedRevision:saved.revision});
      const active=await status();assert.equal(active.owner.profileId,saved.id);assert.equal(active.state,'running');
      const record=await recoveryRecord(database,'recovery-profile','profile-session-v1.json');
      assert.equal(record.payload.active.owner.profileId,saved.id);assert.equal(record.payload.pending,null);
      assert.equal(record.payload.schemaVersion,undefined);assert.equal(record.payload.paused,undefined);
      if(retired===null)await assert.rejects(access(retiredPath));else assert.equal(await readFile(retiredPath,'utf8'),retired);
      await writeFile(resolve(artifacts,'prepared.json'),JSON.stringify({saved,record,active,retired,url:page.url()},null,2));
    }else{
      const prepared=JSON.parse(await readFile(resolve(artifacts,'prepared.json'),'utf8'));
      const retired=await readFile(resolve(configRoot,'profile-session-v1.json'),'utf8').catch(error=>{if(error.code==='ENOENT')return null;throw error;});
      assert.equal(retired,prepared.retired,'Retired file contents remain untouched.');
      const restarted=await status();assert.equal(restarted.state,'recovery-needed');assert.equal(restarted.owner,null);
      assert.deepEqual(await recoveryRecord(database,'recovery-profile','profile-session-v1.json'),prepared.record,'Startup must preserve evidence without replay.');
      await page.getByRole('button',{name:'Open profiles',exact:true}).click();
      const recover=page.getByRole('button',{name:'Recover',exact:true});
      await recover.waitFor({state:'visible'});
      await page.screenshot({path:resolve(artifacts,'recovery-required.png')});
      await recover.click();await recover.waitFor({state:'hidden'});
      const recovered=await status();assert.equal(recovered.state,'stopped');assert.equal(recovered.owner,null);
      await page.waitForFunction(()=>!document.querySelector('.profile-status')?.textContent?.includes('Recovery needed'));
      const record=await recoveryRecord(database,'recovery-profile','profile-session-v1.json');
      assert.ok(record.revision>prepared.record.revision);assert.equal(record.payload.active,null);assert.equal(record.payload.pending,null);
      await page.getByRole('button',{name:'Delete '+prepared.saved.name,exact:true}).click();
      await page.getByRole('alertdialog').getByRole('button',{name:'Delete saved profile',exact:true}).click();
      await page.getByRole('button',{name:'Edit '+prepared.saved.name,exact:true}).waitFor({state:'hidden'});
      await page.screenshot({path:resolve(artifacts,'recovered.png')});
      await writeFile(resolve(artifacts,'verified.json'),JSON.stringify({restarted,recovered,record,verifiedAt:new Date().toISOString()},null,2));
    }
  }finally{await browser.close();}
});
