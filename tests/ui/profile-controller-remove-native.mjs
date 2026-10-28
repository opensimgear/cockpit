import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { randomUUID } from 'node:crypto';

const require = createRequire(import.meta.url), module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');

test('native physical and virtual controller Remove actions share one destructive footer style', { timeout: 180_000 }, async () => {
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP ?? 'http://127.0.0.1:9222');
  const page = browser.contexts().flatMap(context => context.pages()).find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page, 'Start the native dev cockpit.');
  const request = value => page.evaluate(async request => (await import('/src/lib/gateway.ts')).gatewayRequest(request), value);
  const name = `Controller remove ${randomUUID()}`;
  let profileId;
  const saved = async () => (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
  const save = async () => {
    const button = page.locator('.editor-save-actions').getByRole('button', { name: 'Save', exact: true });
    if (await button.isEnabled()) await button.click();
    await expect(button).toBeDisabled();
    return saved();
  };
  const actionSnapshot = async () => page.locator('.canvas-inspector .controller-actions').evaluate(footer => {
    const button = footer.querySelector('button');
    if (!button) throw Error('Controller action footer has no Remove button.');
    const style = getComputedStyle(button), footerStyle = getComputedStyle(footer), rect = button.getBoundingClientRect(), footerRect = footer.getBoundingClientRect();
    return {
      count: footer.querySelectorAll('button').length,
      label: button.textContent.replace(/\s+/g, ' ').trim(),
      classes: button.className,
      background: style.backgroundColor,
      border: style.border,
      borderRadius: style.borderRadius,
      color: style.color,
      font: style.font,
      padding: style.padding,
      height: style.height,
      icon: !!button.querySelector('svg, [data-icon], .icon'),
      rightAligned: Math.abs(rect.right - footerRect.right + parseFloat(footerStyle.paddingRight || '0')) < 1,
      footerBorder: footerStyle.borderTop,
    };
  });

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
      const inventory = (await gatewayRequest({ command: 'profile_resources_list', offset: 0, limit: 64 })).profileResources.resources;
      const source = inventory.find(resource => resource.resourceType === 'host-controller' && resource.availability === 'online');
      if (!source) throw Error('An online physical host controller is required.');
      let draft = addProfileResources(blankProfile(crypto.randomUUID(), name), [], [source]);
      const physical = draft.document.resources.find(resource => draft.assignments.some(row => row.resourceId === resource.id && row.sourceId === source.id));
      draft = graphSetVirtual(draft, { id: 'virtual-remove-target', name: 'Virtual remove target', required: false, layout: { buttons: ['button-1'], axes: [], hats: [] } });
      draft.document.editor.positions = [
        { nodeId: physical.id, x: 80, y: 100 },
        { nodeId: 'virtual-remove-target', x: 500, y: 100 },
      ];
      await gatewayRequest({ command: 'profile_editor_save', ...draft, expectedRevision: null });
      return { id: draft.document.id, physicalId: physical.id, sourceId: source.id };
    }, name);
    profileId = fixture.id;

    await page.reload();
    await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: 'Profiles', exact: true }).click();
    const back = page.getByRole('button', { name: 'Back to Profiles', exact: true });
    const edit = page.getByRole('button', { name: `Edit ${name}`, exact: true });
    await expect.poll(async () => await back.isVisible() || await edit.isVisible()).toBe(true);
    if (await back.isVisible()) await back.click();
    await edit.click();

    await page.locator(`.resource-block[data-node-id="${fixture.physicalId}"] .block-heading`).click();
    const footer = page.locator('.canvas-inspector .controller-actions');
    await expect(footer).toBeVisible();
    const physicalStyle = await actionSnapshot();
    assert.equal(physicalStyle.count, 1, 'Physical controller has one footer action.');
    assert.equal(physicalStyle.label, 'Remove');
    assert.match(physicalStyle.classes, /secondary-button/);
    assert.match(physicalStyle.classes, /destructive-button/);
    assert.ok(physicalStyle.icon, 'Remove includes a trash icon.');
    assert.ok(physicalStyle.rightAligned, 'Remove is right aligned in its footer.');

    await page.locator('.canvas-stage').click({ position: { x: 12, y: 12 } });
    await expect(footer).toHaveCount(0);
    await page.locator('.resource-block[data-node-id="virtual-remove-target"] .block-heading').click();
    await expect(footer).toBeVisible();
    await expect(page.locator('.canvas-inspector').getByRole('button', { name: 'Remove', exact: true })).toHaveCount(1);
    await expect(page.locator('.canvas-inspector').getByRole('button', { name: 'Remove block and its routes', exact: true })).toHaveCount(0);
    const virtualStyle = await actionSnapshot();
    assert.equal(virtualStyle.count, 1, 'Virtual controller has exactly one footer action.');
    assert.equal(virtualStyle.label, 'Remove');
    assert.ok(virtualStyle.icon, 'Virtual Remove includes a trash icon.');
    assert.deepEqual(virtualStyle, physicalStyle, 'Virtual Remove matches the physical controller action and footer styling.');

    await footer.getByRole('button', { name: 'Remove', exact: true }).click();
    await expect(page.locator('.resource-block[data-node-id="virtual-remove-target"]')).toHaveCount(0);
    await expect(page.locator(`.resource-block[data-node-id="${fixture.physicalId}"]`)).toBeVisible();
    const afterRemoval = await save();
    assert.equal(afterRemoval.document.virtualOutputs.length, 0, 'Removing the virtual controller updates the saved document.');
    assert.ok(afterRemoval.document.resources.some(resource => resource.id === fixture.physicalId), 'The physical resource remains in the document.');
    assert.ok(afterRemoval.assignments.some(assignment => assignment.resourceId === fixture.physicalId && assignment.sourceId === fixture.sourceId), 'The physical controller assignment is retained.');
  } finally {
    if (profileId) {
      const state = (await request({ command: 'profile_get', id: profileId })).profiles.document;
      await request({ command: 'profile_delete', id: profileId, expectedRevision: state.revision });
      await page.reload();
    }
    await browser.close();
  }
});
