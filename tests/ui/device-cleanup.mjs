// Exercise the actual Devices component through the running Vite cockpit.
// Deterministic IPC replies qualify UI behavior, not native device removal.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url), module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(module),{expect}=require(module+'/test');
const origin=process.env.OSG_DEVICES_UI_URL??'http://127.0.0.1:1420';
const deferred=()=>{let resolve;const promise=new Promise(r=>resolve=r);return {promise,resolve};};

test('cleanup removes completed rows, cancels the queue, and protects profile devices', {timeout:60000}, async()=>{
 const browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})});
 const page=await browser.newPage();
 const compiled=await (await fetch(origin+'/src/lib/SystemDevices.svelte')).text();
 const runtime=compiled.match(/from ["']([^"']*\/svelte\.js[^"']*)["']/)[1];
 page.on('pageerror',error=>console.error(error.message));
 const removed=[], first=deferred(), next=deferred();let pauseNext=false;
 const sources=['First','Second','Third','Protected'].map(id=>({id,name:id,online:false,role:'controller',backend:'windows-raw-input',controlCount:0,baselineReady:false}));
 await page.exposeFunction('__cleanupInvoke',async(command,args)=>{
  assert.equal(command,'gateway_request');const r=args.request;let state={};
  if(r.command==='controller_list')state={controllers:{session:'cleanup',offset:0,total:sources.length,sources}};
  else if(r.command==='module_usb_list')state={moduleUsb:{offset:0,total:0,modules:[]}};
  else if(r.command==='system_inventory')state={systemInventory:{observedAt:1,total:0,offset:0,hiddenDevices:[],virtualDevices:[],diagnostics:[],visibilityInspected:true}};
  else if(r.command==='companion_status')state={companions:{host:{clients:[],instances:[]},usb:{devices:[]}}};
  else if(r.command==='profile_status')state={profiles:{profileSession:{state:'stopped'}}};
  else if(r.command==='game_get_state')state={games:{catalogue:[]}};
  else if(r.command==='profile_resources_list')state={profileResources:{session:'cleanup',revision:'1',offset:0,total:0,resources:[]}};
  else if(r.command==='profile_list'){if(pauseNext)await next.promise;state={profiles:{profiles:[{id:'saved',name:'Flight'}]}};}
  else if(r.command==='profile_assignments_get')state={profiles:{assignments:[{sourceId:'Protected',resourceId:'stick',ports:[]}]}};
  else if(r.command==='controller_forget'){removed.push(r.sourceId);await first.promise;sources.splice(sources.findIndex(s=>s.id===r.sourceId),1);pauseNext=true;}
  else throw Error('Unexpected cleanup request '+r.command);
  return {ok:true,state};
 });
 await page.addInitScript(()=>{window.isTauri=true;window.__TAURI_INTERNALS__={invoke:(command,args)=>window.__cleanupInvoke(command,args)};});
 await page.route(origin+'/__cleanup_test',route=>route.fulfill({contentType:'text/html',body:`<html><body><div id="test"></div><script type="module">import {mount} from '${runtime}';import Devices from '/src/lib/SystemDevices.svelte';import '/src/app.css';mount(Devices,{target:document.querySelector('#test'),props:{active:true,entries:[],openModule(){},openCompanions(){},openIntegration(){},oninventory(){}}});</script></body></html>`}));
 try{
  await page.goto(origin+'/__cleanup_test');
  await page.getByRole('button',{name:'Cleanup (4)',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'Cleanup devices'});
  const protectedRow=dialog.getByRole('row').filter({has:page.getByRole('checkbox',{name:'Select Protected',exact:true})});
  await expect(protectedRow).toContainText('Used in profiles');
  await expect(protectedRow.getByRole('checkbox')).toBeDisabled();
  await dialog.getByRole('checkbox',{name:'Select all',exact:true}).check();
  await dialog.getByRole('button',{name:'Confirm',exact:true}).click();
  await expect(dialog.locator('.cleanup-summary .cleanup-spinner')).toBeVisible();
  await expect(dialog.getByRole('status',{name:'Cleaning up First',exact:true})).toBeVisible();
  await expect.poll(()=>removed.length).toBe(1);first.resolve();
  await expect(dialog.getByRole('checkbox',{name:'Select First',exact:true})).toHaveCount(0);
  await dialog.getByRole('button',{name:'Cancel',exact:true}).click();
  await expect(dialog).toContainText('Cancelling after the current device finishes');next.resolve();
  await expect(dialog.locator('.cleanup-spinner')).toHaveCount(0);
  assert.deepEqual(removed,['First']);
  await expect(dialog.getByRole('checkbox',{name:'Select Second',exact:true})).toBeVisible();
  await expect(dialog.getByRole('checkbox',{name:'Select Third',exact:true})).toBeVisible();
  await dialog.getByRole('button',{name:'Cancel',exact:true}).click();await expect(dialog).toHaveCount(0);
 }catch(error){console.error(await page.locator('body').innerText());throw error;}finally{first.resolve();next.resolve();await browser.close();}
});

