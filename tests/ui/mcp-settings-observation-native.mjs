import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import test from 'node:test';
import { browserPage, nativePage, until } from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_MCP_SETTINGS_OBSERVATION_BROWSER_URL;
const section = 'document.querySelector(\'[aria-label="AI integration settings"]\')';
const selectTab = (page, label) => page.evaluate(`Array.from(document.querySelectorAll('[aria-label="Settings sections"] [role="tab"]'))
  .find(button => button.textContent.trim() === ${JSON.stringify(label)}).click()`);
const openSettings = page => until(() => page.evaluate(`(() => {
  const button = Array.from(document.querySelectorAll('.sidebar-bottom button'))
    .find(button => (button.title || button.textContent.trim()) === 'Settings');
  if (!button) return false;
  button.click();
  return true;
})()`));

test('AI settings observes committed edits without idle status polling', {
  skip: !!browserUrl || process.env.OSG_MCP_SETTINGS_OBSERVATION_NATIVE !== '1', timeout: 60000,
}, async () => {
  const page = await nativePage();
  let credentialId, breakpoint;
  const invoke = (command, args = {}) => page.evaluate(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
  );
  const status = () => invoke('mcp_settings_get');
  const trace = () => page.evaluate('window.__mcpObservationTrace');
  const setHidden = hidden => page.evaluate(`(() => {
    if (${hidden}) Object.defineProperty(document, 'hidden', {configurable:true, value:true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const revoke = async id => invoke('mcp_credential_revoke', {
    id, expectedSettingsRevision: (await status()).settingsRevision,
  });
  try {
    await openSettings(page);
    await until(() => page.evaluate(`!!document.querySelector('[aria-label="Settings sections"]')`));
    const initial = await status();
    await page.evaluate('window.__mcpObservationTrace={events:[],closed:[]}');
    await page.call('Debugger.enable');
    const target = await page.call('Runtime.evaluate', { expression: 'window.__TAURI_INTERNALS__.invoke' });
    ({ breakpointId: breakpoint } = await page.call('Debugger.setBreakpointOnFunctionCall', {
      objectId: target.result.objectId,
      condition: `(cmd==='observation_subscribe' && payload?.topic==='settings.mcp' && window.__mcpObservationTrace.events.push('open'),
        cmd==='observation_unsubscribe' && window.__mcpObservationTrace.closed.push(payload.subscriptionId),
        cmd==='mcp_settings_get' && window.__mcpObservationTrace.events.push('read'), false)`,
    }));
    await selectTab(page, 'AI integration');
    const ready = () => until(() => page.evaluate(`!!${section}?.querySelector('[role="switch"]:not(:disabled)')`));
    await ready();
    await new Promise(resolve => setTimeout(resolve, 5500));
    assert.deepEqual((await trace()).events, ['open', 'read'], 'One read after the opening subscription and none while idle');
    const name = `Settings observation ${randomUUID()}`;
    const created = await page.evaluate(`window.__TAURI_INTERNALS__.invoke('mcp_credential_create', ${JSON.stringify({
      name, scope: 'read', expectedSettingsRevision: initial.settingsRevision,
    })}).then(value => ({id:value.credential.id, revision:value.status.settingsRevision}))`);
    credentialId = created.id;
    await until(() => page.evaluate(`${section}?.innerText.includes(${JSON.stringify(name)})`));
    const visible = await trace();
    assert.deepEqual(visible.events, ['open', 'read', 'read']);
    await setHidden(true);
    await until(trace, state => state.closed.length > visible.closed.length);
    assert.ok(await page.evaluate(`!!${section}?.querySelector('[role="switch"]:disabled')`));
    await invoke('mcp_credential_revoke', {id: credentialId, expectedSettingsRevision: created.revision});
    credentialId = undefined;
    await new Promise(resolve => setTimeout(resolve, 250));
    assert.deepEqual((await trace()).events, visible.events, 'Hidden AI settings must not read a committed change');
    await setHidden(false);
    await ready();
    assert.equal(await page.evaluate(`${section}.innerText.includes(${JSON.stringify(name)})`), false);
    assert.deepEqual((await trace()).events, [...visible.events, 'open', 'read']);
    const reopened = await trace();
    await selectTab(page, 'General');
    await until(trace, state => state.closed.length > reopened.closed.length);
    assert.equal(await page.evaluate(`!${section}`), true);
  } finally {
    try {
      if (breakpoint) await page.call('Debugger.removeBreakpoint', { breakpointId: breakpoint });
      await page.call('Debugger.disable');
      await setHidden(false);
      await page.evaluate('delete window.__mcpObservationTrace');
      if (credentialId) await revoke(credentialId);
    } finally { page.close(); }
  }
});

test('browser-dev AI settings displays its desktop-only boundary without requesting status', {
  skip: !browserUrl, timeout: 30000,
}, async () => {
  const page = await browserPage(browserUrl);
  const requests = [];
  const stop = page.listen(event => {
    if (event.method !== 'Network.requestWillBeSent') return;
    const { url, postData } = event.params.request;
    if (postData?.includes('settings.mcp') || postData?.includes('mcp_settings_get') || url.includes('mcp_settings_get')) requests.push(url);
  });
  try {
    await page.call('Network.enable');
    await openSettings(page);
    await selectTab(page, 'AI integration');
    await until(() => page.evaluate(`${section}?.innerText.includes('Open the Cockpit desktop app to manage AI connections.')`));
    assert.ok(await page.evaluate(`!!${section}?.querySelector('[role="switch"]:disabled')`));
    await selectTab(page, 'General');
    await selectTab(page, 'AI integration');
    await until(() => page.evaluate(`!!${section}`));
    assert.deepEqual(requests, []);
  } finally {
    stop();
    await page.close();
  }
});
