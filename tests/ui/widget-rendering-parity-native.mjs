import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import test from 'node:test';
import { widgetReferenceHtml } from './widget-reference.mjs';

// A pure shared-renderer reference in the real Cockpit WebView. It neither
// injects app state nor claims live companion or device qualification.
test('capture the actual shared widget renderer at device resolution', { skip: process.env.OSG_WIDGETS_NATIVE_UI !== '1', timeout: 60000 }, async () => {
  const targets = await (await fetch(`${process.env.OSG_WIDGETS_CDP ?? 'http://127.0.0.1:9232'}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
  const target = targets.find(t => t.type === 'page' && new URL(t.url).origin === (process.env.OSG_WIDGETS_ORIGIN ?? 'http://127.0.0.1:1422'));
  assert.ok(target);
  const socket = new WebSocket(target.webSocketDebuggerUrl), pending = new Map(); let sequence = 0;
  await new Promise((yes, no) => { socket.addEventListener('open', yes, { once: true }); socket.addEventListener('error', no, { once: true }); });
  socket.addEventListener('message', event => { const msg = JSON.parse(event.data), item = pending.get(msg.id); if (!item) return; clearTimeout(item.timer); pending.delete(msg.id); msg.error ? item.reject(Error(msg.error.message)) : item.resolve(msg.result); });
  const send = (method, params = {}) => new Promise((resolve, reject) => { const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(Error(method + ' timed out')); }, 15000); pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params })); });
  const evaluate = async expression => { const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value; };
  const directory = '.research/widget-browser-reference'; await mkdir(directory, { recursive: true });
  try {
    await send('Emulation.setDeviceMetricsOverride', { width: 800, height: 480, deviceScaleFactor: 1, mobile: false });
    const html = await widgetReferenceHtml('active');
    await evaluate(`(()=>{const fixture=document.createElement('div');fixture.id='widget-reference';fixture.style.cssText='position:fixed;inset:0;z-index:2147483647;background:#0d1720;';fixture.innerHTML=${JSON.stringify(html)};document.body.append(fixture);})()`);
    assert.deepEqual(await evaluate(`(async()=>{const fonts=await Promise.all(['OSGPortableSans','OSGPortableMono'].map(face=>document.fonts.load('22px '+face)));await document.fonts.ready;return fonts.map(group=>group.some(font=>font.status==='loaded'));})()`), [true, true]);
    assert.equal(await evaluate(`document.querySelectorAll('#widget-reference .widget-artwork').length`), 10);
    await evaluate(`new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))`);
    const screenshot = await send('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: 800, height: 480, scale: 1 } });
    await writeFile(directory + '/active.png', Buffer.from(screenshot.data, 'base64'));
    console.log('Shared WidgetView pixel reference: ' + directory + '/active.png');
  } finally { await evaluate(`document.querySelector('#widget-reference')?.remove()`); await send('Emulation.clearDeviceMetricsOverride'); socket.close(); }
});
