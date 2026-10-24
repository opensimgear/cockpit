// Actual portal -> SQLite backup -> stopped restore -> full native restart.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';
import {randomUUID} from 'node:crypto';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function until(read,label,timeout=30000){let last;const end=Date.now()+timeout;while(Date.now()<end){try{const value=await read();if(value)return value;}catch(error){last=error;}await pause(150);}throw Error(label+': '+String(last));}
async function connect(endpoint,token){const client=new Client({name:'SQLite backup acceptance',version:'1.0.0'},{capabilities:{}});await client.connect(new StreamableHTTPClientTransport(new URL(endpoint),{requestInit:{headers:{Authorization:'Bearer '+token}},reconnectionOptions:{maxRetries:0}}));return client;}
async function call(client,name,args={}){const reply=await client.callTool({name,arguments:args});return reply.structuredContent??JSON.parse(reply.content.find(c=>c.type==='text').text);}
test('running Cockpit backup and restart restore reject historical MCP credentials',{skip:process.env.OSG_SQLITE_BACKUP_ACCEPTANCE!=='1',timeout:120000},async()=>{
 const require=createRequire(import.meta.url),{chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');
 const artifacts=resolve('.research/sqlite/backup-native');await mkdir(artifacts,{recursive:true});
 let browser,page,writer,reader;const clients=[];
 const attach=async()=>{browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224');page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith(process.env.OSG_NATIVE_DEV_ORIGIN??'http://127.0.0.1:1421/')&&!p.url().includes('activation'));return !!page&&await page.evaluate(()=>!!window.__TAURI_INTERNALS__?.invoke);};
 const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
 const native=async request=>{const reply=await invoke('gateway_request',{request:{...request,clientId:'sqlite-backup-acceptance'}});assert.equal(reply.ok,true,reply.error);return reply.state;};
 try{
  await attach();const before=await invoke('persistence_status');assert.equal((await native({command:'profile_status'})).profiles.profileSession.state,'stopped');
  const original=(await native({command:'profile_list'})).profiles.profiles.find(p=>p.id==='00000000-0000-4000-8000-000000000100');assert.ok(original);
  let settings=await invoke('mcp_settings_get');if(settings.enabled)settings=await invoke('mcp_settings_update',{settings:{enabled:false,port:settings.port,expectedSettingsRevision:settings.settingsRevision}});for(const credential of [...settings.credentials]){settings=await invoke('mcp_credential_revoke',{id:credential.id,expectedSettingsRevision:settings.settingsRevision});}assert.equal(settings.enabled,false);
  const write=await invoke('mcp_credential_create',{name:'Backup acceptance writer',scope:'write',expectedSettingsRevision:settings.settingsRevision});settings=await invoke('mcp_settings_get');
  const read=await invoke('mcp_credential_create',{name:'Backup acceptance reader',scope:'read',expectedSettingsRevision:settings.settingsRevision});settings=await invoke('mcp_settings_get');
  settings=await invoke('mcp_settings_update',{settings:{enabled:true,port:Number(process.env.OSG_SQLITE_BACKUP_MCP_PORT??54447),expectedSettingsRevision:settings.settingsRevision}});
  await until(async()=> (await invoke('mcp_settings_get')).running,'MCP listener');writer=await connect(settings.endpoint,write.token);reader=await connect(settings.endpoint,read.token);clients.push(writer,reader);
  const tools=(await reader.listTools()).tools;assert.ok(tools.some(t=>t.name==='storage_status'));assert.ok(!tools.some(t=>t.name==='storage_restore'||t.name==='storage_create'||t.name==='storage_restart'));
  await assert.rejects(()=>call(reader,'storage_create',{expectedRevision:0,operationId:randomUUID()}),/does not permit changes/);
  let hostBefore=null,publication=null;
  if(process.env.OSG_SQLITE_BACKUP_PUBLICATION==='1'){
    const listener=await call(writer,'companion_listener_get');assert.equal(listener.ok,true);if(!listener.data.enabled){const enabled=await call(writer,'companion_listener_save',{enabled:true,expectedSettingsRevision:listener.data.settingsRevision,address:'127.0.0.1',port:54449,advertisedHost:'127.0.0.1',discovery:false,operationId:randomUUID()});assert.equal(enabled.ok,true,JSON.stringify(enabled));}
    const offer=await call(writer,'companion_pair_offer',{kind:'browser',operationId:randomUUID()});assert.equal(offer.ok,true,JSON.stringify(offer));const cancelled=await call(writer,'companion_pair_cancel',{operationId:randomUUID()});assert.equal(cancelled.ok,true);
    publication=JSON.parse(await readFile(new URL('../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json',import.meta.url),'utf8'));const bytes=Buffer.from(JSON.stringify(publication));
    const begin=await call(writer,'companion_publish_begin',{size:bytes.length,operationId:randomUUID()});assert.equal(begin.ok,true);const uploadId=begin.data.uploadId;
    for(let offset=0;offset<bytes.length;offset+=8192){const chunk=await call(writer,'companion_publish_chunk',{uploadId,offset,base64:bytes.subarray(offset,offset+8192).toString('base64'),operationId:randomUUID()});assert.equal(chunk.ok,true);}
    const state=await call(writer,'companion_inventory');assert.equal(state.ok,true);const committed=await call(writer,'companion_publish_commit',{uploadId,saveAsLatest:true,selection:{expectedHostToken:state.data.host.snapshotToken,companionIds:[]},operationId:randomUUID()});assert.equal(committed.ok,true,JSON.stringify(committed));hostBefore=committed.data.host;
  }
  await page.getByRole('button',{name:'Settings',exact:true}).click();const section=page.getByRole('region',{name:'Backups',exact:true});await section.getByRole('button',{name:'Create',exact:true}).click();await section.getByText('Backup saved and verified.',{exact:true}).waitFor();
  const captured=(await native({command:'storage_status'})).storage;assert.equal(captured.backups.length,1);const id=captured.backups[0].id;
  const mcp=await call(writer,'storage_status');assert.equal(mcp.ok,true);assert.equal(mcp.data.revision,captured.revision);
  const extra=await call(writer,'storage_create',{expectedRevision:captured.revision,operationId:randomUUID()});assert.equal(extra.ok,true,JSON.stringify(extra));
  const removed=await call(writer,'storage_remove',{id:extra.data.backup.id,expectedRevision:extra.data.status.revision,operationId:randomUUID()});assert.equal(removed.ok,true);assert.equal(removed.data.filesRemoved,true);
  const stale=await call(writer,'storage_create',{expectedRevision:captured.revision,operationId:randomUUID()});assert.equal(stale.ok,false);
  await native({command:'profile_rename',id:original.id,expectedRevision:original.revision,name:'After the saved backup'});
  await section.getByRole('button',{name:'Refresh',exact:true}).click();await section.locator('[data-backup-id="'+id+'"]').getByRole('button',{name:'Restore',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'Restore saved settings?',exact:true});await dialog.waitFor();await page.screenshot({path:resolve(artifacts,'restore-review.png')});await dialog.getByRole('button',{name:'Restore',exact:true}).click();await section.getByText('Restore ready',{exact:true}).waitFor();
  assert.equal((await native({command:'storage_status'})).storage.restartRequired,true);await page.screenshot({path:resolve(artifacts,'restore-ready.png')});
  await section.getByRole('button',{name:'Restart',exact:true}).click();await pause(1200);await browser.close().catch(()=>{});
  await until(async()=>{try{await attach();const current=await invoke('persistence_status');if(current.processId!==before.processId)return current;}catch{}await browser?.close().catch(()=>{});return null;},'Native restart',45000);
  const after=await invoke('persistence_status'),restored=(await native({command:'storage_status'})).storage;
  assert.notEqual(after.processId,before.processId);assert.notEqual(restored.epoch,captured.epoch);assert.equal(restored.restartRequired,false);assert.equal(restored.backups.length,2);
  const current=(await native({command:'profile_list'})).profiles.profiles.find(p=>p.id===original.id);assert.equal(current.name,original.name);assert.equal(current.revision,original.revision);
  const state=(await native({command:'profile_status'})).profiles.profileSession;assert.equal(state.state,'stopped');assert.equal(state.owner,null);
  let hostAfter=null;if(publication){hostAfter=(await native({command:'companion_inventory'})).companions.host;assert.notEqual(hostAfter.hostId,hostBefore.hostId);assert.deepEqual(hostAfter.clients,[]);assert.deepEqual(hostAfter.instances,[]);assert.ok(hostAfter.publications.some(p=>p.publicationHash===publication.publicationHash));}
  const trust=await invoke('mcp_settings_get');assert.equal(trust.enabled,false);assert.deepEqual(trust.credentials,[]);
  await assert.rejects(()=>connect(settings.endpoint,write.token));
  await page.getByRole('button',{name:'Settings',exact:true}).click();await page.getByRole('region',{name:'Backups',exact:true}).getByRole('button',{name:'Create',exact:true}).waitFor();await page.getByRole('region',{name:'Backups',exact:true}).screenshot({path:resolve(artifacts,'restored.png')});
  const manifest=JSON.parse(await readFile(resolve(dirname(process.env.OSG_DCS_BIOS_CONFIG),'backups',id,'manifest.json'),'utf8'));assert.ok(manifest.databaseHash);
  await writeFile(resolve(artifacts,'verified.json'),JSON.stringify({before,after,captured,restored,original,current,state,credentialsInvalidated:true,hostBefore,hostAfter,readerMutationsDenied:true,mcpAndPortal:true,actualNative:true,manifest,verifiedAt:new Date().toISOString()},null,2));
 }finally{for(const client of clients)await client.close().catch(()=>{});await browser?.close().catch(()=>{});}
});
