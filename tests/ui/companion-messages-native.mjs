// Inspect the actual running native Cockpit without changing companion settings.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url), modulePath = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(modulePath), { expect } = require(modulePath + '/test');
const artifacts = resolve('.research/companion-messages-native');

test('native companion messages use independent severity icons and accessible full-text tooltips', { timeout: 60_000 }, async () => {
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222', { timeout: 15_000 });
  const page = browser.contexts().flatMap(context => context.pages()).find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page, 'Start the native dev Cockpit before running this test.');
  await mkdir(artifacts, { recursive: true });
  const errors = [], checks = [];
  page.on('pageerror', error => errors.push(error.message));
  const companions = page.getByRole('region', { name: 'Companions', exact: true });
  const details = page.getByRole('region', { name: 'Companion settings', exact: true });
  const messageButtons = container => container.getByRole('button', { name: /^(Info|Warning|Error): / });
  const request = (command, companion = {}) => page.evaluate(({ command, companion }) =>
    window.__TAURI_INTERNALS__.invoke('gateway_request', { request: { command, companion, clientId: 'companion-messages-native' } }), { command, companion });
  async function membership() {
    const reply = await request('companion_status');
    assert.equal(reply.ok, true, reply.error ?? 'Native companion status failed.');
    const host = reply.state.companions.host;
    return { clients: host.clients.map(client => client.companionId).sort(), instances: host.instances.map(instance => instance.instanceId).sort() };
  }
  async function tooltipFor(button) {
    const id = await button.getAttribute('aria-describedby');
    assert.ok(id?.trim(), 'Message icons must describe an actual full-text tooltip.');
    return page.locator(`[id=${JSON.stringify(id)}][role="tooltip"]`);
  }
  async function checkIcon(button) {
    const label = await button.getAttribute('aria-label');
    assert.match(label, /^(Info|Warning|Error): .+/);
    assert.equal((await button.innerText()).trim(), '', 'Messages must appear as icons without inline button text.');
    await expect(button.locator('svg')).toHaveCount(1);
    return label.replace(/^(Info|Warning|Error): /, '');
  }
  async function checkTooltip(button, screenshot) {
    const message = await checkIcon(button), tooltip = await tooltipFor(button);
    await page.mouse.move(5, 5);
    await expect(tooltip).toBeHidden();
    await button.hover();
    await expect(tooltip).toBeVisible();
    await expect(tooltip).toHaveText(message);
    const bounds = await tooltip.boundingBox(), viewport = await page.evaluate(() => ({ width: innerWidth, height: innerHeight }));
    assert.ok(bounds && bounds.x >= 0 && bounds.y >= 0 && bounds.x + bounds.width <= viewport.width + 1 && bounds.y + bounds.height <= viewport.height + 1,
      `Full tooltip must fit inside the native viewport: ${JSON.stringify({ bounds, viewport })}`);
    await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
    await expect(tooltip).toBeVisible();
    await page.screenshot({ path: resolve(artifacts, screenshot), fullPage: true });
    await page.mouse.move(5, 5);
    await expect(tooltip).toBeHidden();
    await button.focus();
    await expect(button).toBeFocused();
    await expect(tooltip).toBeVisible();
    await expect(tooltip).toHaveText(message);
    await page.keyboard.press('Escape');
    await expect(tooltip).toBeHidden();
    await page.keyboard.press('Tab');
    return message;
  }
  try {
    await page.reload();
    assert.equal(await page.evaluate(() => typeof window.__TAURI_INTERNALS__?.invoke), 'function');
    await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: 'Companions', exact: true }).click();
    await expect(companions.getByRole('heading', { name: 'Companions', exact: true })).toBeVisible();
    await expect(companions.locator('.companion-tile').first()).toBeVisible();
    const icons = messageButtons(companions.locator('.tile-messages'));
    const count = await icons.count();
    for (const button of await icons.all()) {
      await checkIcon(button);
      assert.equal(await button.evaluate(element => !!element.closest('.tile-open')), false, 'Status controls must be separate from the Edit button.');
    }
    await expect(companions.locator('.tile-open button')).toHaveCount(0);
    let message = null, tile = companions.locator('.companion-tile').first();
    if (count) {
      const icon = icons.first();
      message = await checkTooltip(icon, 'message-hover.png');
      await icon.click();
      await expect(companions.getByRole('heading', { name: 'Companions', exact: true })).toBeVisible();
      await expect(details).toHaveCount(0);
      tile = icon.locator('xpath=ancestor::article[contains(@class,"companion-tile")]');
      checks.push('Real tile status icons are independent of Edit; clicking a message preserves the companion list');
    } else checks.push('No actual list status needs a message');
    const edit = tile.getByRole('button', { name: /^Edit / });
    const editName = await edit.getAttribute('aria-label');
    await edit.click();
    await expect(details.getByRole('heading', { name: editName.replace(/^Edit /, ''), exact: true })).toBeVisible();
    await expect(details.getByRole('button', { name: /^Info: Changes save/ })).toHaveCount(0);
    const detailIcons = messageButtons(details);
    for (const button of await detailIcons.all()) await checkIcon(button);
    const detailMessage = await detailIcons.count() ? await checkTooltip(detailIcons.first(), 'details-message.png') : null;
    checks.push('Settings omit routine autosave hints; any current messages retain accessible full-text tooltips');
    await details.getByRole('button', { name: 'Back to Companions', exact: true }).click();
    await companions.getByRole('button', { name: 'Pair', exact: true }).click();
    const pairing = companions.getByRole('region', { name: 'Pair companion', exact: true });
    const esp32 = pairing.getByRole('radio', { name: /^ESP32/ });
    await expect(esp32).toBeVisible();
    await expect(esp32).toBeDisabled();
    await expect(pairing.getByText(/pairing (?:is )?temporarily unavailable\.?/i)).toBeVisible();
    await expect(pairing.getByRole('radio', { name: /^Browser/ })).toBeEnabled();
    await expect(pairing.getByRole('radio', { name: /^Mobile/ })).toBeEnabled();
    await expect(pairing.getByRole('region', { name: 'ESP32 companion setup', exact: true })).toHaveCount(0);
    await page.screenshot({ path: resolve(artifacts, 'esp32-pairing-paused.png'), fullPage: true });
    const before = await membership(), rejectedCommands = [];
    for (const [command, companion] of [
      ['companion_pair_offer', { kind: 'esp32' }],
      ['companion_usb_provision', {}],
      ['companion_usb_provision', { companionId: '' }],
    ]) {
      const reply = await request(command, companion);
      assert.equal(reply.ok, false, `${command} must reject new ESP32 pairing.`);
      assert.equal(reply.error, 'ESP32 pairing is temporarily unavailable.');
      rejectedCommands.push({ command, error: reply.error });
    }
    assert.deepEqual(await membership(), before, 'Rejected pairing requests must preserve paired clients and instances.');
    checks.push('ESP32 pairing is visibly disabled while Browser and Mobile remain available; direct native pairing and new USB provisioning are rejected without changing saved companion membership');
    await pairing.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(pairing).toHaveCount(0);
    await expect(companions.getByRole('heading', { name: 'Companions', exact: true })).toBeVisible();
    checks.push('Cancelling the pairing chooser returns to the list without mounting ESP32 setup, selecting USB, scanning, installing or pairing');
    assert.deepEqual(errors, []);
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, readOnly: true, actualListIconCount: count, message, detailMessage, rejectedCommands, preservedMembership: before, checks, errors }, null, 2) + '\n');
  } catch (error) {
    await page.screenshot({ path: resolve(artifacts, 'failure.png'), fullPage: true }); throw error;
  } finally { await browser.close(); }
});
