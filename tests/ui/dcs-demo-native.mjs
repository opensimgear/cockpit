import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile,mkdir} from 'node:fs/promises';
import {resolve} from 'node:path';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');

test('DCS demo persists intact and opens every integration in the shared native canvas',async()=>{
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
    await page.getByRole('button',{name:'Save',exact:true}).waitFor();
    const canvas=page.getByRole('application',{name:'Profile canvas',exact:true});
    for(const resource of expected.resources){const block=canvas.locator('[data-node-id="'+resource.id+'"]');await block.waitFor();assert.equal(await block.count(),1);assert.ok((await block.textContent()).includes(resource.name));}
    for(const id of ['game-context-dcs-1','ah6-dcs','hornet-dcs'])assert.equal(await canvas.locator('[data-node-id="'+id+'"] [data-block-icon="link"]').count(),1);
    assert.equal(await page.getByRole('group',{name:'Profile vehicle tabs'}).count(),0);
    assert.equal(await page.getByLabel('Tab vehicle',{exact:true}).count(),0);
    await mkdir(resolve('.research/dcs-demo-native'),{recursive:true});
    await page.screenshot({path:resolve('.research/dcs-demo-native/shared-canvas.png')});
  }finally{await browser.close();}
});

test('development seed includes editable DCS panel, sources and working compiled preview',async()=>{
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  try {
    const page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith('http://127.0.0.1:1420/'));
    assert.ok(page,'The native dev Cockpit must be running');
    const expected=JSON.parse(await readFile(new URL('../../examples/dcs-bios/win-companion/workspace.json',import.meta.url),'utf8'));
    const saved=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('persistence_workspace',{request:{operation:'read'}}));
    assert.deepEqual(saved.value,expected);
    assert.equal(saved.revision,1,'Fresh development workspace must be seeded once');
    const widgets=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'widgets_list',offset:0,limit:32,clientId:'dcs-seed-native-check'}}));
    assert.equal(widgets.ok,true);
    assert.deepEqual(widgets.state.widgetLibrary.widgets.map(w=>w.id).sort(),expected.sources.map(s=>s.id).sort());
    await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Companions',exact:true}).click();
    const panel=expected.drafts[0];
    await page.getByRole('button',{name:`Edit ${panel.title}`,exact:true}).waitFor();
    for(const source of expected.sources)await page.getByRole('button',{name:`Edit design ${source.title}`,exact:true}).waitFor();
    await page.getByRole('button',{name:`Edit ${panel.title}`,exact:true}).click();
    await page.getByRole('heading',{name:panel.title,exact:true}).waitFor();
    await page.getByText('2400 × 1440',{exact:true}).waitFor();
    await page.getByRole('button',{name:'Try',exact:true}).click();
    await page.getByText('Compiled Try',{exact:true}).waitFor();
    await page.locator('.stage canvas').first().waitFor();
    await mkdir(resolve('.research/dcs-demo-native'),{recursive:true});
    await page.screenshot({path:resolve('.research/dcs-demo-native/seeded-panel.png')});
    await page.getByRole('button',{name:'Stop',exact:true}).click();
  }finally{await browser.close();}
});
