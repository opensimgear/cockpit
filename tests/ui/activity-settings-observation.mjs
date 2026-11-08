// Requires an isolated dev Cockpit database. The fixture raises retention and
// restores its original value after checking the live Settings view.
import assert from 'node:assert/strict';
import {setTimeout as delay} from 'node:timers/promises';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const url = process.env.OSG_ACTIVITY_SETTINGS_BROWSER_URL;
const enabled = process.env.OSG_R5_ISOLATED_DATABASE === '1'
  && (url || process.env.OSG_ACTIVITY_SETTINGS_NATIVE === '1');
const transport = url ? 'browser-dev' : 'native';
const readEndpoint = url ? '/api/gateway/' : '/gateway_request';

test(transport + ' Activity settings publishes one committed value without refresh queries', {
  skip: !enabled, timeout: 60000,
}, async () => {
  const page = url ? await browserPage(url) : await nativePage();
  const reads = [];
  const stopTrace = page.listen(({method, params}) => {
    if (method !== 'Network.requestWillBeSent' || !params.request.postData
        || new URL(params.request.url).pathname !== readEndpoint) return;
    const body = JSON.parse(params.request.postData);
    if ((body.request ?? body).command === 'activity_settings_get') reads.push('read');
  });
  const request = value => page.evaluate(`import('/src/lib/gateway.ts')
    .then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(value)}))`);
  const input = () => page.evaluate(`document.querySelector(
    '[aria-label="Activity retention days"]:not(:disabled)')?.valueAsNumber`);
  const deliveries = () => page.evaluate('window.__r5Retention?.days');
  let original, changed;
  try {
    await page.call('Network.enable', {maxPostDataSize: 65536});
    await until(() => page.evaluate(`(() => {
      const settings = [...document.querySelectorAll('.sidebar-bottom button')]
        .find(button => (button.title || button.textContent.trim()) === 'Settings');
      if (!settings) return false;
      settings.click();
      return true;
    })()`));
    original = await until(input, Number.isInteger);
    assert.ok(original < 3650, 'Use the disposable database with default retention');

    // Check that the trace captures explicit reads before claiming the view
    // stays quiet. The subscribed probe observes the same retained topic.
    const response = await request({command: 'activity_settings_get'});
    assert.equal(response.activitySettings.retentionDays, original);
    await until(() => reads.length === 1);
    reads.length = 0;
    await page.evaluate(`(async () => {
      const {SharedObservations} = await import('/src/lib/streams/shared.ts');
      const reader = new SharedObservations();
      const trace = window.__r5Retention = {days: [], stop: () => reader.close()};
      reader.subscribe('settings.activity', state => {
        if (state.status === 'ready') trace.days.push(state.frame.payload.retentionDays);
      });
    })()`);
    await until(deliveries, days => days?.length > 0);
    assert.deepEqual(await deliveries(), [original]);

    changed = original + 1;
    const committed = await request({command: 'activity_settings_save', retentionDays: changed});
    assert.equal(committed.activitySettings.retentionDays, changed);
    await until(input, days => days === changed);
    await until(deliveries, days => days?.includes(changed));
    await delay(5500);
    assert.deepEqual(await deliveries(), [original, changed]);
    assert.deepEqual(reads, []);
    console.log(JSON.stringify({transport, settingsCommits: 1, baselineDeliveries: 1,
      changeDeliveries: 1, refreshReads: 0, quietMs: 5500}));
  } finally {
    stopTrace();
    try {
      await page.evaluate('window.__r5Retention?.stop();delete window.__r5Retention');
      if (changed !== undefined) await request({command: 'activity_settings_save', retentionDays: original});
    } finally { await page.close(); }
  }
});
