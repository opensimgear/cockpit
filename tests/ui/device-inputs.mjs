// Actual controller details in the running dev cockpit, with read-only IPC fixtures.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
const require = createRequire(import.meta.url), module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');
const origin = process.env.OSG_DEVICES_UI_URL ?? 'http://127.0.0.1:1420';
const artifacts = resolve('.research/device-inputs');

test('controller details show every input in a compact, stable, accessible matrix', { timeout: 60_000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : {}) });
  const page = await browser.newPage({ viewport: { width: 1200, height: 1000 } });
  const compiled = await (await fetch(origin + '/src/lib/HostControllers.svelte')).text();
  const runtime = compiled.match(/from ["']([^"']*\/svelte\.js[^"']*)["']/)[1];
  const source = { id: 'matrix-controller', name: 'Test controller', online: true, role: 'joystick', backend: 'windows-raw-input',
    vendorId: 1, productId: 2, baselineReady: true, generation: '1', sequence: '1', controlCount: 68 };
  const control = (id, kind, usage, normalized = 0) => ({ capability: { id, kind, usage, usagePage: 1,
    reportId: 1, collection: 2, dataIndex: Number(id.replace(/\D/g, '')) || 0, min: 0, max: kind === 'button' ? 1 : 32767 },
    value: normalized === undefined ? null : { native: normalized, normalized } });
  const controls = [control('axis1', 'axis', 48, 12000), control('axis2', 'axis', 49, 24000),
    control('hat1', 'hat', 57, 2), control('diagnostic1', 'diagnostic', 99, 42),
    ...Array.from({ length: 64 }, (_, index) => control(`button${index + 1}`, 'button', index + 1, index === 1 ? 1 : 0))];
  const requests = [], errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.exposeFunction('__inputsInvoke', async (command, args) => {
    assert.equal(command, 'gateway_request');
    const request = args.request; requests.push(request);
    let controllers = { offset: 0, total: 1, limit: 32, sources: [source] };
    if (request.command === 'controller_inspect') controllers = { ...controllers, source, offset: request.offset, total: controls.length,
      controls: controls.slice(request.offset, request.offset + 32) };
    else if (request.command === 'controller_visibility_status') controllers.visibility = { platform: 'windows', enabled: false,
      intent: { mode: 'visible' }, modes: [{ mode: 'visible', label: 'Visible', supported: true }], global: { inspected: false }, reason: 'Read-only fixture.', recovery: { required: false } };
    else assert.equal(request.command, 'controller_list');
    return { ok: true, state: { controllers } };
  });
  await page.addInitScript(() => { window.isTauri = true; window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.__inputsInvoke(command, args) }; });
  await page.route(origin + '/__inputs_test', route => route.fulfill({ contentType: 'text/html', body: `<html><body><div class="cockpit"><main style="height:100dvh;max-width:1100px;margin:auto;padding:20px"><div id="test"></div></main></div><script type="module">import {mount} from '${runtime}';import HostControllers from '/src/lib/HostControllers.svelte';import '/src/app.css';mount(HostControllers,{target:document.querySelector('#test'),props:{initialSourceId:'matrix-controller',detailsOnly:true}});</script></body></html>` }));
  try {
    await page.goto(origin + '/__inputs_test');
    const matrix = page.getByRole('region', { name: 'Buttons', exact: true }), analog = page.getByRole('region', { name: 'Axes, hats and other inputs', exact: true });
    await expect(matrix.getByRole('button')).toHaveCount(64);
    await expect(page.getByRole('region', { name: 'Device visibility', exact: true })).toContainText('Read-only fixture.');
    await expect(analog.getByRole('meter')).toHaveCount(2);
    await expect(analog).toContainText('Hat'); await expect(analog).toContainText('E'); await expect(analog).toContainText('Raw 42');
    await expect(page.getByRole('button', { name: /^(Previous|Next) controls$/ })).toHaveCount(0);
    assert.ok(requests.some(request => request.command === 'controller_inspect' && request.offset === 64));
    const left = await analog.boundingBox(), right = await matrix.boundingBox();
    assert.ok(left.x + left.width <= right.x, 'Axes and hats sit to the left of the button matrix.');
    const slots = () => matrix.locator('.button-slot').evaluateAll(nodes => nodes.map(node => { const r = node.getBoundingClientRect(), parent = node.parentElement.getBoundingClientRect(); return { x: r.x - parent.x, y: r.y - parent.y, width: r.width, height: r.height }; }));
    const before = await slots(), layoutBefore = await page.locator('.controller-identity').boundingBox();
    const first = matrix.locator('[data-control-id="button1"]');
    await expect(first.locator('svg')).toHaveCount(0);
    assert.equal((await first.boundingBox()).width, 40);
    await page.screenshot({ path: resolve(artifacts, 'compact.png'), fullPage: true });
    await first.hover();
    assert.ok((await first.boundingBox()).width > 40);
    await expect(first).toContainText('Report 1 · collection 2 · field 1');
    await expect(first).toContainText('Native 0 · range 0…1');
    await expect(first).toHaveAccessibleDescription(/Report 1 · collection 2 · field 1/);
    assert.deepEqual(await slots(), before);
    assert.deepEqual(await page.locator('.controller-identity').boundingBox(), layoutBefore);
    controls[4].value = { native: 1, normalized: 1 };
    await expect(first).toHaveAttribute('aria-label', 'Button 1: Pressed');
    await expect(first).toHaveClass(/pressed/);
    await page.screenshot({ path: resolve(artifacts, 'expanded.png'), fullPage: true });
    await page.mouse.move(0, 0);
    await first.focus(); await expect(first.locator('.button-information')).toBeVisible();
    await first.press('Escape'); await expect(first.locator('.button-information')).not.toBeVisible();
    for (const width of [520, 375]) {
      await page.setViewportSize({ width, height: 1000 });
      const narrowBefore = await slots();
      for (const tile of [matrix.getByRole('button').first(), matrix.getByRole('button').last()]) {
        await tile.hover();
        const box = await tile.boundingBox();
        assert.ok(box.x >= 0 && box.x + box.width <= width, 'Expanded readout remains inside the viewport.');
        assert.deepEqual(await slots(), narrowBefore);
        await page.mouse.move(0, 0);
      }
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      await page.screenshot({ path: resolve(artifacts, `compact-${width}.png`), fullPage: true });
    }
    assert.deepEqual(errors, []);
    assert.ok(requests.every(request => ['controller_list', 'controller_inspect', 'controller_visibility_status'].includes(request.command)));
  } catch (error) {
    await page.screenshot({ path: resolve(artifacts, 'failure.png'), fullPage: true });
    throw error;
  } finally { await browser.close(); }
});
