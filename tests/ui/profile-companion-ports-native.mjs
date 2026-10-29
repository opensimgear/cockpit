import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir} from 'node:fs/promises';
import {resolve} from 'node:path';

const require = createRequire(import.meta.url);
const modulePath = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const {chromium} = require(modulePath), {expect} = require(modulePath + '/test');

test('the native companion inspector shows consistent input and output port tiles', {timeout: 60_000}, async () => {
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222');
  try {
    const page = browser.contexts().flatMap(context => context.pages()).find(candidate => candidate.url().includes(':1420'));
    assert.ok(page, 'Start the running native dev Cockpit.');
    if (!await page.getByRole('heading', {name: 'DCS Feature Lab', exact: true}).isVisible()) {
      await page.getByRole('navigation', {name: 'Main navigation'}).getByRole('button', {name: 'Profiles', exact: true}).click();
      const back = page.getByRole('button', {name: 'Back to Profiles', exact: true});
      if (await back.isVisible()) await back.click();
      await page.getByRole('button', {name: 'Edit DCS Feature Lab', exact: true}).click();
    }
    const panel = page.getByRole('region', {name: 'Companion panel', exact: true});
    if (!await panel.isVisible()) {
      await page.getByRole('article', {name: 'Browser panel · DCS controls and feedback · Companion', exact: true}).getByRole('button').first().click();
    }
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
    const directory = resolve('.research/companion-port-layout-native');
    await mkdir(directory, {recursive: true});
    await page.screenshot({path: resolve(directory, 'inputs-outputs.png'), fullPage: true});
  } finally { await browser.close(); }
});
