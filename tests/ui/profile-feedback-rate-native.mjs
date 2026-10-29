// Observe the active native DCS run without changing its saved configuration.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir, writeFile} from 'node:fs/promises';

test('native DCS companion feedback stays healthy across mapping heartbeats', {
  skip: process.env.OSG_FEEDBACK_RATE_NATIVE !== '1', timeout: 180_000,
}, async () => {
  const require = createRequire(import.meta.url);
  const {chromium} = require(process.env.OSG_PLAYWRIGHT_MODULE
    ?? 'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222');
  const evidence = {startedAt: Date.now(), samples: 0, states: {}, degradations: {}, warnings: [], final: null};
  try {
    const page = browser.contexts().flatMap(context => context.pages()).find(candidate =>
      /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(candidate.url())
      && !candidate.url().includes('profile-activation-window'));
    assert.ok(page, 'Start the actual native development Cockpit with CDP enabled.');
    const native = async request => {
      const reply = await page.evaluate(request => window.__TAURI_INTERNALS__.invoke('gateway_request', {
        request: {...request, clientId: 'profile-feedback-rate-native'},
      }), request);
      assert.equal(reply.ok, true, request.command + ': ' + reply.error);
      return reply.state;
    };
    const status = async () => (await native({command: 'profile_status'})).profiles.profileSession;
    const initial = await status();
    assert.equal(initial.owner?.gameId, 'dcs', 'Apply the actual DCS profile before observing feedback.');
    const connected = async () => {
      const host = (await native({command: 'companion_status'})).companions.host;
      assert.ok(host.instances.some(instance => instance.profileOwned && instance.connected && instance.armed),
        'A connected and armed profile companion is required.');
    };
    await connected();
    const deadline = Date.now() + 120_000;
    while (Date.now() < deadline) {
      const current = await status();
      evidence.samples++;
      evidence.states[current.state] = (evidence.states[current.state] ?? 0) + 1;
      assert.deepEqual(current.owner, initial.owner, 'The native owner changed during the feedback soak.');
      assert.ok(['running', 'degraded'].includes(current.state), JSON.stringify(current));
      for (const reason of current.degradation) {
        evidence.degradations[reason] = (evidence.degradations[reason] ?? 0) + 1;
        assert.doesNotMatch(reason, /feedback:.*(?:rate|30\s*Hz)/i);
      }
      await new Promise(resolve => setTimeout(resolve, 200));
    }
    await connected();
    evidence.final = await status();
    const activity = (await native({command: 'activity_query', activityFilter: {
      since: evidence.startedAt, levels: ['error', 'warning'], limit: 100,
    }})).activity;
    evidence.warnings = activity.events;
    assert.ok(activity.total <= activity.events.length, 'Too many warnings to verify the full soak.');
    assert.ok(!activity.events.some(event => /feedback:.*(?:rate|30\s*Hz)/i.test(event.message)),
      JSON.stringify(activity.events));
  } finally {
    await mkdir('.research/activity-errors', {recursive: true});
    await writeFile('.research/activity-errors/native-soak.json', JSON.stringify(evidence, null, 2));
    await browser.close();
  }
});
