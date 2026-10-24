// Current SQL DCS setup/removal through the running native Cockpit; private fixtures only.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,readFile,writeFile,access} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
const execute=promisify(execFile), wait=ms=>new Promise(r=>setTimeout(r,ms));
test('native DCS setup and removal retain only SQL receipts',{skip:process.env.OSG_SQLITE_DCS_ACCEPTANCE!=='1',timeout:60000},async()=>{
 const require=createRequire(import.meta.url),{chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');
 const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224');
 const page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith(process.env.OSG_NATIVE_DEV_ORIGIN??'http://127.0.0.1:1421/')&&!p.url().includes('activation'));
 assert.ok(page);
 const native=async request=>{const r=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'sqlite-dcs-receipts'}}),request);assert.equal(r.ok,true,r.error);return r.state;};
 const directory=resolve('.research/sqlite/dcs-native',String(Date.now())),userFolder=resolve(directory,'user'),executable=resolve(directory,'install/bin/DCS.exe');
 await mkdir(dirname(executable),{recursive:true});await mkdir(resolve(userFolder,'Scripts'),{recursive:true});await writeFile(executable,'inert fixture, never launched');
 const before=Buffer.from('-- private original export\r\n');await writeFile(resolve(userFolder,'Scripts/Export.lua'),before);
 await execute(process.env.OSG_TEST_PYTHON??'python',['-c','import zipfile,sys; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])',process.env.OSG_DCS_SETUP_ARCHIVE,resolve(userFolder,'Scripts')],{windowsHide:true});
 await writeFile(resolve(userFolder,'Scripts/Export.lua'),before);
 const sql=async()=>{const {stdout}=await execute(process.env.OSG_TEST_PYTHON??'python',['-c',"import sqlite3,json,sys,pathlib; c=sqlite3.connect(pathlib.Path(sys.argv[1]).as_uri()+'?mode=ro',uri=True); print(json.dumps([{'key':r[0],'revision':r[1],'value':json.loads(r[2])} for r in c.execute(\"SELECT key,revision,payload FROM records WHERE domain='recovery-dcs' AND payload IS NOT NULL\")]))",resolve(dirname(process.env.OSG_DCS_BIOS_CONFIG),'cockpit.sqlite3')],{windowsHide:true});return JSON.parse(stdout);};
 try{
  const initial=(await native({command:'game_get_state'})).games;const previous=initial.catalogue.find(g=>g.id==='dcs')?.executable;
  assert.equal((await native({command:'profile_status'})).profiles.profileSession.state,'stopped');
  await native({command:'game_installation',gameId:'dcs',executable});
  await native({command:'game_integration_setup',gameId:'dcs',userFolder});
  let status;for(let i=0;i<150;i++){status=(await native({command:'game_get_state'})).games.catalogue.find(g=>g.id==='dcs').integrationSetup;if(status.state!=='setting-up')break;await wait(100);}
  assert.equal(status.state,'ready',JSON.stringify(status));assert.equal(status.removable,true);
  const installed=await sql(),row=installed[0];assert.ok(row);assert.equal(row.value.pending,null);assert.equal(row.value.receipts.at(-1).state,'completed');
  const work=resolve(dirname(process.env.OSG_DCS_BIOS_CONFIG),'games.integration-setup.setup-work');
  await assert.rejects(access(resolve(work,'dcs-setup-journal.json')));
  await assert.rejects(access(resolve(work,row.value.receipts.at(-1).record.id,'completed.json')));
  await native({command:'game_integration_remove',gameId:'dcs',userFolder});
  for(let i=0;i<150;i++){status=(await native({command:'game_get_state'})).games.catalogue.find(g=>g.id==='dcs').integrationSetup;if(status.state!=='setting-up'&&status.state!=='removing')break;await wait(100);}
  assert.deepEqual(await readFile(resolve(userFolder,'Scripts/Export.lua')),before);
  const removed=await sql();assert.equal(removed[0].value.pending,null);assert.equal(removed[0].value.receipts.at(-1).state,'restored');
  await assert.rejects(access(resolve(userFolder,'Scripts/OpenSimGear/DCSPhysics.lua')));
  assert.ok((await access(resolve(userFolder,'Scripts/DCS-BIOS/BIOS.lua')).then(()=>true)));
  await native({command:'game_installation',gameId:'dcs',executable:previous??null});
  await writeFile(resolve(directory,'verified.json'),JSON.stringify({installed,removed,status,userFolder,actualNative:true,noPhysicalOutputs:true},null,2));
 }finally{await browser.close();}
});
