// Verify authoring gestures and persistence in the real running native dev Cockpit.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
const require=createRequire(import.meta.url),modulePath=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(modulePath),{expect}=require(modulePath+'/test');
const artifacts=resolve('.research/profile-groups-native');

test('saved profile context manages integration and canvas groups accept drops without losing components',{timeout:180000},async()=>{
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=browser.contexts().flatMap(context=>context.pages()).find(page=>/:1420\/(?!.*profile-activation-window)/.test(page.url()));
  assert.ok(page,'Start the native dev Cockpit before this test.');
  await mkdir(artifacts,{recursive:true});
  const name=`Group authoring ${Date.now()}`,errors=[];
  page.on('pageerror',error=>errors.push(error.message));
  const native=async request=>{const response=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'profile-groups-native'}}),request);assert.equal(response.ok,true,response.error);return response.state;};
  const navigation=page.getByRole('navigation',{name:'Main navigation'});
  const palette=page.getByRole('complementary',{name:'Component palette',exact:true});
  const stage=page.getByRole('application',{name:'Profile canvas',exact:true});
  const frames=page.locator('.editor-group-frame');
  const blocks=page.locator('.resource-block');
  async function list(){const discard=page.getByRole('button',{name:'Discard and continue',exact:true});if(await discard.isVisible())await discard.click();const back=page.getByRole('button',{name:'← Back to Profiles',exact:true});if(await back.isVisible())await back.click();else await navigation.getByRole('button',{name:'Profiles',exact:true}).click();if(await discard.isVisible())await discard.click();await expect(page.getByRole('heading',{name:'Profiles',exact:true})).toBeVisible();}
  async function renameGroup(value){await palette.getByRole('button',{name:'Rename Group',exact:true}).click();const input=palette.getByLabel('Group name',{exact:true});await input.fill(value);await input.press('Enter');await expect(palette.getByRole('button',{name:`Focus ${value}`,exact:true})).toBeVisible();}
  async function dragBlockTo(frameId){const heading=blocks.first().locator('.block-heading');await stage.scrollIntoViewIfNeeded();await heading.scrollIntoViewIfNeeded();const from=await heading.boundingBox(),target=frameId?await page.locator(`[data-group-id="${frameId}"]`).boundingBox():await stage.boundingBox();assert.ok(from&&target);const x=frameId?target.x+target.width/2:target.x+target.width-180,y=frameId?target.y+target.height/2:target.y+target.height-145;await page.mouse.move(from.x+from.width/2,from.y+from.height/2);await page.mouse.down();await page.mouse.move(x,y,{steps:14});await page.mouse.up();}
  let savedId;
  try{
    await page.reload();await list();await page.getByRole('button',{name:'Add',exact:true}).click();
    await page.getByRole('button',{name:'Edit profile name',exact:true}).click();const profileName=page.getByLabel('Profile name',{exact:true});await profileName.fill(name);await profileName.press('Enter');
    await expect(stage).toBeVisible();await expect(page.getByRole('button',{name:'Activate profile',exact:true})).toHaveCount(0);await expect(page.locator('.profile-runtime.panel,.group-editor')).toHaveCount(0);
    const context=page.getByLabel('Game context',{exact:true});await expect(context.locator('option')).toHaveCount(5);await expect(context).toHaveValue('');await expect(palette.getByLabel('Integrations',{exact:true})).toHaveCount(0);
    for(const [id,label]of [['dcs','DCS World'],['assetto-corsa-rally','Assetto Corsa Rally'],['x-plane-11','X-Plane 11'],['x-plane-12','X-Plane 12']]){await context.selectOption(id);await expect(blocks).toHaveCount(1);await expect(blocks.first()).toHaveAttribute('aria-label',`${label} · Game integration`);await expect(palette.getByLabel('Integrations',{exact:true})).toHaveCount(0);}
    await context.selectOption('');await expect(blocks).toHaveCount(0);await expect(palette.getByLabel('Integrations',{exact:true})).toHaveCount(0);
    const outputs=palette.getByLabel('Output components',{exact:true});await outputs.locator('summary').click();await expect(outputs).not.toHaveAttribute('open');await outputs.locator('summary').click();await expect(outputs).toHaveAttribute('open');
    await palette.getByRole('button',{name:'Add group',exact:true}).click();await renameGroup('Controls');await palette.getByRole('button',{name:'Add group',exact:true}).click();await renameGroup('Outputs');await page.getByRole('button',{name:'Fit all',exact:true}).click();
    await expect(frames).toHaveCount(2);const rects=await frames.evaluateAll(elements=>elements.map(element=>({x:parseFloat(element.style.left),y:parseFloat(element.style.top),width:parseFloat(element.style.width),height:parseFloat(element.style.height)})));assert.ok(rects[0].x+rects[0].width<=rects[1].x||rects[1].x+rects[1].width<=rects[0].x||rects[0].y+rects[0].height<=rects[1].y||rects[1].y+rects[1].height<=rects[0].y);
    const target=await frames.first().boundingBox(),stageBox=await stage.boundingBox();assert.ok(target&&stageBox);
    await palette.getByRole('button',{name:'Add virtual controller',exact:true}).dragTo(stage,{targetPosition:{x:target.x+target.width/2-stageBox.x,y:target.y+target.height/2-stageBox.y}});
    await expect(blocks).toHaveCount(1);await expect(frames.first()).toHaveAttribute('data-group-members','virtual-1');await expect(page.getByRole('complementary',{name:'Selected block inspector',exact:true})).toHaveCount(0);
    await page.getByRole('button',{name:'Undo',exact:true}).click();await expect(blocks).toHaveCount(0);await expect(frames.first()).toHaveAttribute('data-group-members','');await page.getByRole('button',{name:'Redo',exact:true}).click();await expect(blocks).toHaveCount(1);
    await dragBlockTo('group-2');await expect(frames.first()).toHaveAttribute('data-group-members','');await expect(frames.nth(1)).toHaveAttribute('data-group-members','virtual-1');
    await dragBlockTo(null);await expect(frames.nth(1)).toHaveAttribute('data-group-members','');await page.getByRole('button',{name:'Undo',exact:true}).click();await expect(frames.nth(1)).toHaveAttribute('data-group-members','virtual-1');
    await palette.getByRole('button',{name:'Remove Outputs group',exact:true}).click();await expect(frames).toHaveCount(1);await expect(blocks).toHaveCount(1);await page.getByRole('button',{name:'Undo',exact:true}).click();await expect(frames).toHaveCount(2);await expect(frames.nth(1)).toHaveAttribute('data-group-members','virtual-1');
    await page.getByRole('button',{name:'Save profile',exact:true}).click();await expect(page.getByRole('button',{name:'Save profile',exact:true})).toBeDisabled();const row=(await native({command:'profile_list'})).profiles.profiles.find(profile=>profile.name===name);assert.ok(row);savedId=row.id;const document=(await native({command:'profile_get',id:savedId})).profiles.document;assert.equal(document.editor.groups.length,2);assert.equal(document.virtualOutputs.length,1);assert.deepEqual(document.editor.groups[1].members,['virtual-1']);assert.ok(document.editor.groups.every(group=>group.bounds));
    await list();await page.getByRole('button',{name:`Edit ${name}`,exact:true}).click();await expect(frames).toHaveCount(2);await expect(frames.nth(1)).toHaveAttribute('data-group-members','virtual-1');await expect(blocks).toHaveCount(1);
    await page.screenshot({path:resolve(artifacts,'groups.png'),fullPage:true});assert.deepEqual(errors,[]);await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify({native:true,contextGames:4,independentAccordions:true,nonOverlapping:true,paletteDropWithoutSelection:true,groupTransfers:true,dragOutUngroups:true,removePreservesComponents:true,undoRedo:true,savedAndReopened:true,document,pageErrors:errors},null,2)+'\n');
  }catch(error){console.error(error);await page.screenshot({path:resolve(artifacts,'failure.png'),fullPage:true}).catch(()=>{});throw error;}
  finally{try{await list();const library=(await native({command:'profile_list'})).profiles.profiles;for(const profile of library.filter(profile=>profile.id===savedId||profile.name===name))await native({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});await page.evaluate(()=>window.dispatchEvent(new Event('focus')));}finally{await browser.close();}}
});
