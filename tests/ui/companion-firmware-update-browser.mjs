import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import test from 'node:test';

const require = createRequire(import.meta.url);
const modulePath = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const {chromium} = require(modulePath);
const url = process.env.OSG_COMPANION_FIRMWARE_UI_BROWSER_URL;

test('the firmware update keeps its prepared identity and reconnects once', {
  skip: !url, timeout: 45_000,
}, async () => {
  const browser = await chromium.launch({channel:process.env.OSG_BROWSER_CHANNEL ?? 'msedge',headless:true});
  const page = await browser.newPage();
  const companionId = 'update-fixture', profile = 'sunton-3248s035r';
  const requests = [];
  try {
    await page.route('**/api/gateway/', route => {
      const request = route.request().postDataJSON();
      const reply = companions => route.fulfill({json:{ok:true,state:{companions}}});
      if (request.command === 'companion_status') return reply({
        host:{clients:[{companionId,kind:'esp32',name:'Update fixture',connected:true}]},
        firmware:{[companionId]:{version:'1.0.0',profile}},
        firmwareProfiles:[{id:profile,name:'Sunton',available:true,version:'1.1.0'}],
      });
      if (!['companion_usb_ports','companion_usb_update_prepare','companion_usb_flash','companion_usb_connect'].includes(request.command)) {
        return route.continue();
      }
      requests.push({command:request.command,companion:request.companion});
      if (request.command === 'companion_usb_flash') {
        return reply({usb:{connected:false,flashId:'fixture-flash',flashing:true,waitingForBoot:true}});
      }
      return reply({usbPorts:[{port:'COM-fixture',name:'Simulated port',vid:1,pid:2}]});
    });
    await page.goto(url);
    const navigation = page.getByRole('navigation',{name:'Main navigation'});
    await navigation.waitFor();
    await page.evaluate(async () => {
      const loaded = performance.getEntriesByType('resource')
        .find(entry => new URL(entry.name).pathname === '/src/lib/companion-observations.ts');
      const {companionObservations} = await import(loaded?.name ?? '/src/lib/companion-observations.ts');
      const observations = companionObservations();
      const subscribe = observations.subscribe;
      const fixture = {};
      window.__companionFirmwareFixture = fixture;
      observations.subscribe = function (topic, listener) {
        if (topic === 'companion.presence' || topic === 'companion.listener') {
          let active = true;
          queueMicrotask(() => { if (active) listener({status:'ready',frame:{payload:{}}}); });
          return () => { active = false; };
        }
        if (topic !== 'companion.firmware') return subscribe.call(this, topic, listener);
        fixture.finish = () => listener({status:'ready',frame:{payload:{
          connected:false,flashId:'fixture-flash',flashFinished:true,progress:100,
        }}});
        return () => { delete fixture.finish; };
      };
    });
    await navigation.getByRole('button',{name:'Companions',exact:true}).click();
    await page.getByRole('button',{name:/Update fixture/}).click();
    await page.getByRole('button',{name:'Update firmware',exact:true}).click();
    await page.waitForFunction(() => typeof window.__companionFirmwareFixture.finish === 'function');
    assert.deepEqual(requests, [
      {command:'companion_usb_update_prepare',companion:{companionId:'update-fixture'}},
      {command:'companion_usb_flash',companion:{companionId:'update-fixture',port:'COM-fixture',profile:'sunton-3248s035r'}},
    ]);
    await page.evaluate(() => {
      const {finish}=window.__companionFirmwareFixture;
      finish();
      finish();
    });
    assert.equal(await page.getByRole('button',{name:'Cancel',exact:true}).isDisabled(), true);
    await page.getByRole('button',{name:'Cancel',exact:true}).waitFor({state:'hidden'});
    assert.deepEqual(requests.slice(2), [
      {command:'companion_usb_connect',companion:{port:'COM-fixture'}},
    ]);
  } finally { await browser.close(); }
});
