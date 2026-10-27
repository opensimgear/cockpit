import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url), module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');
const artifacts = resolve('.research/connection-overview-native');

test('dragging opens the shared overview and only a completed connection creates ports, pins and transforms', { timeout: 120_000 }, async () => {
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP ?? 'http://127.0.0.1:9222');
  const page = browser.contexts().flatMap(context => context.pages()).find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page, 'Start the native dev cockpit.');
  const request = value => page.evaluate(async request => (await import('/src/lib/gateway.ts')).gatewayRequest(request), value);
  const name = `Connection overview ${Date.now()}`;
  let profileId;
  await mkdir(artifacts, { recursive: true });
  const saved = async () => (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
  const save = async () => {
    const button = page.locator('.editor-save-actions').getByRole('button', { name: 'Save', exact: true });
    if (await button.isEnabled()) await button.click();
    await expect(page.locator('.editor-save-actions').getByRole('button', { name: 'Duplicate', exact: true })).toBeEnabled();
    await expect(button).toBeDisabled();
    return saved();
  };
  try {
    const current = page.locator('.editor-save-actions').getByRole('button', { name: 'Save', exact: true });
    if (await current.count()) assert.ok(await current.isDisabled(), 'Save or discard the current draft before acceptance.');
    const fixture = await page.evaluate(async name => {
      const { gatewayRequest } = await import('/src/lib/gateway.ts');
      const { blankProfile, addProfileResources } = await import('/src/lib/profile-editor.ts');
      const { graphSetVirtual } = await import('/src/lib/profile-graph.ts');
      const resources = (await gatewayRequest({ command: 'profile_resources_list', offset: 0, limit: 32 })).profileResources.resources;
      const source = resources.filter(resource => resource.resourceType === 'host-controller' && resource.availability === 'online').sort((a, b) => b.portCount - a.portCount)[0];
      if (!source) throw Error('Connect a physical controller.');
      const detail = (await gatewayRequest({ command: 'profile_resources_inspect', resourceId: source.id, offset: 0, limit: 32 })).profileResources;
      const axis = detail.ports.find(port => port.descriptor.direction === 'output' && port.descriptor.contract.delivery === 'state' && port.descriptor.contract.schema.type === 'integer');
      if (!axis) throw Error('A physical axis is required for conversion acceptance.');
      let draft = addProfileResources(blankProfile(crypto.randomUUID(), name), [], [source]);
      const sourceNode = draft.document.resources[0].id;
      draft = graphSetVirtual(draft, { id: 'virtual-target', name: 'Virtual target', required: false, layout: { buttons: ['button-1', 'button-2'], axes: [], hats: [] } });
      draft.document.editor.positions = [{ nodeId: sourceNode, x: 0, y: 100 }, { nodeId: 'virtual-target', x: 354, y: 100 }];
      await gatewayRequest({ command: 'profile_editor_save', ...draft, expectedRevision: null });
      return { id: draft.document.id, sourceNode, axisId: axis.descriptor.id };
    }, name);
    profileId = fixture.id;
    await page.reload();
    await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: 'Profiles', exact: true }).click();
    const back = page.getByRole('button', { name: 'Back to Profiles', exact: true });
    const edit = page.getByRole('button', { name: `Edit ${name}`, exact: true });
    await expect.poll(async () => await back.isVisible() || await edit.isVisible()).toBe(true);
    if (await back.isVisible()) {
      await expect(page.locator('.editor-save-actions').getByRole('button', { name: 'Duplicate', exact: true })).toBeEnabled();
      await back.click();
    }
    await edit.click();
    const overview = page.getByRole('region', { name: 'Signal lanes', exact: true });
    const drag = async () => {
      const output = page.locator(`[data-node-id="${fixture.sourceNode}"] [data-connection-source]`);
      const input = page.locator('[data-node-id="virtual-target"] [data-connection-target]');
      await output.scrollIntoViewIfNeeded();
      const from = await output.boundingBox(), to = await input.boundingBox();
      await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
      await page.mouse.down();
      await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 12 });
      await page.mouse.up();
      await expect(overview).toBeVisible();
      await expect(page.getByRole('dialog', { name: 'Choose connection ports' })).toHaveCount(0);
    };
    const choose = async (label, value) => {
      const field = overview.getByRole('combobox', { name: label, exact: true });
      await field.fill(value);
      await page.locator('button[role="option"]:visible').first().click();
    };
    const pending = overview.getByLabel('New connection draft', { exact: true });
    const empty = state => {
      assert.equal(state.document.resources[0].ports.length, 0);
      assert.equal(state.assignments[0].ports.length, 0);
      assert.equal(state.document.processingNodes.length, 0);
      assert.equal(state.document.bindings.length, 0);
    };
    await drag();
    await expect(overview).toContainText('No connections yet.');
    await overview.getByRole('button', { name: 'Add', exact: true }).click();
    await choose('Find an output', fixture.axisId);
    await choose('Find an input', 'button-1');
    await expect(pending.getByRole('button', { name: 'Connect', exact: true })).toBeDisabled();
    await choose('Add transform', 'Compare');
    await pending.locator('.transform-chip').click();
    const settings = overview.locator('.lane-settings');
    await expect(settings.locator('input[aria-label="Threshold"]')).toBeVisible();
    await settings.locator('input[aria-label="Threshold"]').fill('1234');
    await settings.locator('input[aria-label="Threshold"]').press('Tab');
    empty(await save());
    await pending.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(pending).toHaveCount(0);
    empty(await save());
    await overview.getByRole('button', { name: 'Add', exact: true }).click();
    await choose('Find an output', fixture.axisId);
    await choose('Find an input', 'button-1');
    await page.keyboard.press('Escape');
    await expect(overview).toHaveCount(0);
    empty(await save());
    await drag();
    await overview.getByRole('button', { name: 'Add', exact: true }).click();
    await choose('Find an output', fixture.axisId);
    await choose('Find an input', 'button-1');
    await choose('Add transform', 'Compare');
    await pending.locator('.transform-chip').hover();
    await pending.getByRole('button', { name: 'Remove Compare', exact: true }).click();
    await expect(pending.locator('.draft-transform')).toHaveCount(0);
    await choose('Add transform', 'Compare');
    await pending.locator('.transform-chip').click();
    await settings.locator('input[aria-label="Threshold"]').fill('1234');
    await settings.locator('input[aria-label="Threshold"]').press('Tab');
    await page.screenshot({ path: resolve(artifacts, 'provisional-overview.png') });
    await pending.getByRole('button', { name: 'Connect', exact: true }).click();
    await expect(pending).toHaveCount(0);
    const complete = await save();
    assert.equal(complete.document.resources[0].ports.length, 1);
    assert.equal(complete.assignments[0].ports.length, 1);
    assert.equal(complete.document.processingNodes.length, 1);
    assert.equal(complete.document.processingNodes[0].operation.threshold, 1234);
    assert.equal(complete.document.bindings.length, 2);
    await expect(page.locator('.signal-edge')).toHaveCount(1);
    await drag();
    await expect(overview.locator('.signal-lane:not(.pending-connection)')).toHaveCount(1);
    await overview.getByRole('button', { name: 'Add', exact: true }).click();
    await choose('Find an output', fixture.axisId);
    await choose('Find an input', 'button-1');
    await choose('Add transform', 'Compare');
    await pending.locator('.transform-chip').click();
    await settings.locator('input[aria-label="Threshold"]').fill('2345');
    await settings.locator('input[aria-label="Threshold"]').press('Tab');
    await pending.getByRole('button', { name: 'Replace', exact: true }).click();
    await expect(page.locator('.editor-save-actions').getByRole('button', { name: 'Save', exact: true })).toBeEnabled();
    const replaced = await save();
    assert.equal(replaced.document.resources[0].ports.length, 1);
    assert.equal(replaced.assignments[0].ports.length, 1);
    assert.equal(replaced.document.processingNodes.length, 1);
    assert.notEqual(replaced.document.processingNodes[0].id, complete.document.processingNodes[0].id);
    assert.equal(replaced.document.processingNodes[0].operation.threshold, 2345);
    assert.equal(replaced.document.bindings.length, 2);
    await expect(page.locator('.signal-edge')).toHaveCount(1);
    await page.screenshot({ path: resolve(artifacts, 'completed-overview.png') });
  } catch (error) {
    await page.screenshot({ path: resolve(artifacts, 'failure.png') });
    throw error;
  } finally {
    if (profileId) {
      const state = (await request({ command: 'profile_get', id: profileId })).profiles.document;
      await request({ command: 'profile_delete', id: profileId, expectedRevision: state.revision });
      await page.reload();
    }
    await browser.close();
  }
});
