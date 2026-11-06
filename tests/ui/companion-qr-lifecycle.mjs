import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import test from 'node:test';

const origin = process.env.OSG_COMPANION_QR_UI_URL;
const form = '/apps/companion/src/lib/PairingForm.svelte';

test('restarting or closing QR pairing retires in-flight camera work', {
  skip: !origin, timeout: 30_000,
}, async () => {
  const require = createRequire(import.meta.url);
  const { chromium } = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');
  const browser = await chromium.launch({ headless: true, channel: process.env.OSG_BROWSER_CHANNEL });
  try {
    const page = await browser.newPage();
    const module = await page.request.get(new URL(form, origin).href);
    assert.ok(module.ok());
    const runtime = (await module.text()).match(/from ["']([^"']*\/svelte\.js[^"']*)["']/)?.[1];
    assert.ok(runtime, 'Expected the Svelte runtime from the dev server');
    const fixture = new URL('/__companion_qr_fixture', origin).href;
    await page.route(fixture, route => route.fulfill({
      contentType: 'text/html',
      body: `<!doctype html><main></main><script type="module">
        import { mount, unmount } from ${JSON.stringify(runtime)};
        import PairingForm from ${JSON.stringify(form)};
        const cameras = [], detections = [], stopped = [], commands = [];
        Object.defineProperty(navigator.mediaDevices, 'getUserMedia', {
          value: () => new Promise(resolve => {
            const id = cameras.length + 1;
            cameras.push(() => resolve({ getTracks: () => [{ stop: () => stopped.push(id) }] }));
          }),
        });
        Object.defineProperty(HTMLVideoElement.prototype, 'srcObject', { set() {} });
        HTMLVideoElement.prototype.play = async () => {};
        window.BarcodeDetector = class {
          detect() { return new Promise(resolve => detections.push(resolve)); }
        };
        window.__TAURI_INTERNALS__ = { invoke: async (...args) => { commands.push(args); } };
        const component = mount(PairingForm, { target: document.querySelector('main'), props: { onsubmitted() {} } });
        window.fixture = { cameras, detections, stopped, commands, close: () => unmount(component) };
      </script>`,
    }));
    await page.goto(fixture);
    const scan = page.getByRole('button', { name: 'Scan pairing QR code' });
    await scan.click();
    await scan.click();
    await page.waitForFunction(() => fixture.cameras.length === 2);
    await page.evaluate(() => fixture.cameras[1]());
    await page.waitForFunction(() => fixture.detections.length === 1);
    await page.evaluate(() => fixture.cameras[0]());
    assert.deepEqual(await page.evaluate(() => fixture.stopped), [1]);
    await page.evaluate(() => fixture.close());
    await page.evaluate(() => fixture.detections[0]([{ rawValue: 'retired QR result' }]));
    assert.deepEqual(await page.evaluate(() => ({
      stopped: fixture.stopped, commands: fixture.commands.length,
    })), { stopped: [1, 2], commands: 0 });
  } finally {
    await browser.close();
  }
});
