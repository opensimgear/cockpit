// Opt-in acceptance against the running native Cockpit; no IPC is intercepted.
// Executables contain harmless text bytes and are never launched.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile,rename,rm} from 'node:fs/promises';
import {resolve,relative,isAbsolute} from 'node:path';

const enabled=process.platform==='win32'&&process.env.OSG_GAME_DISCOVERY_NATIVE==='1';
const normalize=value=>(value??'').replace(/^\\\\\?\\/,'').replaceAll('/','\\').toLowerCase().replace(/\\+$/,'');
const display=value=>(value??'').replace(/^\\\\\?\\/,'').replaceAll('/','\\');
const within=(parent,path)=>{const child=relative(parent,path);return child!==''&&!child.startsWith('..')&&!isAbsolute(child);};

test('native discovery reviews libraries, installation copies, moves and explicit removals',{skip:!enabled,timeout:300_000},async()=>{
  const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
  const {chromium}=require(module),{expect}=require(module+'/test');
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:142[01]|tauri\.localhost/.test(page.url())&&!page.url().includes('profile-activation-window'));
  assert.ok(page,'Start the native dev Cockpit with CDP before this test.');page.setDefaultTimeout(25_000);
  const artifacts=resolve('.research/game-discovery-native'),fixtures=resolve(artifacts,'fixtures-'+Date.now());
  await mkdir(fixtures,{recursive:true});
  const dcsA=resolve(fixtures,'DCS-copy-a'),dcsB=resolve(fixtures,'DCS-copy-b'),moved=resolve(fixtures,'DCS-copy-a-moved'),foo=resolve(fixtures,'Foo');
  const errors=[],evidence={actualNativeCockpit:true,gameLaunches:false,fixtureBytes:'Harmless discovery fixture; not executable.',checks:[],snapshots:{},commands:[]};
  const onError=error=>errors.push(error.message);page.on('pageerror',onError);
  const native=async request=>{
    evidence.commands.push(request.command);
    const reply=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'game-discovery-native-acceptance'}}),request);
    assert.equal(reply.ok,true,request.command+': '+reply.error);return reply.state;
  };
  const games=async()=>(await native({command:'game_get_state'})).games;
  const ready=async()=>{
    await expect.poll(async()=>(await games()).discovery?.status,{timeout:90_000}).toBe('ready');
    return games();
  };
  const scan=async paths=>{await native({command:'game_discovery_start',...(paths?{paths}:{})});return ready();};
  const row=entry=>page.locator('[data-entry-id="'+entry.id+'"]');
  const uiGames=async()=>{
    await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Games',exact:true}).click();
    const back=page.getByRole('button',{name:'All games',exact:true});if(await back.isVisible().catch(()=>false))await back.click();
  };
  const openReview=async()=>{
    await uiGames();await page.getByRole('button',{name:'Rescan',exact:true}).click();
    await expect(page.getByRole('heading',{name:'Discover games',exact:true})).toBeVisible();
    return ready();
  };
  const applyUi=async(state,selectedIds)=>{
    for(const entry of state.discovery.entries)await row(entry).getByRole('checkbox').setChecked(selectedIds.has(entry.id));
    await page.getByRole('button',{name:'Apply',exact:true}).click();
    await expect.poll(async()=>(await games()).discovery.status).toBe('applied');
    await expect(page.getByRole('heading',{name:'Discover games',exact:true})).toHaveCount(0);
    return games();
  };
  const fixtureEntry=entry=>within(fixtures,resolve(display(entry.installPath)));
  const installation=(state,path)=>state.catalogue.flatMap(game=>(game.installations??[]).map(installation=>({game,installation}))).find(item=>normalize(item.installation.installPath)===normalize(path));
  const removeFixtureDirectory=async path=>{assert.ok(within(fixtures,path),'Only fixture directories may be deleted.');await rm(path,{recursive:true,force:true});};
  const screenshot=name=>page.screenshot({path:resolve(artifacts,name+'.png'),fullPage:true});
  let original,originalDcs,createdGenericId;
  try{
    assert.equal(await page.evaluate(()=>!!window.__TAURI_INTERNALS__?.invoke),true);
    await expect.poll(async()=>(await games()).discovery?.status!=='scanning',{timeout:90_000}).toBe(true);
    original=await games();originalDcs=original.catalogue.find(game=>game.id==='dcs');
    for(const directory of [resolve(dcsA,'bin'),resolve(dcsB,'bin'),foo])await mkdir(directory,{recursive:true});
    for(const path of [resolve(dcsA,'bin/DCS.exe'),resolve(dcsB,'bin/DCS.exe'),resolve(foo,'Game.exe'),resolve(foo,'AGame.exe'),resolve(foo,'Foo.exe')])await writeFile(path,evidence.fixtureBytes);

    // First exercise the real asynchronous command, then repeat through visible UI.
    const started=(await native({command:'game_discovery_start',paths:[fixtures]})).games;
    assert.ok(['scanning','ready'].includes(started.discovery.status));
    const expectedLibraries=started.discovery.libraries.map(library=>library.id);assert.ok(expectedLibraries.includes('disk'));assert.equal(new Set(expectedLibraries).size,expectedLibraries.length);
    let state=await ready();evidence.snapshots.nativeScan=state.discovery;
    assert.deepEqual([...state.discovery.libraries.map(library=>library.id)].sort(),[...expectedLibraries].sort());
    assert.ok(state.discovery.libraries.every(library=>!['queued','scanning'].includes(library.status)));
    for(const entry of state.discovery.entries)assert.equal(entry.selected,entry.action!=='remove');
    const savedMetadata=state=>state.catalogue.map(game=>({id:game.id,name:game.name,selectedInstallationId:game.selectedInstallationId,installations:game.installations}));
    assert.deepEqual(savedMetadata(state),savedMetadata(original),'Scanning must not edit the saved library.');
    evidence.checks.push('Native asynchronous full scan exposes installed-client library progress records and leaves the saved catalogue unchanged.');

    await openReview();
    await page.getByRole('button',{name:'Add location',exact:true}).click();
    const location=page.getByRole('textbox',{name:'Local discovery location',exact:true});await location.fill(fixtures);
    await page.getByRole('region',{name:'Local locations',exact:true}).getByRole('button',{name:'Add',exact:true}).click();state=await ready();
    await expect(page.locator('.libraries > details.library')).toHaveCount(state.discovery.libraries.filter(library=>library.found||library.message||state.discovery.entries.some(entry=>entry.library===library.id)).length);
    const entries=state.discovery.entries.filter(fixtureEntry);
    const first=entries.find(entry=>normalize(entry.installPath)===normalize(dcsA)),second=entries.find(entry=>normalize(entry.installPath)===normalize(dcsB));
    const generic=entries.find(entry=>normalize(entry.installPath)===normalize(foo));
    assert.ok(first&&second&&generic,'Both known copies and generic executable must be discovered.');
    assert.equal(first.integration,'dcs');assert.equal(second.integration,'dcs');assert.equal(generic.integration,null);
    for(const entry of state.discovery.entries){const checkbox=row(entry).getByRole('checkbox');if(entry.action==='remove')await expect(checkbox).not.toBeChecked();else await expect(checkbox).toBeChecked();}
    const accordion=page.locator('.libraries > details.library').first();await accordion.locator(':scope > summary').click();await expect(accordion).not.toHaveAttribute('open','');await page.waitForTimeout(2500);await expect(accordion).not.toHaveAttribute('open','');await accordion.locator(':scope > summary').click();
    evidence.checks.push('A manually closed library accordion stays closed after progress polling completes.');
    for(const entry of state.discovery.entries){
      if(entry.name==='ARC Raiders')assert.ok(entry.executable.endsWith('PioneerGame.exe'),'A web helper cannot be the default ARC Raiders executable.');
      if(entry.name.startsWith('Diablo II: Resurrected'))assert.ok(entry.executable.endsWith('D2R.exe'),'Roman sequel initials must beat an error reporter.');
    }
    if(state.discovery.entries.some(entry=>entry.image))assert.ok(await page.locator('.entry-art img').count()>0,'Available platform artwork is rendered.');
    await screenshot('review-ready');
    state=await applyUi(state,new Set(entries.map(entry=>entry.id)));evidence.snapshots.applied=state.catalogue;
    const firstInstalled=installation(state,dcsA),secondInstalled=installation(state,dcsB),genericInstalled=installation(state,foo);
    assert.ok(firstInstalled&&secondInstalled&&genericInstalled);createdGenericId=genericInstalled.game.id;
    assert.equal(firstInstalled.game.id,secondInstalled.game.id);assert.equal(firstInstalled.game.integrationKey,'dcs');
    assert.notEqual(firstInstalled.installation.id,secondInstalled.installation.id);
    for(const game of [firstInstalled.game,genericInstalled.game])assert.equal(Object.hasOwn(game,'custom'),false);
    for(const old of originalDcs?.installations??[])assert.ok(state.catalogue.find(game=>game.id===originalDcs.id).installations.some(item=>item.id===old.id),'A discovered copy must preserve every pre-existing installation.');
    evidence.checks.push('Actual Add location and review checkboxes apply only fixtures, preserving two DCS installation instances and all host copies without custom flags.');

    const unchanged=await scan([fixtures]);
    assert.equal(unchanged.discovery.entries.filter(fixtureEntry).length,0,'An imported installation with unchanged paths and executable choices must not propose Update.');
    evidence.checks.push('A second full scan proposes no Update for any unchanged imported fixture, including a title with multiple executable candidates.');

    // Unified metadata/select commands retain identities and make profile lookup usable.
    const renamed='Discovery acceptance '+Date.now();
    await native({command:'game_save',gameId:createdGenericId,name:renamed});
    state=(await native({command:'game_installation_select',gameId:firstInstalled.game.id,installationId:secondInstalled.installation.id})).games;
    assert.equal(state.catalogue.find(game=>game.id===firstInstalled.game.id).selectedInstallationId,secondInstalled.installation.id);
    await uiGames();await page.getByRole('button',{name:'Open '+renamed+' details',exact:true}).click();
    await expect(page.getByRole('textbox',{name:'Game name',exact:true})).toHaveValue(renamed);
    const matching=page.getByRole('region',{name:'Matching profiles',exact:true});
    await expect(matching.getByRole('status')).toHaveCount(0,{timeout:30_000});await expect(matching.getByRole('alert')).toHaveCount(0);
    await screenshot('applied-details');evidence.checks.push('Unified rename and installation selection retain identity; actual matching-profile UI loads without an error.');

    await rename(dcsA,moved);state=await openReview();
    const relocated=state.discovery.entries.find(entry=>normalize(entry.installPath)===normalize(moved));assert.ok(relocated);assert.equal(relocated.action,'update');assert.equal(relocated.updateKind,'replace-path');assert.equal(relocated.installationId,firstInstalled.installation.id);
    // Same-volume rename retains the runtime file identity and proposes replacement.
    await expect(row(relocated).getByRole('combobox',{name:relocated.name+' executable',exact:true})).toHaveValue(relocated.executable);
    await screenshot('move-review');state=await applyUi(state,new Set([relocated.id]));
    assert.equal(installation(state,moved).installation.id,firstInstalled.installation.id);
    assert.ok(installation(state,dcsB));assert.equal(installation(state,dcsA),undefined);
    evidence.checks.push('Moving one copy, reviewing Update and explicitly replacing its installation preserves the old installation id and the second copy.');

    await removeFixtureDirectory(moved);state=await openReview();
    let removed=state.discovery.entries.find(entry=>entry.action==='remove'&&entry.installationId===firstInstalled.installation.id);assert.ok(removed);
    await expect(row(removed).getByRole('checkbox')).not.toBeChecked();await screenshot('removal-opt-in');
    state=await applyUi(state,new Set());
    assert.ok(state.catalogue.find(game=>game.id===firstInstalled.game.id).installations.some(item=>item.id===firstInstalled.installation.id));
    state=await openReview();removed=state.discovery.entries.find(entry=>entry.action==='remove'&&entry.installationId===firstInstalled.installation.id);assert.ok(removed);
    state=await applyUi(state,new Set([removed.id]));
    assert.ok(!state.catalogue.find(game=>game.id===firstInstalled.game.id).installations.some(item=>item.id===firstInstalled.installation.id));assert.ok(installation(state,dcsB));
    evidence.snapshots.afterRemoval=state.catalogue;await screenshot('removed-copy');
    evidence.checks.push('Missing installations default unchecked; Apply without selection retains metadata, and explicit Remove affects only the selected copy.');
    assert.deepEqual(errors,[]);
  }catch(error){evidence.failure=String(error);await screenshot('failure').catch(()=>{});throw error;}
  finally{
    try{
      for(const path of [dcsA,dcsB,moved,foo])await removeFixtureDirectory(path);
      const current=await games();if(current.discovery?.status==='scanning')await ready();
      const cleanup=await scan([fixtures]);
      await native({command:'game_discovery_apply',scanId:cleanup.discovery.scanId,actions:cleanup.discovery.entries.map(entry=>({id:entry.id,selected:entry.action==='remove'&&fixtureEntry(entry)}))});
      if(originalDcs?.selectedInstallationId)await native({command:'game_installation_select',gameId:originalDcs.id,installationId:originalDcs.selectedInstallationId});
      const after=await games();
      assert.ok(!after.catalogue.some(game=>(game.installations??[]).some(item=>within(fixtures,resolve(display(item.installPath))))),'Cleanup must remove every test installation through reviewed removals.');
      for(const game of original?.catalogue??[])for(const old of game.installations??[])assert.ok(after.catalogue.find(item=>item.id===game.id)?.installations.some(item=>item.id===old.id),'Cleanup must preserve all original installations.');
      evidence.cleanup='All fixture installations removed through discovery review; original installations retained.';
    }catch(error){evidence.cleanupFailure=String(error);throw error;}
    finally{page.off('pageerror',onError);evidence.pageErrors=errors;await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');await browser.close();}
  }
});
