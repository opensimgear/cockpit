import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';

const require = createRequire(import.meta.url);
const modulePath = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const {chromium} = require(modulePath), {expect} = require(modulePath + '/test');

test('the native companion inspector shows consistent input and output port tiles', {timeout: 60_000}, async () => {
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222');
  const page = browser.contexts().flatMap(context => context.pages()).find(candidate => candidate.url().includes(':1420') && !candidate.url().includes('profile-activation-window'));
  const request = value => page.evaluate(async value => {
    const {gatewayRequest} = await import('/src/lib/gateway.ts');
    return gatewayRequest(value);
  }, value);
  let fixture;
  try {
    assert.ok(page, 'Start the running native dev Cockpit.');
    const name = 'Companion port layout fixture ' + Date.now();
    fixture = (await request({command:'profile_create', name})).profiles.document;
    const contract = {delivery:'state', schema:{type:'boolean'}, freshnessMs:1000, fallback:false};
    const document = structuredClone(fixture);
    document.resources.push({id:'panel', name:'Fixture panel', kind:'companion', required:false, ports:[
      {id:'lamp', name:'Lamp', direction:'input', contract},
      {id:'button', name:'Button', direction:'output', contract},
    ]});
    fixture = (await request({command:'profile_editor_save', document, assignments:[], expectedRevision:fixture.revision})).profiles.document;
    await page.getByRole('navigation', {name:'Main navigation'}).getByRole('button', {name:'Profiles', exact:true}).click();
    const back = page.getByRole('button', {name:'Back to Profiles', exact:true});
    if (await back.isVisible()) await back.click();
    await page.getByRole('button', {name:'Edit ' + name, exact:true}).click();
    await page.getByRole('article', {name:'Fixture panel · Companion', exact:true}).getByRole('button').first().click();
    const inspector = page.getByRole('complementary', {name: 'Selected block inspector'});
    const inputs = inspector.getByRole('region', {name: 'Companion inputs', exact: true});
    const outputs = inspector.getByRole('region', {name: 'Companion outputs', exact: true});
    await expect(inputs.getByRole('heading', {name: 'Inputs', exact: true})).toBeVisible();
    await expect(outputs.getByRole('heading', {name: 'Outputs', exact: true})).toBeVisible();
    assert.ok(await inputs.locator('.port-tile').count() > 0);
    assert.ok(await outputs.locator('.port-tile').count() > 0);
    await expect(inputs.getByRole('textbox', {name: 'Filter inputs', exact: true})).toBeVisible();
    await expect(outputs.getByRole('textbox', {name: 'Filter outputs', exact: true})).toBeVisible();
    assert.equal(await inspector.locator('details.ports-section').count(), 0);
    const input = inputs.getByRole('textbox', {name:'Filter inputs', exact:true});
    await input.fill('unmatched');
    await expect(inputs.getByText('No matching inputs.', {exact:true})).toBeVisible();
    await expect(outputs.locator('[data-port-id="button"]')).toBeVisible();
    await inputs.getByRole('button', {name:'Clear input filter', exact:true}).click();
    await expect(inputs.locator('[data-port-id="lamp"]')).toBeVisible();
    await expect(input).toBeFocused();
  } finally {
    try {
      if (fixture) await request({command:'profile_delete', id:fixture.id, expectedRevision:fixture.revision});
    } finally {
      await browser.close();
    }
  }
});
