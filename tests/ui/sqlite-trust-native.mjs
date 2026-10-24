// A real browser retains its own origin credentials; Cockpit trust stays sealed in SQLite.
import test from 'node:test';import assert from 'node:assert/strict';
import {createRequire} from 'node:module';import {randomUUID,createHash} from 'node:crypto';import {DatabaseSync} from 'node:sqlite';
import {mkdir,readFile,writeFile} from 'node:fs/promises';import {resolve,dirname,join} from 'node:path';
import {verifiedServerKey,redactDiagnostic} from './native/browser-trust.mjs';
function publicArchive(path){const database=new DatabaseSync(path,{readOnly:true});try{database.exec('PRAGMA busy_timeout=2000');const row=database.prepare("SELECT revision,payload FROM records WHERE domain='companion' AND key='publications'").get();assert.ok(row,'Atomic public archive is absent');const content=JSON.parse(Buffer.from(row.payload).toString('utf8'));const trust=database.prepare("SELECT revision FROM records WHERE domain='secrets' AND key='companion-host'").get();assert.ok(row.revision>0&&trust?.revision>0,'Both guarded snapshot records must be present');assert.equal(database.prepare('PRAGMA quick_check').get().quick_check,'ok');return {revisions:{host:trust.revision,publications:row.revision},hash:createHash('sha256').update(row.payload).digest('hex'),heads:content.savedHeads,publications:content.publications.map(p=>p.publicationHash)};}finally{database.close();}}
test('paired browser reconnects to the same SQLite host identity and assignment after native restart',{
 skip:!process.env.OSG_SQLITE_NATIVE_ACCEPTANCE || !process.env.OSG_SQLITE_TRUST_RESTART_PHASE,timeout:120_000,
},async()=>{
 const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';const {chromium}=require(module),{expect}=require(module+'/test');
 const nativeBrowser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224');let context,companionId,verified=false;
 const phase=process.env.OSG_SQLITE_TRUST_RESTART_PHASE,evidencePath=resolve(process.env.OSG_SQLITE_TRUST_EVIDENCE??'.research/sqlite/trust-restart.json');
 const currentPage=()=>nativeBrowser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith(process.env.OSG_COCKPIT_DEV_URL??'http://127.0.0.1:1421/'));
 let native;
 try{
  await expect.poll(()=>!!currentPage(),{timeout:15_000}).toBe(true);const page=currentPage();
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  native=async(command,companion={})=>{const result=await invoke('gateway_request',{request:{command,companion,clientId:'sqlite-trust-acceptance'}});assert.equal(result.ok,true,result.error);return result.state.companions;};
  const status=await invoke('persistence_status'),host=await native('companion_status');assert.equal(host.listener.enabled,true);
  const certificate=(await native('companion_browser_certificate')).certificatePem,spki=await verifiedServerKey(host.browserUrl,certificate);
  context=await chromium.launchPersistentContext(resolve(process.env.OSG_SQLITE_TRUST_PROFILE??'.research/sqlite/browser-reconnect-profile'),{headless:true,channel:process.env.OSG_BROWSER_CHANNEL??'msedge',args:['--ignore-certificate-errors-spki-list='+spki]});
  const peer=context.pages()[0]??await context.newPage();
  if(phase==='prepare'){
   const before=new Set(host.host.clients.map(c=>c.companionId)),offer=await native('companion_pair_offer',{kind:'browser'});
   await peer.goto(offer.pairing.browserUrl,{waitUntil:'domcontentloaded'});
   await expect.poll(async()=>{companionId=(await native('companion_status')).host.clients.find(c=>!before.has(c.companionId))?.companionId;return !!companionId;},{timeout:30_000}).toBe(true);
   const publication=host.host.publications.find(p=>p.title.startsWith('Qualification panel'));assert.ok(publication,'Run the real panel builder acceptance first.');
   const instanceId='sqlite-reconnect-'+randomUUID().slice(0,12);
   await native('companion_assign',{companionId,instanceId,publicationHash:publication.publicationHash});
   await expect(peer.getByText('Connected · panel active',{exact:true})).toBeVisible({timeout:30_000});
   const directory=join(dirname(status.path),'companions');for(const file of ['companion-host-state.dpapi','companion-identity.dpapi','companion-panels.json','companion-panel-heads.json'])await writeFile(join(directory,file),'retired invalid trust file');
   const evidence={processId:status.processId,hostId:host.host.hostId,caPin:host.listener.caSpkiSha256,companionId,instanceId,publicationHash:publication.publicationHash,archive:publicArchive(status.path)};assert.ok(Object.values(evidence.archive.heads).includes(publication.publicationHash));await mkdir(dirname(evidencePath),{recursive:true});await writeFile(evidencePath,JSON.stringify(evidence,null,2));
   console.log(JSON.stringify({phase,processId:status.processId,hostId:evidence.hostId,companionId,publicationHash:evidence.publicationHash}));
  }else{
   const previous=JSON.parse(await readFile(evidencePath,'utf8'));companionId=previous.companionId;assert.notEqual(status.processId,previous.processId);assert.equal(host.host.hostId,previous.hostId);assert.equal(host.listener.caSpkiSha256,previous.caPin);
   await peer.goto(host.browserUrl,{waitUntil:'domcontentloaded'});await expect(peer.getByText('Connected · panel active',{exact:true})).toBeVisible({timeout:30_000});
   const current=await native('companion_status');assert.equal(current.host.clients.find(c=>c.companionId===companionId).connected,true);
   const assignment=current.host.instances.find(i=>i.instanceId===previous.instanceId);assert.equal(assignment.publicationHash,previous.publicationHash);
   for(const file of ['companion-host-state.dpapi','companion-identity.dpapi','companion-panels.json','companion-panel-heads.json'])assert.equal(await readFile(join(dirname(status.path),'companions',file),'utf8'),'retired invalid trust file');
   assert.deepEqual(publicArchive(status.path),previous.archive);
   verified=true;console.log(JSON.stringify({phase,previousProcessId:previous.processId,processId:status.processId,sameHostAndCa:true,authenticatedReconnect:true,sameAssignment:true,retiredTrustFilesIgnored:true,retiredPanelFilesIgnored:true,exactArchiveAndHeads:true,sameGuardedSnapshotRevisions:true}));
  }
 }catch(error){throw Error(redactDiagnostic(String(error)));}
 finally{await native?.('companion_pair_cancel').catch(()=>{});await context?.close();if((phase==='verify'||verified)&&companionId)await native?.('companion_revoke',{companionId}).catch(()=>{});await nativeBrowser.close();}
});
