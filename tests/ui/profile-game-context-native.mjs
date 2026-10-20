// Exercise the running native Cockpit, real persisted profiles and an actual
// executable launched by Cockpit. IPC and process verification are never mocked.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {access,mkdir,readFile,writeFile} from 'node:fs/promises';
import {dirname,resolve} from 'node:path';

const require=createRequire(import.meta.url),modulePath=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(modulePath),{expect}=require(modulePath+'/test'),execute=promisify(execFile);
const gameId='x-plane-12',gameName='X-Plane 12',artifacts=resolve('.research/profile-game-context-native');
const executable=process.env.OSG_CONTEXT_GAME_EXE?resolve(process.env.OSG_CONTEXT_GAME_EXE):null;
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));

// Enumerate and stop only the explicitly supplied fixture executable. Check the
// live process path again before Stop-Process so unrelated X-Plane is untouched.
async function fixtureProcesses(stop=false){
  const source=`$ErrorActionPreference='Stop'; $target=[System.IO.Path]::GetFullPath($env:OSG_CONTEXT_GAME_EXE); $items=@(Get-CimInstance Win32_Process -Filter "Name = 'X-Plane.exe'" | Where-Object {[string]::Equals($_.ExecutablePath,$target,[System.StringComparison]::OrdinalIgnoreCase)}); ${stop?`foreach($item in $items){$running=Get-Process -Id $item.ProcessId -ErrorAction SilentlyContinue; if($running -and [string]::Equals($running.Path,$target,[System.StringComparison]::OrdinalIgnoreCase)){Stop-Process -Id $running.Id -Force -ErrorAction Stop}};`:''} ConvertTo-Json -InputObject @($items | ForEach-Object {[pscustomobject]@{pid=$_.ProcessId;executable=$_.ExecutablePath;created=$_.CreationDate.ToUniversalTime().ToString('o')}}) -Compress`;
  const {stdout}=await execute('powershell.exe',['-NoProfile','-NonInteractive','-Command',source],{windowsHide:true,env:{...process.env,OSG_CONTEXT_GAME_EXE:executable},maxBuffer:1024*1024});
  return JSON.parse(stdout.trim()||'[]');
}

