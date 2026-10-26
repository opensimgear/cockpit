// Real MCP SDK -> running native Cockpit. Discovery fixtures are never launched.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {randomUUID} from 'node:crypto';
import {mkdir,writeFile,rm} from 'node:fs/promises';
import {resolve,relative,isAbsolute} from 'node:path';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';

const enabled=process.platform==='win32'&&process.env.OSG_GAME_DISCOVERY_NATIVE==='1';
const normalize=value=>(value??'').replace(/^\\\\\?\\/,'').replaceAll('/','\\').toLowerCase().replace(/\\+$/,'');
const display=value=>(value??'').replace(/^\\\\\?\\/,'').replaceAll('/','\\');
const within=(parent,path)=>{const child=relative(parent,path);return child!==''&&!child.startsWith('..')&&!isAbsolute(child);};
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));

test('native game discovery MCP scans, pages, applies and replays fixture actions',{skip:!enabled,timeout:240_000},async()=>{
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
  const {chromium}=require(module);
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:142[01]|tauri\.localhost/.test(page.url())&&!page.url().includes('profile-activation-window'));
  assert.ok(page,'Start the native dev Cockpit with CDP before running this acceptance test.');
  const artifacts=resolve('.research/game-discovery-mcp-native'),fixtures=resolve(artifacts,'fixtures-'+Date.now());
  const installs=[resolve(fixtures,'Fixture Copy One'),resolve(fixtures,'Fixture Copy Two')];
  const executableBytes='Harmless game discovery fixture; this file is never launched.\n';
  await mkdir(artifacts,{recursive:true});
  for(const directory of installs){await mkdir(directory,{recursive:true});await writeFile(resolve(directory,'FixtureGame.exe'),executableBytes);}
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const native=async request=>{const reply=await invoke('gateway_request',{request:{...request,clientId:'game-discovery-mcp-native'}});assert.equal(reply.ok,true,request.command+': '+reply.error);return reply.state;};
  const snapshot=async()=>(await native({command:'game_get_state'})).games;
  const saved=state=>state.catalogue.map(game=>({id:game.id,name:game.name,selectedInstallationId:game.selectedInstallationId,installations:game.installations}));
  const inFixture=entry=>within(fixtures,resolve(display(entry.installPath)));
  const pauseUntilReady=async(client,scanId)=>{
    const deadline=Date.now()+90_000;let status;
    do{status=await call(client,'game_discovery_status',{scanId,offset:0,limit:1});if(status.status==='ready')return status;assert.notEqual(status.status,'failed',JSON.stringify(status));await pause(200);}
    while(Date.now()<deadline);
    throw new Error('MCP discovery did not reach ready status.');
  };
  const readSettings=()=>invoke('mcp_settings_get');
  const mcpSettings=async(command,args={})=>{const current=await readSettings();return invoke(command,command==='mcp_settings_update'?{settings:{...args,expectedSettingsRevision:current.settingsRevision}}:{...args,expectedSettingsRevision:current.settingsRevision});};
  const call=async(client,name,args={})=>{const reply=await client.callTool({name,arguments:args},undefined,{timeout:90_000});const body=reply.structuredContent??JSON.parse(reply.content.find(part=>part.type==='text').text);assert.equal(body.ok,true,`${name}: ${JSON.stringify(body)}`);return body.data;};
  const mutation=args=>({...args,operationId:randomUUID()});
  const clients=[];let settings,credentialId,createdGameId,installationIds=[];const evidence={nativeCockpit:true,gameLaunches:false,fixtureDirectories:installs,checks:[]};
  try{
    assert.equal(await page.evaluate(()=>!!window.__TAURI_INTERNALS__?.invoke),true);
    const before=await snapshot();settings=await readSettings();
    const credential=await mcpSettings('mcp_credential_create',{name:'Game discovery acceptance',scope:'write'});credentialId=credential.credential.id;
    const enabledSettings=await mcpSettings('mcp_settings_update',{enabled:true,port:settings.port});
    const client=new Client({name:'OpenSimGear game discovery acceptance',version:'1'},{capabilities:{}});
    await client.connect(new StreamableHTTPClientTransport(new URL(enabledSettings.endpoint),{requestInit:{headers:{Authorization:`Bearer ${credential.token}`}},reconnectionOptions:{maxRetries:0}}));clients.push(client);
    const exposed=(await client.listTools()).tools.map(tool=>tool.name);
    for(const name of ['game_discovery_start','game_discovery_status','game_discovery_apply','game_save','game_installation_select','game_remove'])assert.ok(exposed.includes(name),`MCP did not expose ${name}`);
    evidence.tools=exposed.filter(name=>name.startsWith('game_'));

    const started=await call(client,'game_discovery_start',mutation({paths:[fixtures]}));
    assert.ok(started.scanId);assert.ok(['scanning','ready'].includes(started.status));
    const immediatelyAfterStart=await snapshot();assert.deepEqual(saved(immediatelyAfterStart),saved(before),'Starting discovery must not change the saved game catalogue.');
    const ready=await pauseUntilReady(client,started.scanId);assert.equal(ready.scanId,started.scanId);
    const firstPage=await call(client,'game_discovery_status',{scanId:started.scanId,offset:0,limit:1});
    assert.equal(firstPage.offset,0);assert.ok(firstPage.total>=2);assert.equal(firstPage.entries.length,1);assert.equal(firstPage.nextOffset,1);
    const secondPage=await call(client,'game_discovery_status',{scanId:started.scanId,offset:firstPage.nextOffset,limit:1});
    assert.equal(secondPage.offset,1);assert.equal(secondPage.entries.length,1);
    const entries=[...firstPage.entries,...secondPage.entries];let next=secondPage.nextOffset;
    while(next!==null){const pageResult=await call(client,'game_discovery_status',{scanId:started.scanId,offset:next,limit:32});entries.push(...pageResult.entries);next=pageResult.nextOffset;}
    const fixtureEntries=entries.filter(inFixture);assert.equal(fixtureEntries.length,2,'Both local fixture copies must be present in the full paged result.');
    assert.ok(fixtureEntries.every(entry=>entry.action==='add'&&entry.selected));
    const beforeApply=await snapshot();assert.deepEqual(saved(beforeApply),saved(before),'Reading paged discovery results must not edit saved records.');
    evidence.checks.push('Asynchronous MCP start returned a scan ID; exact-scan status polling paged two local fixture candidates; saved catalogue stayed unchanged.');

    const target=fixtureEntries[0].gameId;
    assert.ok(!before.catalogue.some(game=>game.id===target),'Fixture game identity must be new to this catalogue.');
    const actions=entries.map(entry=>({id:entry.id,selected:inFixture(entry),...(entry.id===fixtureEntries[1].id?{gameId:target}:{})}));
    const applyArgs=mutation({scanId:started.scanId,actions});
    createdGameId=target;
    const applied=await call(client,'game_discovery_apply',applyArgs);
    const replay=await call(client,'game_discovery_apply',applyArgs);assert.deepEqual(replay,applied,'Replaying the same operationId must return the original result.');
    let after=await snapshot();let game=after.catalogue.find(item=>item.id===target);assert.ok(game,'The selected fixture game must be saved.');
    assert.equal(game.installations.length,2);assert.equal(after.catalogue.filter(item=>item.id===target).length,1);
    installationIds=game.installations.map(item=>item.id);assert.ok(installationIds.includes(game.selectedInstallationId));
    assert.equal((await call(client,'game_discovery_status',{scanId:started.scanId,offset:0,limit:2})).status,'applied');
    evidence.checks.push('Apply merged two fixture copies into one game with two installations; exact operation replay returned the same result without duplication.');

    const renamed=`MCP discovery fixture ${Date.now()}`;
    await call(client,'game_save',mutation({gameId:target,name:renamed}));
    const nextInstallation=installationIds.find(id=>id!==game.selectedInstallationId);
    const selected=await call(client,'game_installation_select',mutation({gameId:target,installationId:nextInstallation}));
    after=await snapshot();game=after.catalogue.find(item=>item.id===target);assert.equal(game.name,renamed);assert.equal(game.selectedInstallationId,nextInstallation);
    assert.equal(selected.gameId,target);assert.equal(selected.selectedInstallationId,nextInstallation);
    evidence.checks.push('MCP rename and installation selection are reflected by the same native catalogue state.');

    await call(client,'game_remove',mutation({gameId:target}));createdGameId=undefined;
    after=await snapshot();assert.ok(!after.catalogue.some(item=>item.id===target),'MCP game removal must remove the fixture record.');
    evidence.checks.push('MCP game removal deleted only the fixture-created game record.');
    evidence.completed=true;evidence.checkedAt=new Date().toISOString();
  }catch(error){evidence.failure=String(error);throw error;}
  finally{
    for(const client of clients)await client.close().catch(()=>{});
    try{
      if(createdGameId){const state=await snapshot();if(state.catalogue.some(game=>game.id===createdGameId))await native({command:'game_remove',gameId:createdGameId});}
      for(const directory of installs)if(within(fixtures,directory))await rm(directory,{recursive:true,force:true});
      if(within(artifacts,fixtures))await rm(fixtures,{recursive:true,force:true});
    }finally{
      if(credentialId)await mcpSettings('mcp_credential_revoke',{id:credentialId}).catch(()=>{});
      if(settings)await mcpSettings('mcp_settings_update',{enabled:settings.enabled,port:settings.port}).catch(()=>{});
      evidence.cleanup='Fixture game removed if created, fixture directories deleted, and temporary MCP credential revoked.';
      await import('node:fs/promises').then(({writeFile})=>writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n'));
      await browser.close();
    }
  }
});
