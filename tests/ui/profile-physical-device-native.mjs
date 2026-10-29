import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir} from 'node:fs/promises';
import {resolve} from 'node:path';

const require=createRequire(import.meta.url);
const modulePath=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(modulePath),{expect}=require(modulePath+'/test');

test('physical controller blocks show the assigned hardware beside the profile label', {timeout:60_000}, async()=>{
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  try {
    const page=browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().includes(':1420')&&!page.url().includes('profile-activation-window'));
    assert.ok(page,'Start the actual native Cockpit.');
    const status=()=>page.evaluate(async()=>{
      const reply=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'profile_status',clientId:'physical-device-ui-native'}});
      if(!reply.ok)throw Error(reply.error);
      return reply.state.profiles.profileSession;
    });
    const before=await status();
    assert.ok(before.owner,'Keep the real DCS profile active during this read-only check.');
    if(!await page.getByRole('heading',{name:'DCS Feature Lab',exact:true}).isVisible()){
      await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Profiles',exact:true}).click();
      const back=page.getByRole('button',{name:'Back to Profiles',exact:true});
      if(await back.isVisible())await back.click();
      await page.getByRole('button',{name:'Edit DCS Feature Lab',exact:true}).click();
    }
    const cyclic=page.getByRole('article',{name:'VKB right · cyclic · Physical controller',exact:true});
    const identity=cyclic.locator('.block-device-name');
    await expect(identity).toHaveText('VKBsim Gladiator EVO R');
    await expect(identity).toHaveAttribute('title','VKBsim Gladiator EVO R');
    await expect(cyclic.locator('.block-heading strong')).toHaveText('VKB right · cyclic');
    const geometry=await cyclic.evaluate(element=>{
      const heading=element.querySelector('.block-heading').getBoundingClientRect();
      const footer=element.querySelector('.block-port-groups').getBoundingClientRect();
      return {headingBottom:heading.bottom,footerTop:footer.top};
    });
    assert.ok(geometry.headingBottom<=geometry.footerTop,'Device identity must not overlap the existing port anchors.');
    assert.deepEqual((await status()).owner,before.owner,'Inspecting hardware labels must not replace the active profile.');
    const directory=resolve('.research/cyclic-axis');
    await mkdir(directory,{recursive:true});
    await page.screenshot({path:resolve(directory,'physical-device-label.png'),fullPage:true});
  } finally {await browser.close();}
});
