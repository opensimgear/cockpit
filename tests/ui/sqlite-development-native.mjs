// First-run dev seeds in the real Cockpit, then restart without reseeding edits.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile,writeFile,mkdir,readdir} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
const execute=promisify(execFile),phase=process.env.OSG_SQLITE_DEVELOPMENT_PHASE??'prepare';
const artifacts=resolve('.research/sqlite/development-native');
test('fresh dev initialization seeds SQL once and restart preserves edits',{skip:process.env.OSG_SQLITE_DEVELOPMENT_ACCEPTANCE!=='1',timeout:45000},async()=>{
  const require=createRequire(import.meta.url),{chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224');
  const origin=process.env.OSG_NATIVE_DEV_ORIGIN??'http://127.0.0.1:1421';
  const page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith(origin+'/')&&!p.url().includes('profile-activation-window'));
  assert.ok(page);const configRoot=dirname(resolve(process.env.OSG_DCS_BIOS_CONFIG));
  const native=async request=>{const reply=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'sqlite-development-native'}}),request);assert.equal(reply.ok,true,reply.error);return reply.state;};
  const records=async()=>{const script="import sqlite3,json,sys,pathlib; c=sqlite3.connect(pathlib.Path(sys.argv[1]).resolve().as_uri()+'?mode=ro',uri=True); print(json.dumps([{'domain':r[0],'key':r[1],'revision':r[2]} for r in c.execute('SELECT domain,key,revision FROM records WHERE payload IS NOT NULL ORDER BY domain,key')]))";const {stdout}=await execute(process.env.OSG_TEST_PYTHON??'python',['-c',script,resolve(configRoot,'cockpit.sqlite3')],{windowsHide:true});return JSON.parse(stdout);};
  await mkdir(artifacts,{recursive:true});
  try{
    const list=(await native({command:'profile_list'})).profiles.profiles;
    assert.equal(list.length,3);const demo=list.find(p=>p.id==='00000000-0000-4000-8000-000000000100');assert.ok(demo);
    const runtime=(await native({command:'profile_status'})).profiles.profileSession;assert.equal(runtime.state,'stopped');assert.equal(runtime.owner,null);
    const rows=await records();assert.equal(rows.find(r=>r.domain==='development'&&r.key==='initialized').revision,1);
    assert.ok(rows.some(r=>r.domain==='widgets'&&r.key==='library'));
    assert.ok(!rows.some(r=>['secrets','security','recovery-profile','recovery-output','recovery-desktop','recovery-visibility'].includes(r.domain)));
    assert.ok((await readdir(configRoot)).every(name=>!name.endsWith('.json')));
    const widgets=(await native({command:'widgets_list',offset:0,limit:32})).widgetLibrary;assert.ok(widgets.widgets.some(w=>w.id==='toggle'));
    if(phase==='prepare'){
      assert.equal(demo.name,'Dev · Signals');assert.equal(demo.revision,1);
      const open=page.getByRole('button',{name:'Open profiles',exact:true});
      if(await open.count())await open.click();else if(await page.getByRole('button',{name:'← Back to Profiles',exact:true}).count())await page.getByRole('button',{name:'← Back to Profiles',exact:true}).click();
      await page.getByRole('button',{name:'Edit Dev · Signals',exact:true}).click();
      await page.getByText('Demo controls',{exact:true}).first().waitFor();
      await page.getByText('Demo panel',{exact:true}).first().waitFor();
      await page.screenshot({path:resolve(artifacts,'seeded-signals.png')});
      await native({command:'profile_rename',id:demo.id,expectedRevision:demo.revision,name:'Dev · Signals edited'});
      await writeFile(resolve(artifacts,'prepared.json'),JSON.stringify({rows,demo,widgetCount:widgets.total},null,2));
    }else{
      assert.equal(phase,'verify');const prepared=JSON.parse(await readFile(resolve(artifacts,'prepared.json'),'utf8'));
      assert.equal(demo.name,'Dev · Signals edited');assert.equal(demo.revision,2);
      assert.equal(rows.find(r=>r.domain==='widgets'&&r.key==='library').revision,prepared.rows.find(r=>r.domain==='widgets'&&r.key==='library').revision);
      await native({command:'profile_rename',id:demo.id,expectedRevision:demo.revision,name:'Dev · Signals'});
      await page.getByRole('button',{name:'Open profiles',exact:true}).click();
      await page.getByRole('button',{name:'Edit Dev · Signals',exact:true}).waitFor();
      await page.screenshot({path:resolve(artifacts,'seeded-restarted.png')});
      await writeFile(resolve(artifacts,'verified.json'),JSON.stringify({rows,demo,profileCount:list.length,widgetCount:widgets.total,runtime,verifiedAt:new Date().toISOString()},null,2));
    }
  }finally{await browser.close();}
});
