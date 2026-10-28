// Exercise the real running Cockpit UI and native volatile input source.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(module),{expect}=require(`${module}/test`);
const artifacts=resolve('.research/debug-controller-native-ui');

test('debug controller authors, drives native inputs, releases buttons and roundtrips its layout',{timeout:180000},async()=>{
  await mkdir(artifacts,{recursive:true});
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page,'Run the development Cockpit first.');
  const native=request=>page.evaluate(async request=>{const{gatewayRequest}=await import('/src/lib/gateway.ts');return gatewayRequest(request);},request);
  const name=`Debug controller acceptance ${Date.now()}`;
  let saved;
  try{
    assert.equal((await native({command:'profile_status'})).profiles.profileSession.owner,null,'Preserve an existing active profile.');
    await page.getByRole('button',{name:'Profiles',exact:true}).click();
    const discard=page.getByRole('alertdialog').getByRole('button',{name:'Discard and continue',exact:true});if(await discard.isVisible().catch(()=>false))await discard.click();
    const back=page.getByRole('button',{name:/Back to Profiles/});if(await back.count())await back.click();
    await page.getByRole('button',{name:'Add',exact:true}).click();
    await page.getByLabel('Profile name',{exact:true}).fill(name);
    await page.getByLabel('Profile name',{exact:true}).press('Enter');
    const add=page.getByRole('button',{name:'Add debug controller',exact:true});
    if(!await add.isVisible())await page.locator('details[aria-label="Controllers"] > summary').click();
    await add.click();
    const block=page.locator('.resource-block').filter({hasText:'Debug controller'});
    await block.locator('.block-heading').click();
    const editor=page.getByRole('region',{name:'Debug controller settings',exact:true});
    await expect(editor).toBeVisible();
    await editor.getByRole('spinbutton',{name:'Buttons',exact:true}).fill('3');
    await editor.getByRole('spinbutton',{name:'Buttons',exact:true}).press('Tab');
    await editor.getByRole('spinbutton',{name:'Axes',exact:true}).fill('4');
    await editor.getByRole('spinbutton',{name:'Axes',exact:true}).press('Tab');
    await editor.getByRole('spinbutton',{name:'Hats',exact:true}).fill('1');
    await editor.getByRole('spinbutton',{name:'Hats',exact:true}).press('Tab');
    await editor.getByLabel('Axis 3 range',{exact:true}).selectOption('unipolar');
    await expect(editor.getByLabel('XY pad for axes 1 and 2',{exact:true})).toBeDisabled();
    await page.getByRole('button',{name:'Save',exact:true}).click();
    await expect(page.getByRole('region',{name:'Notifications',exact:true}).getByText(/^Profile saved\./).last()).toBeVisible();
    const summary=(await native({command:'profile_list'})).profiles.profiles.find(profile=>profile.name===name);assert.ok(summary);
    saved=(await native({command:'profile_assignments_get',id:summary.id})).profiles;
    const resource=saved.document.resources.find(resource=>resource.kind==='debug-controller');assert.ok(resource);
    assert.equal(resource.ports.length,8);assert.deepEqual(saved.assignments,[]);
    assert.equal(resource.ports.find(port=>port.id==='axis-3').contract.schema.min,0);
    assert.ok(resource.ports.every(port=>port.direction==='output'));
    const status=(await native({command:'profile_apply',id:summary.id,expectedRevision:summary.revision,expectedActiveOwner:null})).profiles.profileSession;
    assert.equal(status.state,'running');assert.deepEqual(status.degradation,[]);
    const owner=status.owner,read=async()=> (await native({command:'profile_debug_controller_status',owner,nodeId:resource.id})).debugController;
    const value=async id=>(await read()).values.find(port=>port.portId===id).value;
    const pad=editor.getByLabel('XY pad for axes 1 and 2',{exact:true});
    await expect(pad).toBeEnabled();
    const one=editor.getByRole('button',{name:'Button 1',exact:true});
    await one.focus();await page.keyboard.down('Space');
    await expect.poll(()=>value('button-1')).toBe(true);
    await page.keyboard.up('Space');await expect.poll(()=>value('button-1')).toBe(false);
    const beforePosition=await block.getAttribute('style'),box=await pad.boundingBox();assert.ok(box);
    await page.mouse.move(box.x+box.width*.5,box.y+box.height*.5);await page.mouse.down();
    await page.mouse.move(box.x+box.width*.8,box.y+box.height*.2,{steps:8});await page.mouse.up();
    await expect.poll(()=>value('axis-1')).toBeGreaterThan(15000);
    await expect.poll(()=>value('axis-2')).toBeLessThan(-15000);
    assert.equal(await block.getAttribute('style'),beforePosition,'Dragging inputs must not move the canvas block.');
    const slider=editor.getByLabel('Axis 3',{exact:true}),sliderBox=await slider.boundingBox();assert.ok(sliderBox);
    await page.mouse.move(sliderBox.x+sliderBox.width*.25,sliderBox.y+sliderBox.height/2);await page.mouse.down();
    await page.mouse.move(sliderBox.x+sliderBox.width*.75,sliderBox.y+sliderBox.height/2,{steps:6});await page.mouse.up();
    await expect.poll(()=>value('axis-3')).toBeGreaterThan(15000);
    await editor.getByLabel('Hat 1',{exact:true}).selectOption('3');await expect.poll(()=>value('hat-1')).toBe(3);
    await expect(editor.getByLabel('Hat 1',{exact:true})).toHaveValue('3');
    await editor.getByRole('button',{name:'Reset axes to neutral',exact:true}).click();
    await expect.poll(async()=> (await read()).values.filter(port=>port.portId.startsWith('axis-')).every(port=>port.value===0)).toBe(true);
    // Closing an inspector while a control is held must release its native value.
    await one.focus();await page.keyboard.down('Space');await expect.poll(()=>value('button-1')).toBe(true);
    await page.keyboard.press('Escape');await page.keyboard.up('Space');
    await expect.poll(()=>value('button-1')).toBe(false);
    await block.locator('.block-heading').click();await expect(editor).toBeVisible();
    await expect(pad).toBeEnabled();
    await editor.screenshot({path:resolve(artifacts,'debug-controller.png')});
    await native({command:'profile_stop'});
    const stale=await page.evaluate(async request=>{const{gatewayRequest}=await import('/src/lib/gateway.ts');try{await gatewayRequest(request);return false;}catch{return true;}},{command:'profile_debug_controller_status',owner,nodeId:resource.id});
    assert.equal(stale,true);
    const exported=(await native({command:'profile_export',id:summary.id})).profiles;
    await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify({document:saved.document,resource,owner,exported,nativeValuesVerified:true,staleOwnerRejected:true},null,2));
  }catch(error){await page.screenshot({path:resolve(artifacts,'failure.png'),fullPage:true});throw error;}
  finally{
    try{await native({command:'profile_stop'});const profiles=(await native({command:'profile_list'})).profiles.profiles;const created=profiles.find(profile=>profile.name===name);if(created)await native({command:'profile_delete',id:created.id,expectedRevision:created.revision});}
    finally{await browser.close();}
  }
});
