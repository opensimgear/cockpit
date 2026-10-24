import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile,mkdir} from 'node:fs/promises';
import {resolve} from 'node:path';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');

test('recovered DCS demo persists intact and opens both aircraft tabs in native Cockpit',async()=>{
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  try {
    const page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith('http://127.0.0.1:1420/'));
    assert.ok(page,'The native dev Cockpit must be running');
    const native=async request=>{const r=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'dcs-demo-native-check'}}),request);assert.equal(r.ok,true,r.error);return r.state.profiles;};
    const expected=JSON.parse(await readFile(new URL('../../examples/dcs-bios/feature-lab.profile.json',import.meta.url),'utf8'));
    const list=await native({command:'profile_list'});
    const summary=list.profiles.find(p=>p.name===expected.name);assert.ok(summary);
    const saved=await native({command:'profile_get',id:summary.id});
    // Existing databases restore via import, which allocates a fresh identity.
    assert.equal(saved.document.id,summary.id);
    assert.equal(saved.document.revision,summary.revision);
    assert.deepEqual({...saved.document,id:expected.id,revision:expected.revision},expected);
    assert.deepEqual((await native({command:'profile_assignments_get',id:summary.id})).assignments,[]);
    assert.equal(list.profileSession.state,'stopped');
    await page.reload();
    const heading=page.getByRole('heading',{name:expected.name,exact:true});
    const edit=page.getByRole('button',{name:`Edit ${expected.name}`,exact:true});
    await page.locator('main').waitFor();
    if(!await heading.count())await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Profiles',exact:true}).click();
    await heading.or(edit).waitFor();
    if(await edit.count())await edit.click();
    await page.getByRole('button',{name:'Save profile',exact:true}).waitFor();
    await page.getByText('VKB right - cyclic',{exact:true}).first().waitFor();
    await mkdir(resolve('.research/dcs-demo-native'),{recursive:true});
    await page.screenshot({path:resolve('.research/dcs-demo-native/base.png')});
    for (const [tab,label] of [['AH-6J','AH-6J - panel lights and telemetry'],['F/A-18C','F/A-18C - panel lights and telemetry']]) {
      await page.getByRole('group',{name:'Profile vehicle tabs'}).getByRole('button',{name:tab,exact:true}).click();
      await page.getByText(label,{exact:true}).first().waitFor();
    }
    await page.getByRole('group',{name:'Profile vehicle tabs'}).getByRole('button',{name:'Base',exact:true}).click();
  }finally{await browser.close();}
});
