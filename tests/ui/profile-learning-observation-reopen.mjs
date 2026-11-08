import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {isAbsolute} from 'node:path';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

// Launch Cockpit and this fixture with the same disposable OSG_SQLITE_DATABASE.
// An attached HID controller must already have a complete native input baseline.
const browserUrl = process.env.OSG_PROFILE_LEARNING_OBSERVATION_BROWSER_URL;
const native = process.env.OSG_PROFILE_LEARNING_OBSERVATION_NATIVE === '1';
const topic = 'profiles.learning';
const transport = browserUrl ? 'browser-dev' : 'native';

test(`${transport} reopens the passive learning reader on the exact cancelled capture`, {
  skip: !browserUrl && !native, timeout: 60_000,
}, async () => {
  assert.ok(!browserUrl || !native, 'Select one Cockpit transport');
  assert.equal(process.env.OSG_PROFILE_LEARNING_DISPOSABLE_DB, '1',
    'Use a disposable Cockpit database and set OSG_PROFILE_LEARNING_DISPOSABLE_DB=1');
  assert.ok(isAbsolute(process.env.OSG_SQLITE_DATABASE ?? ''), 'Set the disposable Cockpit database path');
  const endpoint = new URL(browserUrl ?? process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222');
  assert.ok(endpoint.protocol === 'http:' && endpoint.hostname === '127.0.0.1'
    && !endpoint.username && !endpoint.password, 'Use a private loopback Cockpit endpoint');
  const page = browserUrl ? await browserPage(endpoint.href) : await nativePage(endpoint.origin);
  const request = value => page.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest}) =>
    gatewayRequest(${JSON.stringify(value)}))`);
  const clientId = randomUUID();
  const cancel = async learnId => {
    const payload = {command:'profile_input_learn_cancel', clientId, learnId};
    const reply = await page.evaluate(browserUrl
      ? `fetch('/api/gateway/', {method:'POST', headers:{'Content-Type':'application/json', 'X-OSG-Client':'1'},
          body:${JSON.stringify(JSON.stringify(payload))}}).then(response => response.json())`
      : `window.__TAURI_INTERNALS__.invoke('gateway_request', {request:${JSON.stringify(payload)}})`);
    assert.equal(reply.ok, true, reply.error);
    return reply.state.profileLearning;
  };

  const subscriptions = [];
  const completedCloses = [];
  const pendingCloses = new Map();
  const stopTrace = page.listen(({method, params}) => {
    if (method === 'Network.responseReceived' && pendingCloses.has(params.requestId)) {
      pendingCloses.get(params.requestId).status = params.response.status;
    }
    if (method === 'Network.loadingFinished' && pendingCloses.has(params.requestId)) {
      const {body, status} = pendingCloses.get(params.requestId);
      if (status >= 200 && status < 300) completedCloses.push(body);
      pendingCloses.delete(params.requestId);
    }
    if (method !== 'Network.requestWillBeSent' || !params.request.postData) return;
    const path = new URL(params.request.url).pathname;
    if (path === '/api/observe/subscribe') {
      const body = JSON.parse(params.request.postData);
      if (body.topic === topic) subscriptions.push(body);
    }
    if (path === (browserUrl ? '/api/observe/unsubscribe' : '/observation_unsubscribe')) {
      pendingCloses.set(params.requestId, {body:JSON.parse(params.request.postData)});
    }
  });
  const open = () => page.evaluate(`(async () => {
    const {SharedObservations} = await import('/src/lib/streams/shared.ts');
    const probe = window.__learningReopen ??= {observations:new SharedObservations(), count:0};
    probe.current = undefined;
    probe.stop = probe.observations.subscribe(${JSON.stringify(topic)}, state => {
      probe.current = state;
      if (state.status === 'ready') probe.count++;
    });
  })()`);
  const snapshot = () => page.evaluate(`(() => {
    const probe = window.__learningReopen;
    return {state:probe?.current, count:probe?.count, id:probe?.observations.subscriptionId(${JSON.stringify(topic)})};
  })()`);
  const current = async predicate => until(async () => {
    const value = await snapshot();
    assert.ok(!['lost', 'error'].includes(value.state?.status), value.state?.error ?? 'Learning stream lost');
    return value;
  }, value => value.id && value.state?.status === 'ready' && predicate(value.state.frame));

  let learnId;
  try {
    await until(() => page.evaluate(browserUrl ? 'document.readyState === "complete"'
      : 'document.readyState === "complete" && typeof window.__TAURI_INTERNALS__?.invoke === "function"'));
    const idle = async () => {
      const session = (await request({command:'profile_status'})).profiles.profileSession;
      assert.equal(session.state, 'stopped', 'The disposable Cockpit must remain stopped');
      assert.equal(session.owner, null);
    };
    await idle();
    const source = await until(async () => {
      for (let offset = 0; ; ) {
        const catalogue = (await request({command:'profile_resources_list', offset, limit:128})).profileResources;
        const match = catalogue.resources.find(row => row.resourceType === 'host-controller' && row.availability === 'online'
          && row.connections.length === 1 && row.connections[0].route.transport === 'hid'
          && row.connections[0].route.baselineReady);
        if (match) return match;
        offset += catalogue.resources.length;
        if (offset >= catalogue.total) return null;
        assert.ok(catalogue.resources.length, 'Controller resource catalogue ended early');
      }
    }).catch(cause => { throw new Error('An attached HID controller with a complete baseline is required', {cause}); });

    await page.call('Network.enable', {maxPostDataSize:65536});
    await open();
    const baseline = await current(frame => frame.kind === 'snapshot');
    assert.equal(baseline.state.frame.source, 'profile-input-learning');
    const started = (await request({command:'profile_input_learn_start', resourceId:source.id,
      generation:source.generation, capabilityRevision:source.capabilityRevision})).profileLearning;
    learnId = started.learnId;
    assert.equal(started.phase, 'listening');
    assert.equal(started.resourceId, source.id);
    const listening = await current(frame => BigInt(frame.revision) > BigInt(baseline.state.frame.revision));
    assert.equal(listening.id, baseline.id);
    assert.equal((await request({command:'profile_input_learn_status', learnId})).profileLearning.phase, 'listening');

    await page.evaluate('window.__learningReopen.stop()');
    const opened = subscriptions[0];
    await until(() => completedCloses.some(body => browserUrl
      ? opened && body.connectionId === opened.connectionId && body.requestId === opened.requestId
      : body.subscriptionId === listening.id));
    const closed = await snapshot();
    assert.equal(closed.id, undefined);
    const stopped = await cancel(learnId);
    learnId = undefined;
    assert.equal(stopped.learnId, started.learnId);
    assert.equal(stopped.phase, 'stopped');
    assert.deepEqual(stopped.candidates, []);
    assert.equal((await snapshot()).count, closed.count, 'The closed reader must not receive cancellation');

    await open();
    const reopened = await current(frame => frame.kind === 'snapshot'
      && BigInt(frame.revision) > BigInt(closed.state.frame.revision));
    assert.notEqual(reopened.id, listening.id);
    assert.equal(reopened.state.frame.session, listening.state.frame.session);
    assert.equal(reopened.state.frame.generation, listening.state.frame.generation);
    assert.deepEqual((await request({command:'profile_input_learn_status', learnId:stopped.learnId})).profileLearning, stopped);
    await idle();
    console.log(JSON.stringify({transport, learningBaseline:baseline.state.frame.revision,
      listeningRevision:listening.state.frame.revision, reopenedRevision:reopened.state.frame.revision,
      retiredExactReader:true, reopenedStopped:true, candidates:stopped.candidates.length}));
  } finally {
    stopTrace();
    try {
      try {
        await page.evaluate('window.__learningReopen?.observations.close(); delete window.__learningReopen');
      } finally {
        if (learnId) await cancel(learnId);
      }
    } finally { await page.close(); }
  }
});
