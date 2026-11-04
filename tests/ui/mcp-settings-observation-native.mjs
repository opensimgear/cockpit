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
  const revoke = async id => invoke('mcp_credential_revoke', {
    id, expectedSettingsRevision: (await status()).settingsRevision,
  });
  try {
    await page.evaluate(`[...document.querySelectorAll('button')].find(button => button.textContent.trim()==='Cockpit').click()`);
    await until(() => page.evaluate(`(()=>{const button=[...document.querySelectorAll('.sidebar-link')].find(item=>item.textContent.trim()==='Settings'); if(!button)return false; button.click(); return true;})()`));
    await until(() => page.evaluate(`[...document.querySelectorAll('main button')].find(button => button.textContent.trim()==='AI integration')?.click() ?? !!${section}`));
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
    await revoke(credentialId);
    credentialId = undefined;
    await until(() => page.evaluate(`!${section}?.innerText.includes(${JSON.stringify(name)})`));
  } finally {
    try {
      if (breakpoint) await page.call('Debugger.removeBreakpoint', { breakpointId: breakpoint });
      await page.call('Debugger.disable');
      await page.evaluate('delete window.__mcpStatusReads');
      if (credentialId) await revoke(credentialId);
    } finally { page.close(); }
  }
});
