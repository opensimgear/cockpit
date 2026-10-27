// Uses the running native cockpit and an attached physical HID controller.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url), module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');
const artifacts = resolve('.research/profile-controller-inputs');

test('physical controller settings stay simple and show every real input without activating the profile', { timeout: 120_000 }, async () => {
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP ?? 'http://127.0.0.1:9222');
  const page = browser.contexts().flatMap(context => context.pages()).find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page, 'Start the native dev cockpit.');
  const request = value => page.evaluate(async request => (await import('/src/lib/gateway.ts')).gatewayRequest(request), value);
  const name = `Controller inputs ${Date.now()}`;
  let profileId;
  await mkdir(artifacts, { recursive: true });
  try {
    const save = page.locator('.editor-save-actions').getByRole('button', { name: 'Save', exact: true });
    if (await save.count()) assert.ok(await save.isDisabled(), 'Save or discard an existing dirty draft before running acceptance.');
    const fixture = await page.evaluate(async name => {
      const { gatewayRequest } = await import('/src/lib/gateway.ts');
      const { blankProfile, addProfileResources } = await import('/src/lib/profile-editor.ts');
      const { controllerSourceId } = await import('/src/lib/controller-inputs.ts');
      const resources = (await gatewayRequest({ command: 'profile_resources_list', offset: 0, limit: 32 })).profileResources.resources;
      const physical = resources.filter(resource => resource.resourceType === 'host-controller');
      const online = physical.filter(resource => resource.availability === 'online').sort((a, b) => b.portCount - a.portCount)[0];
      if (!online) throw new Error('Connect a physical HID controller to run native input acceptance.');
      const offline = physical.find(resource => resource.availability === 'offline');
      const draft = addProfileResources(blankProfile(crypto.randomUUID(), name), [], [online, ...(offline ? [offline] : [])]);
      await gatewayRequest({ command: 'profile_editor_save', ...draft, expectedRevision: null });
      return { id: draft.document.id, sourceId: controllerSourceId(online), nodeId: draft.assignments.find(item => item.sourceId === online.id).resourceId,
        offlineNodeId: offline ? draft.assignments.find(item => item.sourceId === offline.id).resourceId : null };
    }, name);
    profileId = fixture.id;
    await page.reload();
    await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: 'Profiles', exact: true }).click();
    const back = page.getByRole('button', { name: 'Back to Profiles', exact: true });
    await expect.poll(async () => await back.isVisible() || await page.getByRole('button', { name: `Edit ${name}`, exact: true }).isVisible()).toBe(true);
    if (await back.isVisible()) {
      await expect(page.locator('.editor-save-actions').getByRole('button', { name: 'Duplicate', exact: true })).toBeEnabled();
      await back.click();
    }
    await page.getByRole('button', { name: `Edit ${name}`, exact: true }).click();
    const block = page.locator(`[data-node-id="${fixture.nodeId}"] .block-heading`);
    await block.press('Enter');
    const inspector = page.getByRole('complementary', { name: 'Selected block inspector' });
    await expect(inspector.getByLabel('Label', { exact: true })).toBeVisible();
    await expect(inspector.getByLabel('Required', { exact: true })).toBeVisible();
    await expect(inspector.getByLabel('Visibility', { exact: true })).toBeVisible();
    await expect(inspector.getByRole('button', { name: 'Remove', exact: true })).toBeVisible();
    const remove = inspector.getByRole('button', { name: 'Remove', exact: true });
    await expect(remove).toHaveClass(/destructive-button/);
    await expect(remove).toHaveCSS('border-top-style', 'solid');
    const removeBox = await remove.boundingBox(), panelBox = await inspector.boundingBox();
    assert.ok(removeBox.x > panelBox.x + panelBox.width / 2, 'Remove sits on the right.');
    assert.ok(panelBox.y + panelBox.height - removeBox.y - removeBox.height < 40, 'Remove stays at the bottom of the panel.');
    await expect(inspector.locator('button')).toHaveCount(1);
    await expect(inspector.getByRole('button', { name: 'Close component settings' })).toHaveCount(0);
    await expect(inspector.locator('details')).toHaveCount(0);
    for (const text of ['Provider catalogue', 'Connection metadata', 'Learn physical input', 'Controller publication policy']) await expect(inspector).not.toContainText(text);
    const controls = (await request({ command: 'controller_inspect', sourceId: fixture.sourceId, offset: 0 })).controllers;
    await expect.poll(() => inspector.locator('[data-control-id]').count()).toBe(controls.total);
    assert.ok(controls.total > 0);
    await expect(inspector.locator('.button-information')).toHaveCount(0);
    for (const meter of await inspector.getByRole('meter').all()) assert.ok(Number.isFinite(Number(await meter.getAttribute('value'))));
    const squares = inspector.locator('.button-indicator');
    if (await squares.count()) assert.equal(Math.round((await squares.first().boundingBox()).width), 28);
    const blank = await page.getByRole('application', { name: 'Profile canvas' }).evaluate(node => {
      const box = node.getBoundingClientRect();
      for (let y = 16; y < Math.min(box.height, innerHeight - box.top) - 16; y += 24) for (let x = 16; x < box.width / 3; x += 24) {
        const hit = document.elementFromPoint(box.left + x, box.top + y);
        if (hit && node.contains(hit) && !hit.closest('.resource-block,.canvas-inspector,button')) return { x: box.left + x, y: box.top + y };
      }
    });
    assert.ok(blank, 'Find blank canvas outside the controller and panel.');
    await page.mouse.click(blank.x, blank.y);
    await expect(inspector).toHaveCount(0);
    await block.press('Enter');
    await expect(inspector.getByLabel('Label', { exact: true })).toBeVisible();
    await inspector.getByLabel('Label', { exact: true }).fill('Physical controls');
    await inspector.getByLabel('Label', { exact: true }).press('Tab');
    await inspector.getByLabel('Required', { exact: true }).check();
    await inspector.getByLabel('Visibility', { exact: true }).selectOption('hidden-while-active');
    await page.locator('.editor-save-actions').getByRole('button', { name: 'Save', exact: true }).click();
    await expect(page.locator('.editor-save-actions').getByRole('button', { name: 'Save', exact: true })).toBeDisabled();
    const saved = (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
    assert.equal(saved.document.resources.find(resource => resource.id === fixture.nodeId).name, 'Physical controls');
    assert.equal(saved.document.resources.find(resource => resource.id === fixture.nodeId).required, true);
    assert.equal(saved.document.visibility.find(intent => intent.resourceId === fixture.nodeId).mode, 'hidden-while-active');
    assert.equal(saved.document.bindings.length, 0);
    await expect.poll(() => inspector.locator('[data-control-id]').count()).toBe(controls.total);
    await page.screenshot({ path: resolve(artifacts, 'simple-controller.png') });
    await inspector.getByRole('button', { name: 'Remove', exact: true }).click();
    await expect(page.locator(`[data-node-id="${fixture.nodeId}"]`)).toHaveCount(0);
    await page.getByRole('button', { name: 'Undo', exact: true }).click();
    await expect(page.locator(`[data-node-id="${fixture.nodeId}"]`)).toHaveCount(1);
    const afterUndo = page.locator('.editor-save-actions').getByRole('button', { name: 'Save', exact: true });
    if (await afterUndo.isEnabled()) await afterUndo.click();
    const restored = (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
    assert.deepEqual(restored.document.resources, saved.document.resources);
    assert.deepEqual(restored.document.bindings, saved.document.bindings);
    assert.deepEqual(restored.assignments, saved.assignments);
    if (fixture.offlineNodeId) {
      await page.locator(`[data-node-id="${fixture.offlineNodeId}"] .block-heading`).press('Enter');
      await expect(inspector).toContainText('Controller is offline.');
      await expect(inspector.locator('[data-control-id]')).toHaveCount(0);
    }
    assert.equal((await request({ command: 'profile_status' })).profiles.profileSession.owner, null);
  } catch (error) {
    await page.screenshot({ path: resolve(artifacts, 'failure.png') });
    throw error;
  } finally {
    if (profileId) {
      const saved = (await request({ command: 'profile_get', id: profileId })).profiles.document;
      await request({ command: 'profile_delete', id: profileId, expectedRevision: saved.revision });
      await page.reload();
    }
    await browser.close();
  }
});
