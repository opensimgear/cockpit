// Drives the already-running native dev Cockpit. No IPC or provider mocks.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');
const artifacts = resolve('.research/multi-viewport');

test('native companion editor authors, validates, previews and saves multiple viewports', { timeout: 120_000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222', { timeout: 15_000 });
  const checks = [], errors = [];
  try {
    const page = browser.contexts().flatMap(context => context.pages()).find(page => /127\.0\.0\.1:1420|localhost:1420/.test(page.url()));
    assert.ok(page, 'An already-running native development Cockpit is required');
    page.on('pageerror', error => errors.push(error.message));
    await page.reload();
    assert.equal(await page.evaluate(() => typeof window.__TAURI_INTERNALS__?.invoke), 'function');
    await page.getByRole('button', { name: 'Companions', exact: true }).click();
    await page.getByRole('button', { name: 'New panel', exact: true }).click();
    const editor = page.getByRole('region', { name: 'Panel editor', exact: true });
    const outlines = editor.locator('.viewport-outline');
    const viewportList = editor.locator('.viewport-list');
    const change = async (input, value) => { await input.fill(String(value)); await input.press('Tab'); };
    await viewportList.getByRole('button').first().click();
    await expect(outlines).toHaveCount(1);
    await expect(editor.getByLabel('Viewport width', { exact: true })).toHaveValue('1280');
    await expect(editor.getByLabel('Viewport height', { exact: true })).toHaveValue('800');
    checks.push('New panels start with one full-screen viewport');

    const panelName = 'Multi viewport acceptance ' + new Date().toISOString().replace(/[:.]/g, '-');
    await editor.getByRole('button', { name: 'Edit panel name', exact: true }).click();
    await change(editor.getByLabel('Panel name', { exact: true }), panelName);
    await editor.getByText('Screen size', { exact: true }).click();
    await editor.getByRole('combobox', { name: /^Companion screen/ }).selectOption('1920x1080');
    await editor.getByText('Viewports', { exact: true }).click();
    await editor.getByRole('button', { name: 'Add viewport', exact: true }).click();
    await expect(outlines).toHaveCount(2);
    await expect(editor.getByLabel('Viewport x', { exact: true })).toHaveValue('960');
    await expect(editor.getByLabel('Viewport width', { exact: true })).toHaveValue('960');
    const initialWidget = editor.locator('.widget').first();
    assert.equal(await initialWidget.evaluate(element => parseFloat(element.style.width)), 15);
    checks.push('Adding a viewport splits a full screen and scales its existing widgets');

    await change(editor.getByLabel('Viewport name', { exact: true }), 'Right instruments');
    await change(editor.getByLabel('Viewport x', { exact: true }), 900);
    await expect(editor.locator('.layout-error')).toContainText(/must not overlap/);
    await expect(editor.getByLabel('Viewport x', { exact: true })).toHaveValue('960');
    checks.push('Overlapping viewport edits show an error and preserve the valid layout');

    await change(editor.getByLabel('Viewport width', { exact: true }), 800);
    await change(editor.getByLabel('Viewport x', { exact: true }), 1120);
    await expect(editor.locator('.layout-error')).toHaveCount(0);
    await editor.getByRole('button', { name: 'Add viewport', exact: true }).click();
    await expect(outlines).toHaveCount(3);
    await expect(editor.getByLabel('Viewport x', { exact: true })).toHaveValue('960');
    await expect(editor.getByLabel('Viewport width', { exact: true })).toHaveValue('160');
    await editor.getByRole('button', { name: 'Remove viewport', exact: true }).click();
    await page.getByRole('alertdialog',{name:'Confirm deletion',exact:true}).getByRole('button',{name:'Delete',exact:true}).click();
    await expect(outlines).toHaveCount(2);
    await editor.getByRole('button', { name: 'Undo', exact: true }).click();
    await expect(outlines).toHaveCount(3);
    await editor.getByRole('button', { name: 'Redo', exact: true }).click();
    await expect(outlines).toHaveCount(2);
    checks.push('New viewports fill gaps; viewport removal supports undo and redo');

    if (!await viewportList.isVisible()) await editor.getByText('Viewports', { exact: true }).click();
    await viewportList.getByRole('button', { name: /^Select viewport Right instruments/ }).click();
    await editor.getByText('Widgets', { exact: true }).click();
    await editor.locator('.palette').getByRole('button', { name: 'Button', exact: true }).click();
    await expect(editor.locator('.widget')).toHaveCount(2);
    const added = editor.locator('.widget').last();
    const before = await added.getAttribute('style');
    await change(editor.getByLabel('X (px)', { exact: true }), 960);
    await expect(editor.locator('.layout-error')).toContainText(/inside.*viewport/);
    assert.equal(await added.getAttribute('style'), before);
    assert.ok(Number(await editor.getByLabel('X (px)', { exact: true }).inputValue()) > 1120);
    await change(editor.getByLabel('X (px)', { exact: true }), 1344);
    await expect(editor.locator('.layout-error')).toHaveCount(0);
    checks.push('Widgets are added to the selected viewport and cannot be placed across a gap');

    if (!await viewportList.isVisible()) await editor.getByText('Viewports', { exact: true }).click();
    await viewportList.getByRole('button', { name: /^Select viewport Right instruments/ }).click();
    await expect(editor.getByRole('button', { name: 'Remove viewport', exact: true })).toBeDisabled();
    await editor.getByRole('button', { name: 'Preview', exact: true }).click();
    await expect(outlines).toHaveCount(0);
    await expect(editor.locator('.widget')).toHaveCount(2);
    await editor.getByRole('button', { name: 'Stop preview', exact: true }).click();
    await expect(outlines).toHaveCount(2);
    checks.push('Preview keeps widgets in place and hides editing guides; occupied viewport removal is blocked');

    // Real pointer gesture: shrink the right viewport vertically, preserving its widgets.
    while (await page.getByRole('button', { name: 'Dismiss notification', exact: true }).count()) await page.getByRole('button', { name: 'Dismiss notification', exact: true }).first().click();
    const handle = editor.getByRole('button', { name: 'Resize viewport Right instruments', exact: true });
    if (await editor.getByRole('button',{name:'Close properties',exact:true}).isVisible()) await editor.getByRole('button',{name:'Close properties',exact:true}).click();
    await handle.scrollIntoViewIfNeeded();
    const handleBox = await handle.boundingBox();
    assert.ok(handleBox);
    await page.mouse.move(handleBox.x + handleBox.width / 2, handleBox.y + handleBox.height / 2);
    await page.mouse.down();
    await page.mouse.move(handleBox.x + handleBox.width / 2, handleBox.y + handleBox.height / 2 - 45, { steps: 6 });
    await page.mouse.up();
    await expect(editor.getByLabel('Viewport height', { exact: true })).not.toHaveValue('1080');
    await expect(editor.locator('.widget.outside-viewport')).toHaveCount(0);
    checks.push('Dragging the viewport resize handle updates its bounds and scales its widgets');
    await editor.locator('.panel-canvas').screenshot({ path: resolve(artifacts, 'multi-viewport-canvas.png') });

    await editor.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(editor.getByRole('button', { name: 'Save', exact: true })).toBeEnabled({ timeout: 30_000 });
    await expect(editor.locator('.layout-error')).toHaveCount(0);
    await expect(editor.getByText('Edit the panel layout.', { exact: true })).toBeVisible();
    await editor.getByRole('button', { name: 'Back to Panels', exact: true }).click();
    await page.getByRole('button', { name: new RegExp('^' + panelName) }).click();
    await expect(outlines).toHaveCount(2);
    await expect(viewportList.getByRole('button', { name: /^Select viewport Right instruments/ })).toBeVisible();
    await expect(editor.getByLabel('Screen width', { exact: true })).toHaveValue('1920');
    await expect(editor.locator('.widget')).toHaveCount(2);
    const native = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('gateway_request', { request: { command: 'companion_status', clientId: 'viewport-native-acceptance' } }));
    assert.equal(native.ok, true, JSON.stringify(native));
    checks.push('Multi-viewport panel saves through native publication validation and reopens with its screen and widgets');
    assert.deepEqual(errors, []);
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, panelName, checks, errors }, null, 2) + '\n');
  } catch (error) {
    const page = browser.contexts().flatMap(context => context.pages()).find(page => /:1420/.test(page.url()));
    await page?.screenshot({ path: resolve(artifacts, 'failure.png'), fullPage: true });
    throw error;
  } finally {
    await browser.close(); // Disconnect only; leave the real Cockpit running.
  }
});
