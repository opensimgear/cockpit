// Actual desktop settings controls and native listener; no gateway mocks.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'node:net';
import {readFile,writeFile} from 'node:fs/promises';
import {resolve,dirname,join} from 'node:path';
async function freePort(){const server=createServer();await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});const port=server.address().port;await new Promise(resolve=>server.close(resolve));return port;}

test('listener settings use SQLite, survive process restart and ignore retired metadata',{
 skip:!process.env.OSG_SQLITE_NATIVE_ACCEPTANCE || !process.env.OSG_SQLITE_LISTENER_RESTART_PHASE,
 timeout:90_000,
},async()=>{
 const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';const {chromium}=require(module),{expect}=require(module+'/test');
 const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224');
 try {
  const currentPage=()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith(process.env.OSG_COCKPIT_DEV_URL??'http://127.0.0.1:1421/'));
  await expect.poll(()=>!!currentPage(),{timeout:15_000}).toBe(true);const page=currentPage();assert.ok(page);
  await page.evaluate(()=>history.replaceState(null,'','/'));await page.reload();
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const native=async(command,companion={})=>{const result=await invoke('gateway_request',{request:{command,companion,clientId:'sqlite-listener-acceptance'}});assert.equal(result.ok,true,result.error);return result.state.companions;};
  const status=await invoke('persistence_status'),directory=join(dirname(status.path),'companions');
  const evidence=resolve('.research/sqlite/listener-restart.json');
  const retired=['companion-listener.json','companion-firmware.json'];
  if(process.env.OSG_SQLITE_LISTENER_RESTART_PHASE==='prepare')for(const file of retired)await writeFile(join(directory,file),'retired invalid SQLite acceptance');
  await page.getByRole('button',{name:'Settings',exact:true}).click();await page.getByRole('tab',{name:'Companions',exact:true}).click();
  const settings=page.getByRole('region',{name:'Companion listener settings'}),toggle=settings.getByRole('switch',{name:'Enable companion listener',exact:true});await expect(toggle).toBeEnabled();
  if(process.env.OSG_SQLITE_LISTENER_RESTART_PHASE==='prepare'){
   if(await toggle.getAttribute('aria-checked')==='true'){await toggle.click();await expect(toggle).toHaveAttribute('aria-checked','false');}
   const port=await freePort();await settings.getByRole('combobox').selectOption('127.0.0.1');await settings.getByRole('spinbutton').fill(String(port));
   const discovery=settings.getByRole('switch',{name:'LAN discovery',exact:true});if(await discovery.getAttribute('aria-checked')==='true')await discovery.click();
   await toggle.click();await expect(toggle).toHaveAttribute('aria-checked','true');await expect.poll(async()=>(await native('companion_status')).listener.enabled).toBe(true);
   const saved=(await native('companion_status')).listener;assert.equal(saved.config.bind,'127.0.0.1:'+port);assert.equal(saved.config.discovery,false);
   await writeFile(evidence,JSON.stringify({processId:status.processId,listener:saved},null,2));console.log(JSON.stringify({phase:'prepare',processId:status.processId,revision:saved.settingsRevision,bind:saved.config.bind}));
  }else{
   const previous=JSON.parse(await readFile(evidence,'utf8'));assert.notEqual(status.processId,previous.processId);
   await expect(toggle).toHaveAttribute('aria-checked','true');const saved=(await native('companion_status')).listener;assert.deepEqual(saved.config,previous.listener.config);assert.equal(saved.settingsRevision,previous.listener.settingsRevision);assert.equal(saved.enabled,true);
   const stale=await invoke('gateway_request',{request:{command:'companion_disable',clientId:'sqlite-listener-acceptance',companion:{expectedSettingsRevision:saved.settingsRevision-1}}});assert.equal(stale.ok,false);assert.match(stale.error,/revision conflict/i);assert.equal((await native('companion_status')).listener.enabled,true);
   await toggle.click();await expect(toggle).toHaveAttribute('aria-checked','false');const disabled=(await native('companion_status')).listener;assert.equal(disabled.enabled,false);assert.equal(disabled.configuredEnabled,false);assert.equal(disabled.settingsRevision,saved.settingsRevision+1);
   console.log(JSON.stringify({phase:'verify',previousProcessId:previous.processId,processId:status.processId,revision:disabled.settingsRevision,staleDisableRejected:true,disabledThroughUi:true}));
  }
  for(const file of retired)assert.equal(await readFile(join(directory,file),'utf8'),'retired invalid SQLite acceptance');
 }finally{await browser.close();}
});
