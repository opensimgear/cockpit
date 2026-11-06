import assert from 'node:assert/strict';
import { createServer } from 'node:net';
import test from 'node:test';
import { browserPage, nativePage, until } from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_COMPANION_LISTENER_OBSERVATION_BROWSER_URL;
const transport = browserUrl ? 'browser-dev' : 'native';

test(transport + ' Companion listener settings observe commits and retry readiness without idle reads', {
  skip: !browserUrl && process.env.OSG_COMPANION_LISTENER_OBSERVATION_NATIVE !== '1', timeout: 60000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const occupied = createServer();
  let configured = false, breakpoint;
  let browserReads = 0;
  const reads = () => browserUrl ? browserReads : page.evaluate('window.__listenerReads');
  const resetReads = async () => {
    if (browserUrl) browserReads = 0;
    else await page.evaluate('window.__listenerReads=0');
  };
  const stopTrace = page.listen(({method, params}) => {
    if (!browserUrl || method !== 'Network.requestWillBeSent' || !params.request.postData
      || new URL(params.request.url).pathname !== '/api/gateway/') return;
    const body = JSON.parse(params.request.postData);
    if (body.command === 'companion_inventory' && body.companion?.listenerOnly === true) browserReads++;
  });
  const native = async (command, companion = {}) => {
    const body = JSON.stringify({ command, companion, clientId: 'listener-observation-fixture' });
    const reply = await page.evaluate(browserUrl
      ? `fetch('/api/gateway/',{method:'POST',headers:{'Content-Type':'application/json','x-osg-client':'1'},body:${JSON.stringify(body)}}).then(response=>response.json())`
      : `window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${body}})`);
    assert.equal(reply.ok, true, reply.error);
    return reply.state.companions.listener;
  };
  const section = 'document.querySelector(\'[aria-label="Companion listener settings"]\')';
  const badge = () => page.evaluate(`${section}?.querySelector('[role="status"]')?.textContent`);
  const tab = id => page.evaluate(`document.getElementById(${JSON.stringify('settings-tab-'+id)})?.click()`);
  const hide = async () => {
    await tab('general');
    await until(() => page.evaluate("document.getElementById('settings-panel-companions')?.hidden"));
    await resetReads();
  };
  const openSettings = () => page.evaluate(`[...document.querySelectorAll('.sidebar-link')]
    .find(item => item.textContent.trim() === 'Settings')?.click()`);
  const pairForm = 'document.querySelector(\'[aria-label="Pair companion"]\')';
  const formButton = label => page.evaluate(`[...${pairForm}?.querySelectorAll('button') ?? []]
    .find(button => button.textContent.trim() === ${JSON.stringify(label)})?.click()`);
  const pairingDisabled = () => page.evaluate(`[...${pairForm}?.querySelectorAll('button') ?? []]
    .find(button => button.textContent.trim() === 'Generate pairing QR code')?.disabled`);
  const openMobilePairing = async () => {
    await page.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="Companions"]').click()`);
    await until(() => page.evaluate(`(() => {
      const button = [...document.querySelectorAll('section.companions button')]
        .find(button => button.textContent.trim() === 'Pair');
      if (!button || button.disabled) return false;
      button.click();
      return true;
    })()`));
    await until(() => page.evaluate(`(() => {
      const mobile = ${pairForm}?.querySelector('input[value="mobile"]');
      if (!mobile || mobile.disabled) return false;
      mobile.click();
      return true;
    })()`));
  };
  const disable = async () => native('companion_disable', {
    expectedSettingsRevision: (await native('companion_inventory')).settingsRevision,
  });
  try {
    await new Promise((resolve, reject) => {
      occupied.once('error', reject);
      occupied.listen(0, '127.0.0.1', resolve);
    });
    const initial = await native('companion_inventory');
    assert.equal(initial.configuredEnabled, false, 'Start with a disabled dev listener.');
    await until(() => page.evaluate(`(()=>{const settings=[...document.querySelectorAll('.sidebar-link')].find(item=>item.textContent.trim()==='Settings');if(!settings)return false;settings.click();return true;})()`));
    await until(() => page.evaluate("!!document.querySelector('#settings-tab-companions')"));
    await tab('companions');
    await until(badge, value => value === 'Disabled');
    await resetReads();
    if (browserUrl) {
      await page.call('Network.enable', {maxPostDataSize:65536});
    } else {
      await page.call('Debugger.enable');
      const target = await page.call('Runtime.evaluate', { expression: 'window.__TAURI_INTERNALS__.invoke' });
      ({ breakpointId: breakpoint } = await page.call('Debugger.setBreakpointOnFunctionCall', {
        objectId: target.result.objectId,
        condition: "(cmd==='gateway_request' && payload?.request?.command==='companion_inventory' && payload.request.companion?.listenerOnly===true && ++window.__listenerReads, false)",
      }));
    }
    // Let the initial observation and WebView focus refresh settle before measuring.
    await new Promise(resolve => setTimeout(resolve, 400));
    await resetReads();
    await new Promise(resolve => setTimeout(resolve, 2200));
    assert.equal(await reads(), 0);
    const waiting = await native('companion_enable', {
      address: '127.0.0.1', advertisedHost: '127.0.0.1',
      port: occupied.address().port, discovery: false, expectedSettingsRevision: initial.settingsRevision,
    });
    configured = true;
    assert.equal(waiting.configuredEnabled, true);
    assert.equal(waiting.enabled, false);
    await until(badge, value => value === 'Waiting to start');
    await until(reads, value => value > 0);
    await hide();
    await new Promise((resolve, reject) => occupied.close(error => error ? reject(error) : resolve()));
    await until(() => native('companion_inventory'), value => value.enabled);
    assert.equal(await reads(), 0, 'Hidden settings must not read listener state on retry');
    await tab('companions');
    await until(badge, value => value === 'Listening');
    await hide();
    await openMobilePairing();
    await until(pairingDisabled, value => value === false);
    await disable();
    configured = false;
    assert.equal(await reads(), 0, 'Hidden settings must not read listener state on disable');
    await until(pairingDisabled, value => value === true);
    await formButton('Cancel');
    await until(() => page.evaluate(`!${pairForm}`));
    await openMobilePairing();
    await until(pairingDisabled, value => value === true);
    await formButton('Cancel');
    await until(() => page.evaluate(`!${pairForm}`));
    await openSettings();
    await until(() => page.evaluate("!!document.querySelector('#settings-tab-companions')"));
    await tab('companions');
    await until(badge, value => value === 'Disabled');
  } finally {
    try {
      if (occupied.listening) await new Promise(resolve => occupied.close(resolve));
      if (configured) await disable();
      if (breakpoint) await page.call('Debugger.removeBreakpoint', { breakpointId: breakpoint });
      if (!browserUrl) {
        await page.call('Debugger.disable');
        await page.evaluate('delete window.__listenerReads');
      }
    } finally {
      stopTrace();
      await page.close();
    }
  }
});
