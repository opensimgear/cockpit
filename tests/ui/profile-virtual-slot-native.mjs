import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {randomUUID} from 'node:crypto';
import {loadModules} from '../../src/lib/test-module-loader.mjs';
const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(module),{expect}=require(module+'/test');
const origin=process.env.OSG_COCKPIT_UI_URL??'http://127.0.0.1:1420';
const frontend=process.env.OSG_SLOT_FRONTEND_ROOT??'';

test('a saved virtual block selects a local slot, adopts its layout and activates that exact slot',{timeout:120000},async()=>{
 const native=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP??'http://127.0.0.1:9222');
 const nativePage=native.contexts().flatMap(context=>context.pages()).find(page=>page.url().startsWith(origin));assert.ok(nativePage);
 let slotReads=0;
 const invoke=(command,args)=>{if(command==='gateway_request'&&args.request.command==='profile_virtual_slots_get')slotReads++;return nativePage.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});};
 const request=async value=>{const response=await invoke('gateway_request',{request:{...value,clientId:'virtual-slot-native-check'}});assert.equal(response.ok,true,response.error);return response.state;};
 const before=(await request({command:'profile_status'})).profiles.profileSession;
 assert.equal(before.owner,null,'Run this check while no user profile is active.');
 const inventory=(await request({command:'system_inventory'})).systemInventory;
 const target=inventory.virtualDevices.filter(slot=>slot.status==='free'&&slot.buttons>0&&Array.isArray(slot.axes)&&!slot.discreteHats&&!slot.forceFeedback&&slot.buttons+slot.axes.length+slot.continuousHats<=128).sort((a,b)=>Number(b.slot===2)-Number(a.slot===2))[0];
 assert.ok(target,'Configure an unused vJoy slot before running native slot selection verification.');
 const modules=await loadModules(['./profile-editor','./profile-graph']);
 const {blankProfile}=modules.get('./profile-editor'),{graphSetVirtual}=modules.get('./profile-graph');
 const name=`Virtual slot ${randomUUID()}`;
 let graph=graphSetVirtual({document:blankProfile(randomUUID(),name),assignments:[]},{id:'virtual-slot',name:'Slot example',required:true,layout:{buttons:['button-1'],axes:[],hats:[]}});
 let saved=(await request({command:'profile_editor_save',document:graph.document,assignments:[],expectedRevision:null})).profiles.document;
 let browser;const errors=[];
 try{
  browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})});
  const page=await browser.newPage({viewport:{width:1500,height:1050},reducedMotion:'reduce'});
  page.on('pageerror',error=>errors.push(error.message));
  const compiled=await(await fetch(origin+frontend+'/src/lib/ProfilesPanel.svelte')).text();
  const runtime=compiled.match(/from ["']([^"']*\/svelte\.js[^"']*)["']/)?.[1];assert.ok(runtime);
  await page.exposeFunction('__slotNativeInvoke',invoke);
  await page.addInitScript(()=>{window.isTauri=true;window.__TAURI_INTERNALS__={invoke:(command,args)=>window.__slotNativeInvoke(command,args)};});
  const path='/__virtual_slot_native';
  await page.route(origin+path,route=>route.fulfill({contentType:'text/html',body:`<html><body><div class="cockpit"><main style="height:100dvh;padding:24px"><div id="test"></div></main></div><script type="module">import {mount} from '${runtime}';import Profiles from '${frontend}/src/lib/ProfilesPanel.svelte';import '${frontend}/src/app.css';mount(Profiles,{target:document.querySelector('#test')});</script></body></html>`}));
  await page.goto(origin+path);
  async function open(){await page.getByRole('button',{name:`Edit ${name}`,exact:true}).click();await page.locator('.resource-block').filter({hasText:'Slot example'}).locator('.block-heading').click();await expect(page.getByLabel('Slot',{exact:true})).toBeEnabled();}
  await open();
  const picker=page.getByLabel('Slot',{exact:true}),save=page.locator('.editor-save-actions').getByRole('button',{name:'Save',exact:true});
  await expect(picker).toHaveValue('');await picker.selectOption(String(target.slot));
  await expect.poll(async()=>((await request({command:'profile_virtual_slots_get',id:saved.id})).profiles.virtualSlots[0].slot)).toBe(target.slot);
  await expect(save).toBeDisabled();
  const readsBeforeEdit=slotReads;
  await picker.focus();
  await page.waitForTimeout(6500); // Cover two normal inventory refresh intervals.
  assert.equal(slotReads,readsBeforeEdit,'Background inventory polling must not reload the slot setting.');
  await expect(picker).toBeFocused();
  await page.getByRole('button',{name:'Use layout',exact:true}).click();
  await expect(page.getByRole('spinbutton',{name:/^buttons$/i})).toHaveValue(String(target.buttons));
  await expect(page.getByRole('spinbutton',{name:/^axes$/i})).toHaveValue(String(target.axes.length));
  await expect(page.getByRole('spinbutton',{name:/^hats$/i})).toHaveValue(String(target.continuousHats));
  // Adopting the layout replaces this output without changing its saved identity.
  // It must not disable or reload the local slot setting.
  await page.waitForTimeout(200);
  assert.equal(slotReads,readsBeforeEdit,'Unrelated block edits must not refresh slot settings.');
  await expect(picker).toBeEnabled();await expect(picker).toHaveValue(String(target.slot));
  const readsBeforeSave=slotReads;
  await save.click();await expect(save).toBeDisabled();
  await expect.poll(()=>slotReads).toBe(readsBeforeSave+1);
  await expect(picker).toBeEnabled();
  const readsBeforeRefresh=slotReads;
  await page.getByRole('button',{name:'Refresh slots',exact:true}).click();
  await expect(picker).toBeEnabled();
  assert.equal(slotReads,readsBeforeRefresh+1,'Explicit Refresh must still fetch current slot settings.');
  saved=(await request({command:'profile_assignments_get',id:saved.id})).profiles.document;
  assert.equal(saved.virtualOutputs[0].layout.buttons[0],'button-1');
  const exported=(await request({command:'profile_export',id:saved.id})).profiles.document;
  assert.equal(exported.localVirtualSlots,undefined);assert.equal(exported.virtualOutputs[0].slot,undefined);
  await page.reload();await open();await expect(picker).toHaveValue(String(target.slot));
  const applied=(await request({command:'profile_apply',id:saved.id,expectedRevision:saved.revision,expectedActiveOwner:null})).profiles.profileSession;
  assert.ok(['running','degraded'].includes(applied.state),JSON.stringify(applied));
  await expect.poll(async()=>((await request({command:'system_inventory'})).systemInventory.virtualDevices.find(slot=>slot.slot===target.slot)?.status),{timeout:10000}).toBe('busy');
  assert.equal(applied.owner.profileId,saved.id);
  await request({command:'profile_stop'});
  await picker.selectOption('');
  await expect.poll(async()=>((await request({command:'profile_virtual_slots_get',id:saved.id})).profiles.virtualSlots[0].slot)).toBe(null);
  await expect(save).toBeDisabled();assert.deepEqual(errors,[]);
 }finally{
  const status=(await request({command:'profile_status'})).profiles.profileSession;
  if(status.owner?.profileId===saved.id)await request({command:'profile_stop'});
  const current=(await request({command:'profile_assignments_get',id:saved.id})).profiles.document;
  await request({command:'profile_delete',id:saved.id,expectedRevision:current.revision});
  await browser?.close();await native.close();
 }
});