test('saved game contexts drive native launch choice, remembered preferences and one-match automatic launch',{skip:!executable,timeout:300_000},async(t)=>{
  assert.equal(process.platform,'win32');await access(executable);await mkdir(artifacts,{recursive:true});
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  t.after(()=>browser.close());
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url())&&!/profile-activation-window/.test(page.url()));
  assert.ok(page,'Start the native dev Cockpit with CDP before running this acceptance.');
  page.setDefaultTimeout(20_000);
  const commands=[],created=new Set(),temporarilyExcluded=[],errors=[],evidence={observedAt:new Date().toISOString(),actualNativeCockpit:true,actualGameFixture:executable,checks:[],cleanupErrors:[]};
  const pageError=error=>errors.push(error.message);page.on('pageerror',pageError);
  const native=async request=>{
    commands.push({command:request.command,at:new Date().toISOString()});
    const reply=await page.evaluate(request=>{if(!window.__TAURI_INTERNALS__?.invoke)throw Error('The existing page is not native Cockpit.');return window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'profile-game-context-native'}});},request);
    assert.equal(reply.ok,true,`${request.command}: ${reply.error}`);return reply.state;
  };
  const status=async()=>(await native({command:'profile_status'})).profiles.profileSession;
  const candidates=async()=>(await native({command:'profile_game_candidates',gameId})).profiles;
  const navigation=page.getByRole('navigation',{name:'Main navigation'}),notifications=page.getByRole('region',{name:'Notifications',exact:true});
  const profileList=page.getByRole('list',{name:'Saved profiles',exact:true});
  const tile=name=>profileList.locator('.profile-tile').filter({has:page.getByRole('button',{name:`Edit ${name}`,exact:true})});
  const switchDialog=page.getByRole('alertdialog',{name:'Switch active profile?',exact:true});
  const chooser=page.getByRole('dialog',{name:`Choose a profile for ${gameName}`,exact:true});
  const matching=page.getByRole('region',{name:'Matching profiles',exact:true});
  const launchButton=()=>page.getByRole('button',{name:`Launch ${gameName}`,exact:true});
  let installationBefore,preferenceBefore=null,modeBefore='manual',first,second;

  async function cancelDialogs(){
    for(const dialog of [chooser,switchDialog,page.getByRole('dialog',{name:'Switch active profile?',exact:true})])if(await dialog.isVisible().catch(()=>false))await dialog.getByRole('button',{name:'Cancel',exact:true}).click();
  }
  async function list(){
    await cancelDialogs();await navigation.getByRole('button',{name:'Profiles',exact:true}).click();
    const debug=page.getByRole('button',{name:'← Back to profile',exact:true});if(await debug.isVisible().catch(()=>false))await debug.click();
    const back=page.getByRole('button',{name:'← Back to Profiles',exact:true});if(await back.isVisible().catch(()=>false))await back.click();
    const discard=page.getByRole('button',{name:'Discard and continue',exact:true});if(await discard.isVisible().catch(()=>false))await discard.click();
    await expect(page.getByRole('heading',{name:'Profiles',exact:true})).toBeVisible();
  }
  async function games(){
    await cancelDialogs();await navigation.getByRole('button',{name:'Games',exact:true}).click();
    const all=page.getByRole('button',{name:/All games$/});if(await all.isVisible().catch(()=>false))await all.click();
    if(!await page.getByRole('button',{name:`Open ${gameName} details`,exact:true}).isVisible().catch(()=>false))await page.getByRole('switch',{name:'Show all supported',exact:true}).check();
    await page.getByRole('button',{name:`Open ${gameName} details`,exact:true}).click();
    await expect(page.getByRole('heading',{name:gameName,exact:true})).toBeVisible();await expect(matching).toBeVisible();await expect(launchButton()).toBeEnabled();
  }
  async function createProfile(name){
    await list();await page.getByRole('button',{name:'Add profile',exact:true}).click();
    await page.getByRole('button',{name:'Edit profile name',exact:true}).click();const input=page.getByLabel('Profile name',{exact:true});await input.fill(name);await input.press('Enter');
    await page.getByLabel('Game context',{exact:true}).selectOption(gameId);
    const palette=page.getByRole('complementary',{name:'Component palette',exact:true});
    await expect(palette.locator('summary').filter({hasText:/^Integrations\b/})).toHaveCount(0);
    await expect(page.locator('.resource-block .block-category').filter({hasText:'Game integration'})).toHaveCount(1);
    await page.getByRole('button',{name:'Save profile',exact:true}).click();
    const toast=notifications.getByRole('status').filter({has:page.getByText('Profile saved.',{exact:true})});await expect(toast).toBeVisible();await toast.getByRole('button',{name:'Dismiss notification',exact:true}).click();
    const saved=(await native({command:'profile_list'})).profiles.profiles.find(profile=>profile.name===name);assert.ok(saved);created.add(saved.id);
    const document=(await native({command:'profile_get',id:saved.id})).profiles.document;assert.deepEqual(document.eligibility.gameIds,[gameId]);assert.equal(document.resources.filter(resource=>resource.kind==='game').length,1);
    await list();await expect(tile(name).getByTitle(`Game context: ${gameName}`,{exact:true})).toBeVisible();await page.getByRole('button',{name:`Edit ${name}`,exact:true}).click();await expect(page.getByLabel('Game context',{exact:true})).toHaveValue(gameId);await expect(page.locator('.resource-block .block-category').filter({hasText:'Game integration'})).toHaveCount(1);
    return saved;
  }
  async function launchAndWait(expected,choose=false,remember=false){
    const ownerBefore=(await status()).owner;await launchButton().click();
    if(choose){await expect(chooser).toBeVisible();await chooser.getByLabel('Profile for game launch',{exact:true}).selectOption(expected.id);await chooser.getByLabel('Remember my choice',{exact:true}).setChecked(remember);await chooser.getByRole('button',{name:'Launch',exact:true}).click();}
    if(ownerBefore&&ownerBefore.profileId!==expected.id){await expect(switchDialog).toBeVisible();await switchDialog.getByRole('button',{name:'Switch profile',exact:true}).click();}
    let watching=true;const progress=[];
    const monitor=(async()=>{while(watching){try{const current=(await native({command:'profile_activation_current'})).profileActivation;if(current)progress.push(current);}catch{}await sleep(150);}})();
    try{await expect.poll(async()=>{const value=await status();return value.owner?.profileId===expected.id&&['running','degraded'].includes(value.state);},{timeout:60_000}).toBe(true);await expect.poll(async()=>(await fixtureProcesses()).length,{timeout:20_000}).toBeGreaterThan(0);await expect(page.locator('.unified-launch .launch-message')).toContainText(/Game launch completed|Game started with unavailable branches/,{timeout:30_000});}
    finally{watching=false;await monitor;}
    const active=await status();assert.equal(active.owner.profileId,expected.id);assert.equal(active.appliedSnapshot.profileId,expected.id);await expect(page.locator('.unified-launch .launch-message')).toContainText(`Selected profile context: ${gameId}.`);evidence.launches??=[];const observed=progress.filter(value=>value.profileId===expected.id).at(-1);let finalProgress=null;if(observed)finalProgress=(await native({command:'profile_activation_status',activationId:observed.activationId})).profileActivation;evidence.launches.push({expectedProfileId:expected.id,active,processes:await fixtureProcesses(),progress:finalProgress,observedProgress:progress});
    await expect(chooser).toHaveCount(0);return active;
  }
  try{
    await page.reload();
    const initial=(await native({command:'profile_status'})).profiles;modeBefore=initial.session.selectionMode;evidence.initial=initial.profileSession;
    if(['recovery-needed','faulted'].includes(initial.profileSession.state))await native({command:'profile_recover'});
    await native({command:'profile_stop'});await native({command:'profile_selection_mode',mode:'manual'});await native({command:'profile_stop'});await fixtureProcesses(true);
    installationBefore=(await native({command:'game_get_state'})).games.manualInstallations?.[gameId]??null;preferenceBefore=(await candidates()).preferredProfileId;
    // Isolate the one-match branch without deleting unrelated saved profiles.
    for(const profile of (await candidates()).profiles){const saved=(await native({command:'profile_get',id:profile.id})).profiles.document,assignments=(await native({command:'profile_assignments_get',id:profile.id})).profiles.assignments;temporarilyExcluded.push({document:saved,assignments});await native({command:'profile_editor_save',document:{...saved,eligibility:{gameIds:saved.eligibility.gameIds.filter(id=>id!==gameId)}},assignments,expectedRevision:saved.revision});}
    await native({command:'profile_game_preference_remove',gameId});await native({command:'game_installation',gameId,executable});
    const stamp=Date.now();first=await createProfile(`Context native A ${stamp}`);await page.getByRole('complementary',{name:'Component palette',exact:true}).getByRole('button',{name:'Add keyboard and mouse',exact:true}).click();await page.locator('.resource-block.desktop .block-heading').click();const scope=page.getByLabel('Keyboard and mouse scope',{exact:true});await expect(scope.locator('option')).toHaveText(['Context scoped','Global']);await expect(page.getByRole('button',{name:'Use last focused external application',exact:true})).toHaveCount(0);await scope.selectOption('desktop-wide');await scope.selectOption('selected-game');await page.getByRole('button',{name:'Save profile',exact:true}).click();await expect(page.getByRole('button',{name:'Save profile',exact:true})).toBeDisabled();const keyboardDraft=(await native({command:'profile_assignments_get',id:first.id})).profiles;const endpoint=keyboardDraft.document.desktopOutputs[0],contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};keyboardDraft.document.processingNodes.push({id:'context-neutral',name:'Released test control',inputs:[],outputs:[{id:'released',name:'Released',direction:'output',contract}],operation:{kind:'constant',value:false}});keyboardDraft.document.bindings.push({id:'context-keyboard',from:{nodeId:'context-neutral',portId:'released'},to:{nodeId:endpoint.id,portId:endpoint.ports[0].id}});await native({command:'profile_editor_save',document:keyboardDraft.document,assignments:keyboardDraft.assignments,expectedRevision:keyboardDraft.document.revision});await page.reload();second=await createProfile(`Context native B ${stamp}`);evidence.profiles=[first,second];evidence.checks.push('UI-created contexts persist, place integration automatically and show tile badges.');
    await list();await page.getByRole('button',{name:`Activate ${first.name}`,exact:true}).click();await expect.poll(async()=>(await status()).owner?.profileId,{timeout:60_000}).toBe(first.id);const before=await status();
    await games();for(const name of [first.name,second.name])await expect(matching.getByTitle(name,{exact:true})).toBeVisible();await page.screenshot({path:resolve(artifacts,'matching-profiles.png'),fullPage:true});
    await launchButton().click();await expect(chooser).toBeVisible();await chooser.getByRole('button',{name:'Cancel',exact:true}).click();assert.deepEqual((await status()).owner,before.owner);assert.deepEqual(await fixtureProcesses(),[]);evidence.checks.push('Cancelling the multiple-match chooser preserves exact active ownership.');
    // A second cancellation occurs after choosing a profile, before native switch.
    await launchButton().click();await expect(chooser).toBeVisible();await chooser.getByLabel('Profile for game launch',{exact:true}).selectOption(second.id);await chooser.getByLabel('Remember my choice',{exact:true}).check();await page.screenshot({path:resolve(artifacts,'choose-profile.png'),fullPage:true});await chooser.getByRole('button',{name:'Launch',exact:true}).click();await expect(switchDialog).toBeVisible();await switchDialog.getByRole('button',{name:'Cancel',exact:true}).click();assert.deepEqual((await status()).owner,before.owner);assert.equal((await candidates()).preferredProfileId,null);assert.deepEqual(await fixtureProcesses(),[]);
    const launched=await launchAndWait(second,true,true);await expect.poll(async()=>(await candidates()).preferredProfileId).toBe(second.id);await expect(matching.locator('li').filter({hasText:second.name})).toContainText('Remembered');evidence.checks.push('Confirmed choice activates the exact saved profile and remembers it before actual fixture launch.');
    await matching.getByLabel('Remembered profile',{exact:true}).selectOption(first.id);await expect.poll(async()=>(await candidates()).preferredProfileId).toBe(first.id);await matching.getByLabel('Remembered profile',{exact:true}).selectOption('');await expect.poll(async()=>(await candidates()).preferredProfileId).toBeNull();evidence.checks.push('Game settings change and remove the persisted remembered choice.');
    await fixtureProcesses(true);await expect.poll(async()=>(await native({command:'game_get_state'})).games.catalogue.find(game=>game.id===gameId).integrationContext?.processVerified,{timeout:20_000}).toBe(false);assert.deepEqual((await status()).owner,launched.owner,'Game exit must preserve active profile ownership.');evidence.checks.push('Game exit invalidates its verified integration context and retains the active profile.');await native({command:'profile_stop'});
    await list();await page.getByRole('button',{name:`Delete ${second.name}`,exact:true}).click();const deletion=page.getByRole('alertdialog',{name:`Delete ${second.name}?`,exact:true});await expect(deletion).toBeVisible();await deletion.getByRole('button',{name:'Delete saved profile',exact:true}).click();await expect(tile(second.name)).toHaveCount(0);created.delete(second.id);
    assert.deepEqual((await candidates()).profiles.map(profile=>profile.id),[first.id]);await games();await expect(matching.getByTitle(first.name,{exact:true})).toBeVisible();await expect(matching.getByText('Automatic',{exact:true})).toBeVisible();await launchAndWait(first);evidence.checks.push('A sole matching saved context activates and launches without a profile chooser.');await page.screenshot({path:resolve(artifacts,'sole-profile-launch.png'),fullPage:true});
    try{evidence.fixtureLog=await readFile(resolve(dirname(executable),'events.log'),'utf8');}catch{}try{evidence.fixtureStartupProfile=JSON.parse(await readFile(resolve(dirname(executable),'startup-profile-session.json'),'utf8'));assert.equal(evidence.fixtureStartupProfile.active.owner.profileId,first.id);}catch(error){if(error.code!=='ENOENT')throw error;}
    assert.deepEqual(errors,[]);
  }catch(error){console.error(error);evidence.failure=String(error);await page.screenshot({path:resolve(artifacts,'failure.png'),fullPage:true}).catch(()=>{});throw error;}
  finally{
    const cleanup=async(label,action)=>{try{await action();}catch(error){evidence.cleanupErrors.push(`${label}: ${error}`);}};
    await cleanup('Cancel dialogs',cancelDialogs);await cleanup('Stop fixture',()=>fixtureProcesses(true));await cleanup('Deactivate profile',()=>native({command:'profile_stop'}));
    for(const id of created)await cleanup(`Remove acceptance profile ${id}`,async()=>{const saved=(await native({command:'profile_get',id})).profiles.document;await native({command:'profile_delete',id,expectedRevision:saved.revision});});
    for(const saved of temporarilyExcluded)await cleanup(`Restore context ${saved.document.id}`,async()=>{const current=(await native({command:'profile_get',id:saved.document.id})).profiles.document;await native({command:'profile_editor_save',document:{...saved.document,revision:current.revision},assignments:saved.assignments,expectedRevision:current.revision});});
    await cleanup('Restore launch preference',async()=>{const matching=await candidates();if(preferenceBefore&&matching.profiles.some(profile=>profile.id===preferenceBefore))await native({command:'profile_game_preference_save',gameId,profileId:preferenceBefore});else await native({command:'profile_game_preference_remove',gameId});});
    if(installationBefore!==undefined)await cleanup('Restore game installation',()=>native({command:'game_installation',gameId,executable:installationBefore}));await cleanup('Restore mode',()=>native({command:'profile_selection_mode',mode:modeBefore}));await cleanup('Leave inactive',()=>native({command:'profile_stop'}));
    evidence.pageErrors=errors;evidence.commands=commands;await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');page.off('pageerror',pageError);await browser.close();assert.deepEqual(evidence.cleanupErrors,[],'Acceptance cleanup failed; inspect evidence.json.');
  }
});
