// A separate browser renders the running cockpit's actual editor. Every native
// request is forwarded to the existing app; no transport response is mocked.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir} from 'node:fs/promises';
import {resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(module),{expect}=require(module+'/test');
const origin=process.env.OSG_COCKPIT_UI_URL??'http://127.0.0.1:1420';

test('separate default Keyboard and Mouse blocks use the running native cockpit editor',{timeout:120_000},async t=>{
 const nativeBrowser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP??'http://127.0.0.1:9222');
 const nativePage=nativeBrowser.contexts().flatMap(context=>context.pages()).find(page=>page.url().startsWith(origin));
 assert.ok(nativePage,'Start the native development cockpit.');
 const invoke=(command,args)=>nativePage.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
 const request=async value=>{const reply=await invoke('gateway_request',{request:{...value,clientId:'desktop-blocks-native-check'}});assert.equal(reply.ok,true,reply.error);return reply.state;};
 const before=(await request({command:'profile_status'})).profiles.profileSession;
 const browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})});
 const page=await browser.newPage({viewport:{width:1500,height:1050}}),errors=[];
 page.on('pageerror',error=>errors.push(error.message));
 const name=`Desktop blocks ${randomUUID()}`,artifacts=resolve('.research/desktop-blocks-native');
 let saved;
 try{
  await mkdir(artifacts,{recursive:true});
  const compiled=await(await fetch(origin+'/src/lib/ProfilesPanel.svelte')).text();
  const runtime=compiled.match(/from ["']([^"']*\/svelte\.js[^"']*)["']/)?.[1];
  assert.ok(runtime,'Use the running Svelte development runtime.');
  await page.exposeFunction('__desktopNativeInvoke',invoke);
  await page.addInitScript(()=>{window.isTauri=true;window.__TAURI_INTERNALS__={invoke:(command,args)=>window.__desktopNativeInvoke(command,args)};});
  const path='/__desktop_blocks_native_check';
  await page.route(origin+path,route=>route.fulfill({contentType:'text/html',body:`<html><body><div class="cockpit"><main style="height:100dvh;padding:24px"><div id="test"></div></main></div><script type="module">import {mount} from '${runtime}';import Profiles from '/src/lib/ProfilesPanel.svelte';import '/src/app.css';mount(Profiles,{target:document.querySelector('#test')});</script></body></html>`}));
  await page.goto(origin+path);
  await page.getByRole('button',{name:'Add',exact:true}).click();
  if(!await page.getByLabel('Profile name',{exact:true}).isVisible())await page.getByRole('button',{name:'Edit profile name',exact:true}).click();
  await page.getByLabel('Profile name',{exact:true}).fill(name);
  await page.getByLabel('Profile name',{exact:true}).press('Enter');
  await page.locator('details[aria-label="Output components"] > summary').click();
  await page.getByRole('button',{name:'Add keyboard',exact:true}).click();
  await page.getByRole('button',{name:'Add mouse',exact:true}).click();
  await expect(page.locator('.resource-block.desktop')).toHaveCount(2);
  await expect(page.getByRole('button',{name:'Add keyboard',exact:true})).toBeDisabled();
  await expect(page.getByRole('button',{name:'Add mouse',exact:true})).toBeDisabled();
  await page.locator('.resource-block.desktop').filter({hasText:'Keyboard'}).locator('.block-heading').click();
  const keyboard=page.getByRole('region',{name:'Keyboard editor',exact:true});
  await expect(keyboard.getByRole('button',{name:/^Configure /})).toHaveCount(105);
  await keyboard.getByRole('button',{name:'Configure Space',exact:true}).click();
  await expect(keyboard.getByLabel('Behavior',{exact:true})).toHaveCount(0);
  await page.screenshot({path:resolve(artifacts,'keyboard.png')});
  await page.getByRole('application',{name:'Profile canvas',exact:true}).click({position:{x:12,y:12}});
  await expect(page.locator('.canvas-inspector')).toHaveCount(0);
  await page.locator('.resource-block.desktop').filter({hasText:'Mouse'}).locator('.block-heading').click();
  const mouse=page.getByRole('region',{name:'Mouse editor',exact:true});
  await expect(mouse.getByRole('button',{name:/^Configure /})).toHaveCount(11);
  await mouse.getByLabel('Mouse scope',{exact:true}).selectOption('desktop-wide');
  await mouse.getByRole('button',{name:'Configure Pointer X',exact:true}).click();
  await mouse.getByText('Speed (px/s)',{exact:true}).locator('input').fill('1500');
  await mouse.getByText('Speed (px/s)',{exact:true}).locator('input').press('Tab');
  await page.screenshot({path:resolve(artifacts,'mouse.png')});
  if(process.env.OSG_DESKTOP_UI_ONLY==='1'){
   await t.test('native persistence requires the rebuilt cockpit binary',child=>child.skip('UI-only check requested; rebuild/restart the native host before checking split-block persistence.'));
   assert.deepEqual((await request({command:'profile_status'})).profiles.profileSession,before);assert.deepEqual(errors,[]);return;
  }
  const save=page.locator('.editor-save-actions').getByRole('button',{name:'Save',exact:true});
  await save.click();await expect(save).toBeDisabled().catch(async error=>{throw Error(`${error.message}\nNative save error: ${await page.locator('.workspace-error').allTextContents()}`);});
  const list=(await request({command:'profile_list'})).profiles.profiles;
  saved=list.find(profile=>profile.name===name);assert.ok(saved,'The actual native service saved the split blocks.');
  const document=(await request({command:'profile_assignments_get',id:saved.id})).profiles.document;
  const keys=document.desktopOutputs.find(output=>output.name==='Keyboard'),pointer=document.desktopOutputs.find(output=>output.name==='Mouse');
  assert.equal(keys.ports.length,105);assert.equal(pointer.ports.length,11);assert.ok(document.desktopOutputs.every(output=>output.scope.kind==='desktop-wide'));
  assert.deepEqual(keys.actions.find(item=>item.portId==='key-44').action,{kind:'key-hold',key:44});
  assert.equal(pointer.actions.find(item=>item.portId==='pointer-x').action.speed,1500);
  const source={id:'neutral',name:'Neutral',operation:{kind:'constant',value:false},inputs:[],outputs:[{id:'out',name:'Button',direction:'output',contract:keys.ports.find(port=>port.id==='key-4').contract}]};
  const connected={...document,processingNodes:[source],bindings:[{id:'key-route',from:{nodeId:'neutral',portId:'out'},to:{nodeId:keys.id,portId:'key-4'}},{id:'mouse-route',from:{nodeId:'neutral',portId:'out'},to:{nodeId:pointer.id,portId:'button-left'}}]};
  const preview=(await request({command:'profile_compile',document:connected,assignments:[]})).profileCompilation;
  assert.notEqual(preview.readiness,'invalid',JSON.stringify(preview));
  assert.equal(preview.sectionTotals.sinks,2);
  assert.deepEqual((await request({command:'profile_status'})).profiles.profileSession,before,'Testing must preserve the main cockpit runtime.');
  assert.deepEqual(errors,[]);
 }finally{
  if(!saved){const list=(await request({command:'profile_list'})).profiles.profiles;saved=list.find(profile=>profile.name===name);}
  if(saved)await request({command:'profile_delete',id:saved.id,expectedRevision:saved.revision});
  await browser.close();await nativeBrowser.close();
 }
});
