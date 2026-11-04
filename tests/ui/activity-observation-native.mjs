import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import test from 'node:test';
import { nativePage, until } from '../../scripts/reactive/cdp.mjs';

test('Activity reads committed changes and stays idle between them', {
  skip: process.env.OSG_ACTIVITY_OBSERVATION_NATIVE !== '1', timeout: 60000,
}, async () => {
  const page = await nativePage();
  let profile, breakpoint;
  const native = async request => {
    const response = await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({ ...request, clientId: 'activity-observation-native' })}})`);
    assert.equal(response.ok, true, response.error);
    return response.state;
  };
  const section = 'document.querySelector(\'[aria-label="Activity log"]\')';
  try {
    await page.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="Activity"]').click()`);
    await until(() => page.evaluate(`!!${section} && ${section}.getAttribute('aria-busy')!=='true'`));
    await page.evaluate('window.__activityReads=[]');
    await page.call('Debugger.enable');
    const target = await page.call('Runtime.evaluate', { expression: 'window.__TAURI_INTERNALS__.invoke' });
    ({ breakpointId: breakpoint } = await page.call('Debugger.setBreakpointOnFunctionCall', {
      objectId: target.result.objectId,
      condition: "(cmd==='gateway_request' && payload?.request?.command==='activity_query' && window.__activityReads.push(payload.request.committedOnly===true), false)",
    }));
    await native({ command: 'activity_query', committedOnly: true });
    assert.deepEqual(await page.evaluate('window.__activityReads'), [true]);
    await page.evaluate('window.__activityReads.length=0');
    await new Promise(resolve => setTimeout(resolve, 6500));
    assert.deepEqual(await page.evaluate('window.__activityReads'), [], 'Activity must not refresh while idle');
    const name = `Activity observation ${randomUUID()}`;
    profile = (await native({ command: 'profile_create', name })).profiles.document;
    await until(() => page.evaluate(`${section}?.innerText.includes(${JSON.stringify(name)})`));
    const reads = await page.evaluate('window.__activityReads');
    assert.ok(reads.length > 0 && reads.every(Boolean), 'The committed event must trigger a passive query');
  } finally {
    try {
      if (breakpoint) await page.call('Debugger.removeBreakpoint', { breakpointId: breakpoint });
      await page.call('Debugger.disable');
      await page.evaluate('delete window.__activityReads');
      if (profile) await native({ command: 'profile_delete', id: profile.id, expectedRevision: profile.revision });
    } finally { page.close(); }
  }
});
