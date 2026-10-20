// Exercise the real running dev Cockpit and its native gateway. No mocked IPC.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
const require=createRequire(import.meta.url),modulePath=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(modulePath),{expect}=require(modulePath+'/test');
const artifacts=resolve('.research/profile-workflow-native');

test('running cockpit creates directly, saves inactive profiles, reapplies active saves, and separates Settings/Debug',{timeout:180000},async()=>{
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page,'Start the native dev cockpit before running this test.');
  await mkdir(artifacts,{recursive:true});
  const errors=[],names=new Set(),name=`Profile workflow ${Date.now()}`;
  page.on('pageerror',error=>errors.push(error.message));
  const native=async request=>{
    const response=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'profile-workflow-native'}}),request);
    assert.equal(response.ok,true,response.error);return response.state;
  };
  const status=async()=>(await native({command:'profile_status'})).profiles.profileSession;
  const navigation=page.getByRole('navigation',{name:'Main navigation'});
  async function list(){
    await navigation.getByRole('button',{name:'Profiles',exact:true}).click();
    const debugBack=page.getByRole('button',{name:/^← Back to profile/});if(await debugBack.isVisible().catch(()=>false))await debugBack.click();
    const back=page.getByRole('button',{name:/^← Back to Profiles/});if(await back.isVisible().catch(()=>false))await back.click();
    const discard=page.getByRole('button',{name:'Discard and continue',exact:true});if(await discard.isVisible().catch(()=>false))await discard.click();
    await expect(page.getByRole('heading',{name:'Profiles',exact:true})).toBeVisible();
  }
  try{
    await list();
    if((await status()).state==='recovery-needed')await page.getByRole('button',{name:'Recover outputs',exact:true}).click();
    await native({command:'profile_stop'});await native({command:'profile_pause',paused:false});
    await page.getByRole('button',{name:'Settings',exact:true}).click();
    const setup=page.getByRole('region',{name:'Controller and desktop output settings'});
    await expect(setup.getByRole('button',{name:'Configure vJoy',exact:true})).toBeVisible();
    await expect(setup.getByRole('button',{name:'Manage HidHide',exact:true})).toBeVisible();
    const desktopEnable=setup.getByLabel('Enable desktop output on this computer',{exact:true});await expect(desktopEnable).toBeEnabled();await desktopEnable.check();
    assert.equal((await native({command:'profile_desktop_status'})).desktop.qualified,true);
    assert.equal(await setup.getByLabel('Application scope',{exact:true}).count(),0);
    await setup.screenshot({path:resolve(artifacts,'output-settings.png')});
    await list();await native({command:'profile_pause',paused:false});await page.getByRole('button',{name:'Add profile',exact:true}).click();
    await expect(page.getByRole('heading',{name:'New profile',exact:true})).toBeVisible();
    await expect(page.getByRole('region',{name:'Choose resources for new profile'})).toHaveCount(0);
    await expect(page.getByText('0 blocks',{exact:true})).toBeVisible();
    await page.getByLabel('Profile name',{exact:true}).fill(name);names.add(name);
    await page.getByRole('button',{name:'Add keyboard and mouse',exact:true}).click();
    await page.getByLabel('Keyboard and mouse scope',{exact:true}).selectOption('desktop-wide');
    await page.getByRole('button',{name:'Save profile',exact:true}).click();
    await expect(page.getByText('All changes saved',{exact:false})).toBeVisible();
    assert.equal((await status()).owner,null,'Saving a new inactive profile must not activate it.');
    const first=(await native({command:'profile_list'})).profiles.profiles.find(profile=>profile.name===name);assert.ok(first);
    let document=(await native({command:'profile_get',id:first.id})).profiles.document;
    assert.equal(document.resources.length,0);assert.equal(document.desktopOutputs[0].scope.kind,'desktop-wide');
    // Connect a neutral constant through the actual editor. This acquires desktop
    // output during Start without emitting a nonneutral key into another app.
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
    await expect(page.getByRole('button',{name:'Start profile',exact:true})).toBeEnabled();
    await page.getByRole('button',{name:'Start profile',exact:true}).click();
    await expect.poll(async()=>(await status()).owner?.profileId).toBe(first.id);
    const running=await status(),desktopRunning=(await native({command:'profile_desktop_status'})).desktop;
    assert.ok(desktopRunning.runtime,'The native desktop output session must actually be acquired.');
    const updated=`${name} updated`;names.add(updated);await page.getByLabel('Profile name',{exact:true}).fill(updated);
    await page.getByRole('button',{name:'Save profile',exact:true}).click();
    await expect(page.getByText('Profile saved and running changes applied.',{exact:true})).toBeVisible();
    const changed=await status();assert.equal(changed.owner.profileId,first.id);assert.ok(changed.owner.generation>running.owner.generation);
    assert.equal(changed.owner.gameId,running.owner.gameId);
    assert.equal((await native({command:'profile_status'})).profiles.session.selectionMode,'manual');
    assert.ok(!/\brevision\b/i.test(await page.locator('main').innerText()),'Normal profile editing must not expose revision numbers.');
    await page.getByRole('button',{name:'Debug',exact:true}).click();
    const debug=page.getByRole('region',{name:'Profile debug'});await expect(debug).toBeVisible();
    await expect(debug.getByRole('button',{name:'Stop current profile',exact:true})).toBeEnabled();
    await expect(debug.getByRole('button',{name:'Pause',exact:true})).toBeEnabled();
    await debug.getByRole('button',{name:'Inspect draft preflight',exact:true}).click();
    await expect(debug.getByText('Readiness:',{exact:false})).toBeVisible();
    await expect(debug.getByRole('region',{name:'Applied profile port inspection'})).toBeVisible();
    await debug.screenshot({path:resolve(artifacts,'profile-debug.png')});
    await debug.getByRole('button',{name:'← Back to profile',exact:true}).click();
    await expect(page.getByLabel('Profile name',{exact:true})).toHaveValue(updated);
    await expect(page.getByRole('region',{name:'Native graph preflight'})).toHaveCount(0);
    await list();await page.getByRole('button',{name:'Add profile',exact:true}).click();
    const inactive=`${name} inactive`;names.add(inactive);await page.getByLabel('Profile name',{exact:true}).fill(inactive);await page.getByRole('button',{name:'Save profile',exact:true}).click();
    await expect(page.getByText('Profile saved.',{exact:true})).toBeVisible();assert.deepEqual((await status()).owner,changed.owner,'Saving another profile must leave the running owner unchanged.');
    await page.screenshot({path:resolve(artifacts,'inactive-save.png'),fullPage:true});
    assert.deepEqual(errors,[]);
    await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify({profileId:first.id,directEmptyEditor:true,scope:document.desktopOutputs[0].scope,inactiveSavePreservedOwner:true,activeSave:{before:running.owner,after:changed.owner},desktopQualified:desktopRunning.qualified,desktopRuntimeAcquired:!!desktopRunning.runtime,debugSeparated:true,pageErrors:errors},null,2)+'\n');
  }catch(error){await page.screenshot({path:resolve(artifacts,'failure.png'),fullPage:true});throw error;}
  finally{
    try{await native({command:'profile_stop'});await native({command:'profile_pause',paused:false});await native({command:'profile_select',id:'00000000-0000-4000-8000-000000000001',selectionSource:'manual'});const library=(await native({command:'profile_list'})).profiles;for(const profile of library.profiles.filter(profile=>names.has(profile.name)))await native({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});await list();await page.getByRole('button',{name:'Refresh profiles',exact:true}).click();}
    finally{await browser.close();}
  }
});
