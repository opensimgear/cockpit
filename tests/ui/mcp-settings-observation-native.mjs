import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import test from 'node:test';
import { nativePage, until } from '../../scripts/reactive/cdp.mjs';

test('AI settings observes committed edits without idle status polling', {
  skip: process.env.OSG_MCP_SETTINGS_OBSERVATION_NATIVE !== '1', timeout: 60000,
}, async () => {
  const page = await nativePage();
  let credentialId, breakpoint;
  const invoke = (command, args = {}) => page.evaluate(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
  );
  const section = 'document.querySelector(\'[aria-label="AI integration settings"]\')';
  const status = () => invoke('mcp_settings_get');
  const selectTab = label => page.evaluate(`Array.from(document.querySelectorAll('[aria-label="Settings sections"] [role="tab"]'))
    .find(button => button.textContent.trim() === ${JSON.stringify(label)}).click()`);
  const revoke = async id => invoke('mcp_credential_revoke', {
    id, expectedSettingsRevision: (await status()).settingsRevision,
  });
  try {
    await page.evaluate(`Array.from(document.querySelectorAll('.sidebar-bottom button'))
      .find(button => (button.title || button.textContent.trim()) === 'Settings').click()`);
    await until(() => page.evaluate(`!!document.querySelector('[aria-label="Settings sections"]')`));
    await selectTab('AI integration');
    await until(() => page.evaluate(`!!${section}?.querySelector('[role="switch"]:not(:disabled)')`));
    await page.evaluate('window.__mcpStatusReads=0');
    await page.call('Debugger.enable');
    const target = await page.call('Runtime.evaluate', { expression: 'window.__TAURI_INTERNALS__.invoke' });
    ({ breakpointId: breakpoint } = await page.call('Debugger.setBreakpointOnFunctionCall', {
      objectId: target.result.objectId,
      condition: "(cmd==='mcp_settings_get' && ++window.__mcpStatusReads, false)",
    }));
    const initial = await status();
    assert.ok(await page.evaluate('window.__mcpStatusReads') >= 1);
    await new Promise(resolve => setTimeout(resolve, 250));
    await page.evaluate('window.__mcpStatusReads=0');
    await new Promise(resolve => setTimeout(resolve, 5500));
    assert.equal(await page.evaluate('window.__mcpStatusReads'), 0);
    const name = `Settings observation ${randomUUID()}`;
    const created = await page.evaluate(`window.__TAURI_INTERNALS__.invoke('mcp_credential_create', ${JSON.stringify({
      name, scope: 'read', expectedSettingsRevision: initial.settingsRevision,
    })}).then(value => value.credential.id)`);
    credentialId = created;
    await until(() => page.evaluate(`${section}?.innerText.includes(${JSON.stringify(name)})`));
    assert.ok(await page.evaluate('window.__mcpStatusReads') > 0);
    await selectTab('General');
    await until(() => page.evaluate(`!${section}`));
    const beforeRevoke = await status();
    await page.evaluate('window.__mcpStatusReads=0');
    await invoke('mcp_credential_revoke', {id: credentialId, expectedSettingsRevision: beforeRevoke.settingsRevision});
    credentialId = undefined;
    await new Promise(resolve => setTimeout(resolve, 250));
    assert.equal(await page.evaluate('window.__mcpStatusReads'), 0, 'Hidden AI settings must not read a committed change');
    await selectTab('AI integration');
    await until(() => page.evaluate(`!!${section}?.querySelector('[role="switch"]:not(:disabled)')`));
    assert.equal(await page.evaluate(`${section}.innerText.includes(${JSON.stringify(name)})`), false);
    assert.ok(await page.evaluate('window.__mcpStatusReads') > 0);
  } finally {
    try {
      if (breakpoint) await page.call('Debugger.removeBreakpoint', { breakpointId: breakpoint });
      await page.call('Debugger.disable');
      await page.evaluate('delete window.__mcpStatusReads');
      if (credentialId) await revoke(credentialId);
    } finally { page.close(); }
  }
});
