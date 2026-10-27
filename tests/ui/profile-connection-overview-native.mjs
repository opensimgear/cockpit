import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url), module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');
const artifacts = resolve('.research/connection-overview-native');

test('overview automatically commits valid rows, inserts blocks and isolates transform settings', { timeout: 180_000 }, async () => {
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
    if (await current.count() && await current.isEnabled()) {
      await page.getByRole('button', { name: 'Back to Profiles', exact: true }).click();
      await page.getByRole('button', { name: 'Save and continue', exact: true }).click();
      await expect(current).toHaveCount(0);
    }
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
      if(label==='Add transform'){const trigger=overview.getByRole('button',{name:label,exact:true});if(await trigger.getAttribute('aria-expanded')!=='true')await trigger.click();}
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
    await expect(pending.getByRole('button', { name: /^(Connect|Cancel)$/ })).toHaveCount(0);
    empty(await save());
    await page.keyboard.press('Escape');
    await expect(overview).toHaveCount(0);
    empty(await save());
    await drag();
    await overview.getByRole('button', { name: 'Add', exact: true }).click();
    await choose('Find an output', fixture.axisId);
    await choose('Find an input', 'button-1');
    await choose('Add transform', 'Compare');
    await expect(pending).toHaveCount(0);
    const settings = overview.locator('.lane-settings');
    const threshold = settings.locator('input[aria-label="Threshold"]');
    await expect(settings).toHaveCount(1);
    if (!await threshold.isVisible()) await overview.locator('.signal-lane:not(.pending-connection) .transform-chip').first().click();
    await expect(threshold).toBeVisible();
    await expect(settings.locator('h4,details,button:not(.delete-transform)')).toHaveCount(0);
    const typeBox = await settings.getByRole('combobox', { name: 'Transformation type', exact: true }).boundingBox();
    const settingBox = await threshold.boundingBox();
    assert.ok(typeBox.x + typeBox.width <= settingBox.x, 'Type stays left of its compact settings.');
    assert.ok(Math.abs(typeBox.y - settingBox.y) < 8, 'Type and settings share a compact row.');
    await threshold.fill('1234');
    await threshold.press('Tab');
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
    assert.ok(Math.abs((await pending.boundingBox()).height-(await overview.locator('.signal-lane:not(.pending-connection)').first().boundingBox()).height)<2,'New and saved rows have the same height.');
    await choose('Find an input', 'button-2');
    await choose('Add transform', 'Compare');
    await expect(pending).toHaveCount(0);
    await expect(settings).toHaveCount(1);
    if (!await threshold.isVisible()) await overview.locator('.signal-lane:not(.pending-connection) .transform-chip').last().click();
    await threshold.fill('2345');
    await threshold.press('Tab');
    const independent = await save();
    assert.equal(independent.document.resources[0].ports.length, 1);
    assert.equal(independent.assignments[0].ports.length, 1);
    assert.equal(independent.document.processingNodes.length, 2);
    assert.deepEqual(independent.document.processingNodes.map(node => node.operation.threshold).sort(), [1234, 2345]);
    assert.equal(independent.document.bindings.length, 4);

    // Old profiles can contain a shared stage. Editing one row must first instance it.
    await page.evaluate(async ({ profileId, firstId }) => {
      const { gatewayRequest } = await import('/src/lib/gateway.ts');
      const { graphRemoveNode, graphConnect } = await import('/src/lib/profile-graph.ts');
      const state = (await gatewayRequest({ command: 'profile_assignments_get', id: profileId })).profiles;
      const second = state.document.processingNodes.find(node => node.id !== firstId);
      let next = graphRemoveNode({ document: state.document, assignments: state.assignments }, second.id);
      next = graphConnect(next, { nodeId: firstId, portId: 'out' }, { nodeId: 'virtual-target', portId: 'button-2' });
      await gatewayRequest({ command: 'profile_editor_save', ...next, expectedRevision: state.document.revision });
    }, { profileId, firstId: complete.document.processingNodes[0].id });
    await page.reload();
    await expect(page.locator('.signal-edge')).toHaveCount(1);
    const midpoint = await page.locator('.signal-edge-hit').evaluate(path => {
      const point = path.getPointAtLength(path.getTotalLength() / 2), matrix = path.getScreenCTM();
      return { x: matrix.a * point.x + matrix.c * point.y + matrix.e, y: matrix.b * point.x + matrix.d * point.y + matrix.f };
    });
    await page.mouse.click(midpoint.x, midpoint.y);
    const rows = overview.locator('.signal-lane:not(.pending-connection)');
    await expect(rows).toHaveCount(2);
    await rows.nth(1).locator('.transform-chip').click();
    await expect(settings).toHaveCount(1);
    await threshold.fill('3456');
    await threshold.press('Tab');
    const isolated = await save();
    assert.equal(isolated.document.processingNodes.length, 2);
    assert.deepEqual(isolated.document.processingNodes.map(node => node.operation.threshold).sort(), [1234, 3456]);
    const laneIds = await rows.evaluateAll(items => items.map(item => item.dataset.laneId));
    await rows.first().locator('.insert-transform').click();
    await expect(pending).toHaveCount(0);
    await expect(rows).toHaveCount(2);
    await expect(settings).toHaveCount(1);
    assert.deepEqual(await rows.evaluateAll(items => items.map(item => item.dataset.laneId)), laneIds);
    await expect(rows.first().locator('.transform-chip')).toHaveCount(2);
    await expect(rows.nth(1).locator('.transform-chip')).toHaveCount(1);
    assert.equal(await settings.evaluate(panel => panel.previousElementSibling.dataset.laneId), laneIds[0]);
    await expect(settings.getByRole('combobox', { name: 'Transformation type', exact: true })).toBeVisible();
    await expect(settings.locator('input,textarea,h4,details,button:not(.delete-transform)')).toHaveCount(0);
    assert.ok((await settings.boundingBox()).height < 130, 'Identity has only the compact type and delete controls.');
    await settings.getByRole('combobox', { name: 'Transformation type', exact: true }).selectOption('not');
    const extended = await save();
    assert.equal(extended.document.processingNodes.length, 3);
    assert.equal(extended.document.bindings.length, 5);
    assert.equal(extended.document.processingNodes.filter(node => node.operation.kind === 'not').length, 1);
    assert.deepEqual(extended.document.processingNodes.filter(node => node.operation.kind === 'compare').map(node => node.operation.threshold).sort(), [1234, 3456]);
    await rows.nth(1).locator('.transform-chip').click({ button: 'middle' });
    await expect(pending).toHaveCount(0);
    const incomplete=overview.getByLabel('Incomplete connection',{exact:true});
    await expect(incomplete).toBeVisible();
    await expect(incomplete.locator('.transform-chip')).toHaveCount(0);
    await expect(rows).toHaveCount(2);
    assert.equal(await incomplete.getAttribute('data-lane-id'),laneIds[1]);
    await expect(incomplete.getByRole('img',{name:/Connection error:/})).toBeVisible();
    await expect(incomplete.getByRole('button',{name:'Add transform',exact:true})).toBeVisible();
    await expect(overview.getByRole('button',{name:'Add',exact:true})).toBeVisible();
    assert.ok(Math.abs((await incomplete.boundingBox()).height-(await rows.first().boundingBox()).height)<2,'Incomplete rows keep the same height.');
    await overview.getByRole('button',{name:'Add',exact:true}).click();
    await expect(pending).toBeVisible();
    await expect(rows).toHaveCount(2);
    await pending.getByRole('button',{name:'Delete new connection row',exact:true}).click();
    await expect(pending).toHaveCount(0);
    await rows.nth(1).locator('.insert-transform').click();
    const deleted = await save();
    assert.equal(deleted.document.processingNodes.length, 2);
    assert.equal(deleted.document.bindings.length, 3);
    assert.equal(deleted.document.resources[0].ports.length, 1);
    await choose('Add transform', 'Compare');
    await expect(pending).toHaveCount(0);
    await expect(rows).toHaveCount(2);
    await save();
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
