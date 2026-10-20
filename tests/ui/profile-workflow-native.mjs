// Exercise the real running dev Cockpit and its native gateway. No mocked IPC.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
const require=createRequire(import.meta.url),modulePath=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(modulePath),{expect}=require(modulePath+'/test');
const artifacts=resolve('.research/profile-workflow-native');

test('native cockpit saves with notifications and confirms exclusive profile activation',{timeout:180000},async()=>{
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page,'Start the native dev cockpit before running this test.');
  await page.reload();
  await mkdir(artifacts,{recursive:true});
  const errors=[],names=new Set(),name=`Profile workflow ${Date.now()}`;
  page.on('pageerror',error=>errors.push(error.message));
  const native=async request=>{
    const response=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'profile-workflow-native'}}),request);
    assert.equal(response.ok,true,response.error);return response.state;
  };
  const status=async()=>(await native({command:'profile_status'})).profiles.profileSession;
  const notifications=page.getByRole('region',{name:'Notifications',exact:true});
  async function saved(message='Profile saved.'){
    const toast=notifications.getByRole('status').filter({has:page.getByText(message,{exact:true})});
    await expect(toast).toBeVisible();
    await expect(page.locator('main').getByText(/All changes saved|Profile saved/)).toHaveCount(0);
    await toast.getByRole('button',{name:'Dismiss notification',exact:true}).click();
  }
  async function noSelectedBadge(){await expect(page.getByText('Selected for runtime',{exact:true})).toHaveCount(0);}
  const navigation=page.getByRole('navigation',{name:'Main navigation'});
  const profileList=page.getByRole('list',{name:'Saved profiles',exact:true});
  const tiles=profileList.locator('.profile-tile');
  const tileFor=value=>tiles.filter({has:page.getByRole('button',{name:`Edit ${value}`,exact:true})});
  const pageActions=page.locator('.page-actions');
  const runtime=page.getByRole('region',{name:'Runtime profile status',exact:true});
  async function healthyList(){
    await expect(runtime).not.toBeVisible();
    await expect(pageActions.getByRole('switch',{name:'Automatic profile selection',exact:true})).toBeVisible();
    await expect(pageActions.getByRole('button',{name:'Debug',exact:true}).locator('svg')).toHaveCount(1);
  }
  async function activeList(value){
    await healthyList();
    const selected=tileFor(value);
    await expect(selected).toHaveClass(/\bprofile-active\b/);
    await expect(tiles.first().getByRole('button',{name:`Edit ${value}`,exact:true})).toBeVisible();
    await expect(profileList.locator('.profile-active')).toHaveCount(1);
    await expect(profileList.locator('.profile-muted')).toHaveCount(await tiles.count()-1);
    await expect(selected).not.toHaveClass(/\bprofile-muted\b/);
    for(const tile of await tiles.all()){
      await expect(tile.getByRole('button',{name:/^Edit /})).toBeEnabled();
      await expect(tile.getByRole('button',{name:/^(Activate|Deactivate) /})).toBeEnabled();
    }
  }
  async function list(){
    const switchDialog=page.getByRole('dialog',{name:'Switch active profile?',exact:true});
    if(await switchDialog.isVisible().catch(()=>false))await switchDialog.getByRole('button',{name:'Cancel',exact:true}).click();
    await navigation.getByRole('button',{name:'Profiles',exact:true}).click();
    const debugBack=page.getByRole('button',{name:/^← Back to profile/});if(await debugBack.isVisible().catch(()=>false))await debugBack.click();
    const back=page.getByRole('button',{name:/^← Back to Profiles/});if(await back.isVisible().catch(()=>false))await back.click();
    const discard=page.getByRole('button',{name:'Discard and continue',exact:true});if(await discard.isVisible().catch(()=>false))await discard.click();
    await expect(page.getByRole('heading',{name:'Profiles',exact:true})).toBeVisible();
  }
  try{
    await list();
    if((await status()).state==='recovery-needed')await page.getByRole('button',{name:'Recover outputs',exact:true}).click();
    await native({command:'profile_stop'});
    const modeSwitch=pageActions.getByRole('switch',{name:'Automatic profile selection',exact:true});
    const profileBadge=page.locator('.topbar .profile-status'),modeBadge=profileBadge.locator('.mode-badge');
    await expect(modeSwitch).toBeEnabled();
    if(await modeSwitch.getAttribute('aria-checked')==='true')await modeSwitch.click();
    await expect(modeSwitch).toHaveAttribute('aria-checked','false');
    await expect(profileBadge.getByText('No profile active',{exact:true})).toBeVisible();
    await expect(runtime.getByRole('button',{name:'Deactivate current profile',exact:true})).toHaveCount(0);
    await healthyList();
    await noSelectedBadge();
    await expect(page.getByRole('region',{name:'Automatic profile selection',exact:true})).toHaveCount(0);
    await expect(modeBadge).toHaveText('Manual');assert.ok(!/\bstopped\b/i.test(await profileBadge.innerText()));
    await modeSwitch.click();await expect(modeSwitch).toHaveAttribute('aria-checked','true');
    await expect(modeBadge).toHaveText('Automatic');
    await modeSwitch.click();await expect(modeSwitch).toHaveAttribute('aria-checked','false');
    await expect.poll(async()=>(await native({command:'profile_status'})).profiles.automatic.mode).toBe('manual');
    await native({command:'profile_stop'});
    await expect.poll(async()=>(await status()).owner).toBeNull();
    await expect(profileBadge.getByText('No profile active',{exact:true})).toBeVisible();
    await expect(modeBadge).toHaveText('Manual');assert.ok(!/\bstopped\b/i.test(await profileBadge.innerText()));
    await pageActions.screenshot({path:resolve(artifacts,'profile-mode.png')});
    await page.getByRole('button',{name:'Settings',exact:true}).click();
    const setup=page.getByRole('region',{name:'Controller and desktop output settings'});
    await expect(setup.getByRole('button',{name:'Configure vJoy',exact:true})).toBeVisible();
    await expect(setup.getByRole('button',{name:'Manage HidHide',exact:true})).toBeVisible();
    const desktopEnable=setup.getByLabel('Enable desktop output on this computer',{exact:true});await expect(desktopEnable).toBeEnabled();await desktopEnable.check();
    assert.equal((await native({command:'profile_desktop_status'})).desktop.qualified,true);
    assert.equal(await setup.getByLabel('Application scope',{exact:true}).count(),0);
    await setup.screenshot({path:resolve(artifacts,'output-settings.png')});
    await list();await page.getByRole('button',{name:'Add profile',exact:true}).click();
    await expect(page.getByRole('heading',{name:'New profile',exact:true})).toBeVisible();
    await expect(page.getByRole('region',{name:'Choose resources for new profile'})).toHaveCount(0);
    await expect(page.getByText('0 blocks',{exact:true})).toBeVisible();
    await page.getByLabel('Profile name',{exact:true}).fill(name);names.add(name);
    await page.getByRole('button',{name:'Add keyboard and mouse',exact:true}).click();
    await page.getByLabel('Keyboard and mouse scope',{exact:true}).selectOption('desktop-wide');
    await page.getByRole('button',{name:'Save profile',exact:true}).click();
    await saved();
    await noSelectedBadge();
    assert.equal((await status()).owner,null,'Saving a new inactive profile must not activate it.');
    const first=(await native({command:'profile_list'})).profiles.profiles.find(profile=>profile.name===name);assert.ok(first);
    let document=(await native({command:'profile_get',id:first.id})).profiles.document;
    assert.equal(document.resources.length,0);assert.equal(document.desktopOutputs[0].scope.kind,'desktop-wide');
    // Connect a neutral constant through the actual editor. This acquires desktop
    // output during activation without emitting a nonneutral key into another app.
    await page.getByLabel('Visible transformation',{exact:true}).selectOption('constant');
    await page.getByRole('button',{name:'Add transformation',exact:true}).click();
    await page.getByRole('button',{name:'Connect exact ports',exact:true}).click();
    const composer=page.getByRole('region',{name:'Exact connection editor'});
    await composer.getByLabel('Source block',{exact:true}).selectOption({label:'Constant value'});
    await composer.getByLabel('Exact source output',{exact:true}).selectOption('out');
    await composer.getByLabel('Destination block',{exact:true}).selectOption({label:'Keyboard and mouse'});
    await composer.getByLabel('Exact destination input / action',{exact:true}).selectOption('action-1');
    await composer.getByRole('button',{name:'Create exact route',exact:true}).click();
    await page.getByRole('button',{name:'Save profile',exact:true}).click();
    await saved();
    await expect(page.getByRole('button',{name:'Activate profile',exact:true})).toBeEnabled();
    await page.getByRole('button',{name:'Activate profile',exact:true}).click();
    await expect.poll(async()=>(await status()).owner?.profileId).toBe(first.id);
    const activated=await status(),desktopActive=(await native({command:'profile_desktop_status'})).desktop;
    assert.ok(desktopActive.runtime,'The native desktop output session must actually be acquired.');
    const updated=`${name} updated`;names.add(updated);await page.getByLabel('Profile name',{exact:true}).fill(updated);
    await page.getByRole('button',{name:'Save profile',exact:true}).click();
    await saved('Profile saved and active changes applied.');
    const changed=await status();assert.equal(changed.owner.profileId,first.id);assert.ok(changed.owner.generation>activated.owner.generation);
    assert.equal(changed.owner.gameId,activated.owner.gameId);
    assert.equal((await native({command:'profile_status'})).profiles.session.selectionMode,'manual');
    assert.equal(Object.hasOwn(changed,'paused'),false,'Profile runtime status must not expose a Pause latch.');
    await expect(page.getByRole('button',{name:'Pause',exact:true})).toHaveCount(0);
    await expect(page.getByRole('button',{name:'Clear pause',exact:true})).toHaveCount(0);
    assert.ok(!/\brevision\b/i.test(await page.locator('main').innerText()),'Normal profile editing must not expose revision numbers.');
    await page.getByRole('button',{name:'Debug',exact:true}).click();
    const debug=page.getByRole('region',{name:'Profile debug'});await expect(debug).toBeVisible();
    await expect(debug.getByRole('button',{name:'Deactivate current profile',exact:true})).toBeEnabled();
    await expect(debug.getByRole('button',{name:'Pause',exact:true})).toHaveCount(0);
    await debug.getByRole('button',{name:'Inspect draft preflight',exact:true}).click();
    await expect(debug.getByText('Readiness:',{exact:false})).toBeVisible();
    await expect(debug.getByRole('region',{name:'Applied profile port inspection'})).toBeVisible();
    await debug.screenshot({path:resolve(artifacts,'profile-debug.png')});
    await debug.getByRole('button',{name:'← Back to profile',exact:true}).click();
    await expect(page.getByLabel('Profile name',{exact:true})).toHaveValue(updated);
    await expect(page.getByRole('region',{name:'Native graph preflight'})).toHaveCount(0);
    await list();await page.getByRole('button',{name:'Add profile',exact:true}).click();
    const inactive=`${name} inactive`;names.add(inactive);await page.getByLabel('Profile name',{exact:true}).fill(inactive);await page.getByRole('button',{name:'Save profile',exact:true}).click();
    await saved();assert.deepEqual((await status()).owner,changed.owner,'Saving another profile must leave the active owner unchanged.');
    const second=(await native({command:'profile_list'})).profiles.profiles.find(profile=>profile.name===inactive);assert.ok(second);
    await page.screenshot({path:resolve(artifacts,'inactive-save.png'),fullPage:true});

    // An activation request alone must not select, deactivate or replace an owner.
    // Exercise the actual tile buttons and both choices of the native-guarded dialog.
    await list();await noSelectedBadge();
    assert.equal(first.isDefault,false,'The active ordering acceptance profile must be independent of Default.');
    await activeList(updated);
    await page.screenshot({path:resolve(artifacts,'profile-active-list.png'),fullPage:true});
    await expect(page.getByRole('button',{name:`Deactivate ${updated}`,exact:true})).toBeEnabled();
    const activateSecond=page.getByRole('button',{name:`Activate ${inactive}`,exact:true});
    await activateSecond.click();
    const switchDialog=page.getByRole('dialog',{name:'Switch active profile?',exact:true});
    await expect(switchDialog).toBeVisible();
    await expect(switchDialog).toContainText(`Deactivate ${updated} and activate ${inactive}?`);
    assert.deepEqual((await status()).owner,changed.owner);
    await switchDialog.getByRole('button',{name:'Cancel',exact:true}).click();
    await expect(switchDialog).toHaveCount(0);
    assert.deepEqual((await status()).owner,changed.owner,'Cancelling a switch must preserve the exact active owner.');
    await activeList(updated);
    await activateSecond.click();await switchDialog.getByRole('button',{name:'Switch profile',exact:true}).click();
    await expect.poll(async()=>(await status()).owner?.profileId).toBe(second.id);
    const switched=await status();assert.ok(switched.owner.generation>changed.owner.generation);
    await activeList(inactive);
    await expect(tileFor(updated)).toHaveClass(/\bprofile-muted\b/);
    await expect(page.getByRole('button',{name:`Activate ${updated}`,exact:true})).toBeEnabled();
    await expect(page.getByRole('button',{name:`Deactivate ${inactive}`,exact:true})).toBeEnabled();
    assert.ok(!(await native({command:'profile_desktop_status'})).desktop.runtime||
      (await native({command:'profile_desktop_status'})).desktop.runtime.state==='stopped',
      'Switching to the empty profile must release the former desktop output session.');
    await noSelectedBadge();
    await page.screenshot({path:resolve(artifacts,'profile-switch.png'),fullPage:true});
    await page.getByRole('button',{name:'Debug',exact:true}).click();
    await expect(debug.getByRole('button',{name:'Pause',exact:true})).toHaveCount(0);
    await debug.getByRole('button',{name:'Deactivate current profile',exact:true}).click();
    await expect.poll(async()=>(await status()).owner).toBeNull();
    const inactiveStatus=await status();assert.equal(inactiveStatus.appliedSnapshot,null);
    await expect(debug.getByText('No profile active',{exact:true})).toBeVisible();
    await list();await noSelectedBadge();
    await healthyList();
    await expect(profileBadge.getByText('No profile active',{exact:true})).toBeVisible();
    await expect(profileList.locator('.profile-active,.profile-muted')).toHaveCount(0);
    const allProfiles=(await native({command:'profile_list'})).profiles.profiles;
    const normalOrder=[...allProfiles.filter(profile=>profile.isDefault),...allProfiles.filter(profile=>!profile.isDefault).sort((a,b)=>a.name.localeCompare(b.name))].map(profile=>profile.name);
    await expect.poll(()=>tiles.locator('.profile-copy strong').allTextContents()).toEqual(normalOrder);
    for(const tile of await tiles.all()){
      await expect(tile.getByRole('button',{name:/^Edit /})).toBeEnabled();
      await expect(tile.getByRole('button',{name:/^Activate /})).toBeEnabled();
    }
    await expect(page.getByRole('button',{name:/^Deactivate /})).toHaveCount(0);
    assert.ok(!/\b(running|stopped)\b/i.test(await profileList.innerText()),'The profile tiles must use activation terminology.');
    assert.deepEqual(errors,[]);
    await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify({profileId:first.id,directEmptyEditor:true,profileModeToggle:true,modeInPageActions:true,healthyRuntimeCardHidden:true,debugIcon:true,activeTileFirst:true,remainingTilesMutedAndEnabled:true,deactivationRestoredOrder:normalOrder,headerModeBadge:true,headerStoppedRemoved:true,scope:document.desktopOutputs[0].scope,inactiveSavePreservedOwner:true,saveNotifications:true,savedBannerAbsent:true,selectedBadgeAbsent:true,activeSave:{before:activated.owner,after:changed.owner},desktopQualified:desktopActive.qualified,desktopRuntimeAcquired:!!desktopActive.runtime,debugSeparated:true,activation:{cancelPreservedOwner:changed.owner,switchedOwner:switched.owner,deactivated:inactiveStatus},pageErrors:errors},null,2)+'\n');
  }catch(error){await page.screenshot({path:resolve(artifacts,'failure.png'),fullPage:true});throw error;}
  finally{
    try{await native({command:'profile_stop'});const library=(await native({command:'profile_list'})).profiles;for(const profile of library.profiles.filter(profile=>names.has(profile.name)))await native({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});await list();await page.getByRole('button',{name:'Refresh profiles',exact:true}).click();}
    finally{await browser.close();}
  }
});
