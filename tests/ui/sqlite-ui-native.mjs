import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(operation,message,timeout=20000){let last;const end=Date.now()+timeout;while(Date.now()<end){try{const value=await operation();if(value)return value;}catch(error){last=error;}await pause(75);}throw Error(`${message}${last?`: ${last.message}`:''}`);}
const evidencePath=resolve('.research/sqlite/ui-settings-restart.json');
test('actual Cockpit SQLite UI preferences, Forget, tray and geometry survive restart',{skip:process.env.OSG_SQLITE_UI_ACCEPTANCE!=='1',timeout:180000},async()=>{
 const origin=process.env.OSG_NATIVE_DEV_ORIGIN??'http://127.0.0.1:1421';const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224');
 let page=browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().startsWith(`${origin}/`)&&!page.url().includes('profile-activation-window'));assert.ok(page);
 const invoke=(command,args={})=>page.evaluate(([command,args])=>window.__TAURI_INTERNALS__.invoke(command,args),[command,args]);
 const native=async request=>{const reply=await invoke('gateway_request',{request:{...request,clientId:'sqlite-ui-native-acceptance'}});assert.equal(reply.ok,true,reply.error);return reply.state;};
 const read=()=>invoke('persistence_ui_read');const nav=name=>page.locator('.sidebar').getByRole('button',{name:new RegExp(`^${name}`)}).click();
 const stored=async(key,predicate)=>until(async()=>{const entry=(await read()).entries[key];return predicate(entry.value)&&entry;},`${key} was not durably acknowledged`);
 let handle;
 const windowAction=async(action,pid)=>{
  const args=['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',resolve('tests/ui/ui-window-native.ps1'),'-CockpitPid',String(pid),'-Executable',resolve('src-tauri/target/debug/osg-cockpit.exe'),'-Action',action];if(handle)args.push('-WindowHandle',handle);
  const result=await promisify(execFile)('powershell.exe',args,{timeout:15000});const value=JSON.parse(result.stdout.trim());handle=value.handle;return value;
 };
 try{
  await page.waitForSelector('nav[aria-label="Main navigation"]');const status=await invoke('persistence_status');assert.ok(status.path.replaceAll('\\','/').includes('/s7-ui/'),'Use the isolated S7 acceptance namespace');
  if(process.env.OSG_SQLITE_UI_PHASE==='verify'){
   const before=JSON.parse(await readFile(evidencePath,'utf8'));assert.notEqual(status.processId,before.processId);const snapshot=await read();
   for(const key of ['theme','devices-mode','canvas-navigation','canvas-styling','tray','progress'])assert.deepEqual(snapshot.entries[key].value,before.entries[key].value,key);
   assert.ok(snapshot.entries.remembered.value.forgotten[before.forgottenId]);assert.ok(!snapshot.entries.remembered.value.components.some(c=>c.id===before.forgottenId));
   const geometry=(await native({command:'ui_preferences_get',key:'windows'})).uiPreferences;assert.deepEqual(geometry.value,before.geometry.value);
   const actual=await windowAction('inspect',status.processId);assert.equal(actual.x,geometry.value.x);assert.equal(actual.y,geometry.value.y);const viewport=await page.evaluate(()=>({width:innerWidth,height:innerHeight,scale:devicePixelRatio}));assert.ok(Math.abs(geometry.value.width-Math.round(viewport.width*viewport.scale))<=2);assert.ok(Math.abs(geometry.value.height-Math.round(viewport.height*viewport.scale))<=2);
   assert.equal(await page.locator('.cockpit').evaluate(node=>node.classList.contains('light')),true);
   await nav('Devices');await page.getByRole('button',{name:'Topology',exact:true}).waitFor();assert.equal(await page.getByRole('button',{name:'Topology',exact:true}).getAttribute('aria-pressed'),'true');
   const after={previousProcessId:before.processId,processId:status.processId,allSettingsRestored:true,forgetTombstoneRetained:true,geometryRestored:true,noBrowserStorageAuthority:true};await writeFile(resolve('.research/sqlite/ui-settings-verified.json'),JSON.stringify(after,null,2));console.log(JSON.stringify(after));return;
  }
  await page.addInitScript(()=>{Object.defineProperty(window,'localStorage',{get(){throw Error('Retired browser settings are unavailable.');}});});await page.reload();await page.waitForSelector('nav[aria-label="Main navigation"]');
  await nav('Settings');await page.getByRole('tab',{name:'Styling',exact:true}).click();const light=page.getByRole('switch',{name:'Light theme',exact:true});if(await light.getAttribute('aria-checked')!=='true')await light.click();await stored('theme',value=>value===true);
  await page.getByRole('tab',{name:'General',exact:true}).click();
  await page.getByLabel('Canvas mouse wheel',{exact:true}).selectOption('zoom');await stored('canvas-navigation',value=>value.wheelAction==='zoom');
  await page.getByRole('tab',{name:'Styling',exact:true}).click();await page.getByLabel('Arrow style',{exact:true}).selectOption('orthogonal');await stored('canvas-styling',value=>value.arrowStyle==='orthogonal');
  const radius=page.getByLabel('Arrow corner radius',{exact:true});await radius.fill('17');await radius.press('Tab');await stored('canvas-styling',value=>value.cornerRadius===17);
  await stored('canvas-styling',value=>Object.keys(value).sort().join(',')==='arrowStyle,cornerRadius');
  await page.getByRole('tab',{name:'General',exact:true}).click();await page.getByLabel('Progress notification style',{exact:true}).selectOption('window');await stored('progress',value=>value.delivery==='window');await page.getByLabel('Progress notification position',{exact:true}).selectOption('top-left');await stored('progress',value=>value.position==='top-left');
  const desktop=await invoke('desktop_preferences');assert.equal(desktop.trayAvailable,true);
  for(const name of ['Close to tray','Minimize to tray']){const button=page.getByRole('switch',{name,exact:true});if(await button.getAttribute('aria-checked')!=='true')await button.click();await stored('tray',value=>value[name==='Close to tray'?'closeToTray':'minimizeToTray']===true);}
  // A fixture represents only remembered metadata; it has no device target or
  // native controller source and cannot touch hardware during the portal Forget.
  const forgottenId=`sqlite-offline-${Date.now()}`,name='SQLite remembered controller '+forgottenId.slice(-6);const memory=(await read()).entries.remembered;
  const component={id:forgottenId,name,type:'controller',role:'Test history',presence:'offline',hidden:false,enabled:null,owner:null,capture:'unknown',status:'Remembered offline',diagnostics:[],connections:[],targets:[],evidence:['sqlite-acceptance'],references:[{kind:'history',id:'unresolved-example',name:'Stored history'}],firstSeen:10,lastSeen:20,observedAt:30,provider:'controllers',stale:true,forget:{allowed:false,reason:'Refresh first',effect:'Local metadata'}};
  await native({command:'ui_preferences_save',key:'remembered',expectedRevision:memory.revision,value:{components:[...(memory.value?.components??[]).filter(c=>!c.id.startsWith('sqlite-offline-')),component],forgotten:memory.value?.forgotten??{}}});
  await nav('Devices');await until(()=>page.getByText(name,{exact:true}).count(),'The remembered fixture did not appear');const topology=page.getByRole('button',{name:'Topology',exact:true});await topology.click();await stored('devices-mode',value=>value==='topology');
  await page.getByRole('button',{name:/^Cleanup \(/}).click();const dialog=page.getByRole('dialog');await dialog.getByRole('checkbox',{name:'Select '+name,exact:true}).check();await dialog.getByRole('button',{name:'Confirm',exact:true}).click();await stored('remembered',value=>!!value.forgotten[forgottenId]&&!value.components.some(c=>c.id===forgottenId));
  await windowAction('resize',status.processId);const geometry=await until(async()=>{const row=(await native({command:'ui_preferences_get',key:'windows'})).uiPreferences;return row.value?.x===80&&row.value?.y===70&&row;},'Real window resize was not retained');
  const viewport=await page.evaluate(()=>({width:innerWidth,height:innerHeight,scale:devicePixelRatio}));assert.ok(Math.abs(geometry.value.width-Math.round(viewport.width*viewport.scale))<=2);assert.ok(Math.abs(geometry.value.height-Math.round(viewport.height*viewport.scale))<=2);
  await windowAction('close',status.processId);await until(async()=>!(await windowAction('inspect',status.processId)).visible,'Close-to-tray did not hide the actual window');await windowAction('show',status.processId);await until(async()=>(await windowAction('inspect',status.processId)).visible,'Single-instance relaunch did not reopen the tray window');
  await windowAction('minimize',status.processId);await until(async()=>!(await windowAction('inspect',status.processId)).visible,'Minimize-to-tray did not hide the actual window');await windowAction('show',status.processId);await until(async()=>(await windowAction('inspect',status.processId)).visible,'The minimized tray window did not reopen');
  await page.waitForTimeout(500);const entries=(await read()).entries;const finalGeometry=(await native({command:'ui_preferences_get',key:'windows'})).uiPreferences;
  await mkdir(resolve('.research/sqlite'),{recursive:true});await writeFile(evidencePath,JSON.stringify({processId:status.processId,database:status.path,entries,geometry:finalGeometry,forgottenId,checks:{blockedLocalStorage:true,actualUiControls:true,durableForget:true,realWindowGeometry:true,closeToTray:true,minimizeToTray:true}},null,2));await page.screenshot({path:resolve('.research/sqlite/ui-settings-native.png')});console.log(JSON.stringify({processId:status.processId,prepared:true,forgetTombstone:true,actualTrayBehavior:true,geometry:finalGeometry.value}));
 }finally{await browser.close();}
});
