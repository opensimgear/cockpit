// Passive running-Cockpit verification against the real native development
// bridge. Native observations are recorded separately from fixture acceptance.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');
const endpoint = process.env.OSG_DEVICES_NATIVE_UI_URL ?? 'http://127.0.0.1:5173';
const artifacts = resolve(process.env.OSG_DEVICES_NATIVE_ARTIFACTS ?? '.research/system-devices-native-browser');

test('running native Cockpit exposes every observed provider record in both inventory modes', { timeout: 120_000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : {}) });
  const page = await browser.newPage({ viewport: { width: 1500, height: 1100 } });
  const requests = [], pageErrors = [];
  page.on('pageerror', error => pageErrors.push(error.message));
  page.on('request', request => {
    if (request.method() === 'POST' && request.url().includes('/api/gateway/')) requests.push(request.postDataJSON());
  });
  const native = request => page.evaluate(async request => {
    const response = await fetch('/api/gateway/', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-OSG-Client': '1' },
      body: JSON.stringify({ ...request, clientId: crypto.randomUUID() }) });
    const result = await response.json();
    if (!response.ok || !result.ok) throw Error(result.error ?? 'Native observation failed.');
    return result.state;
  }, request);
  try {
    await page.goto(endpoint);
    await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: /^Devices(?:\s|$)/ }).click();
    const devices = page.getByRole('region', { name: 'System devices', exact: true });
    await expect(devices.getByRole('button', { name: 'Refresh devices', exact: true })).toBeEnabled({ timeout: 90_000 });
    await expect.poll(() => devices.locator('.system-list [data-component-id]').count(), { timeout: 90_000 }).toBeGreaterThan(0);
    const controllers = [];
    let offset = 0, total = 0;
    do {
      const response = (await native({ command: 'controller_list', offset, limit: 32 })).controllers;
      assert.equal(response.offset, offset); total = response.total; controllers.push(...response.sources); offset += response.sources.length;
      assert.ok(offset >= total || response.sources.length > 0);
    } while (offset < total);
    const supplemental = (await native({ command: 'system_inventory', offset: 0, limit: 32 })).systemInventory;
    const games = (await native({ command: 'game_get_state' })).games;
    const companions = (await native({ command: 'companion_status' })).companions;
    await devices.getByRole('button', { name: 'Refresh devices', exact: true }).click();
    await expect(devices.getByRole('button', { name: 'Refresh devices', exact: true })).toBeEnabled({ timeout: 90_000 });
    const memory = await page.evaluate(() => JSON.parse(localStorage.getItem('osg-system-inventory-v1')));
    const components = memory.components.filter(component => component.type !== 'host');
    const targets = components.flatMap(component => component.targets);
    for (const controller of controllers) assert.ok(targets.some(target => target.kind === 'controller' && target.id === controller.id), `Missing observed native controller ${controller.name} (${controller.id})`);
    for (const game of games.catalogue) assert.ok(targets.some(target => target.kind === 'integration' && target.id === game.id), `Missing known integration ${game.id}`);
    for (const virtual of supplemental.virtualDevices) assert.ok(targets.some(target => target.kind === 'virtual' && target.id === virtual.id), `Missing known virtual device ${virtual.id}`);
    for (const client of companions.host?.clients ?? []) assert.ok(targets.some(target => target.kind === 'companion' && target.id === client.companionId), `Missing paired companion ${client.companionId}`);
    const listIds = await devices.locator('.system-list [data-component-id]').evaluateAll(nodes => nodes.map(node => node.dataset.componentId).sort());
    assert.deepEqual(listIds, components.map(component => component.id).sort());
    await devices.getByRole('button', { name: 'Topology', exact: true }).click();
    const graph = page.getByRole('region', { name: 'System topology', exact: true });
    await expect(graph).toBeVisible();
    const graphIds = await graph.locator('[data-component-id]').evaluateAll(nodes => nodes.map(node => node.dataset.componentId).filter(id => id !== 'host').sort());
    assert.deepEqual(graphIds, listIds);
    await page.screenshot({ path: resolve(artifacts, 'native-system-topology-initial.png'), fullPage: true });
    await graph.getByRole('button', { name: 'Fit all components', exact: true }).click();
    await page.screenshot({ path: resolve(artifacts, 'native-system-topology.png'), fullPage: true });
    await devices.getByRole('button', { name: 'List', exact: true }).click();
    await page.screenshot({ path: resolve(artifacts, 'native-system-list.png'), fullPage: true });
    assert.deepEqual(pageErrors, []);
    const mutations = requests.filter(request => /^(?:profile_apply|profile_game_launch|profile_editor_save|module_usb_connect|module_usb_claim|module_usb_config_apply|controller_visibility_apply|controller_visibility_save|controller_visibility_enable|controller_forget|companion_usb_arm|companion_assign|companion_revoke|claim|register|apply|save|configure|forget)$/.test(request.command));
    assert.deepEqual(mutations, [], 'Passive inventory inspection must not alter configuration or acquire runtime ownership.');
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ observedAt: new Date().toISOString(), endpoint,
      scope: 'Actual native provider observations through running development Cockpit. No hardware output, visibility mutation or driver qualification asserted.',
      listIds, graphIds, components, controllerCount: controllers.length, virtualCount: supplemental.virtualDevices.length,
      integrationCount: games.catalogue.length, companionCount: companions.host?.clients?.length ?? 0,
      diagnostics: await devices.locator('.provider-diagnostics').allTextContents(), commands: requests.map(request => request.command), mutations, pageErrors }, null, 2) + '\n');
  } catch (error) {
    await page.screenshot({ path: resolve(artifacts, 'failure.png'), fullPage: true });
    await writeFile(resolve(artifacts, 'failure.json'), JSON.stringify({ message: String(error), requests, pageErrors }, null, 2) + '\n');
    throw error;
  } finally { await browser.close(); }
});
