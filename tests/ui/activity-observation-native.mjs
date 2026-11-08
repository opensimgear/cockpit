import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import test from 'node:test';
import { browserPage, nativePage, until } from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_ACTIVITY_OBSERVATION_BROWSER_URL;
const transport = browserUrl ? 'browser development' : 'native';

test(transport + ' Activity reads committed changes and stays idle between them', {
  skip: !browserUrl && process.env.OSG_ACTIVITY_OBSERVATION_NATIVE !== '1', timeout: 120000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  let profile, breakpoint;
  const opens = [], closed = new Set(), baselines = new Set(), pending = new Map();
  const browserTokens = new Map(), browserReads = [];
  const reads = async (reset = false) => browserUrl
    ? (reset ? browserReads.splice(0) : [...browserReads])
    : page.evaluate(reset ? 'window.__activityReads.splice(0)' : 'window.__activityReads');
  const stopTrace = page.listen(event => {
    const {method, params} = event;
    if (method === 'Network.requestWillBeSent' && params.request.postData) {
      const path = new URL(params.request.url).pathname;
      if (browserUrl && path === '/api/gateway/') {
        const body = JSON.parse(params.request.postData);
        if (body.command === 'activity_query' && body.clientId !== 'activity-observation-fixture-read') {
          browserReads.push(body.committedOnly === true);
        }
        return;
      }
      const command = browserUrl ? ({
        '/api/observe/subscribe': 'observation_subscribe',
        '/api/observe/unsubscribe': 'observation_unsubscribe',
        '/api/observe/ack': 'observation_acknowledge',
      })[path] : path.split('/').at(-1);
      if (!['observation_subscribe','observation_unsubscribe','observation_acknowledge'].includes(command)) return;
      const body = JSON.parse(params.request.postData);
      if (command !== 'observation_subscribe' || body.topic === 'activity.log') pending.set(params.requestId, {command, body});
    } else if (method === 'Network.responseReceived') {
      const request = pending.get(params.requestId);
      if (request) request.accepted = params.response.status === 200;
    } else if (method === 'Network.loadingFinished' || method === 'Network.loadingFailed') {
      const request = pending.get(params.requestId);
      pending.delete(params.requestId);
      if (!request?.accepted || method === 'Network.loadingFailed') return;
      if (request.command === 'observation_unsubscribe') {
        closed.add(browserUrl ? browserTokens.get(request.body.requestId) : request.body.subscriptionId);
      } else if (request.command === 'observation_acknowledge' && request.body.delivery === '1') baselines.add(request.body.subscriptionId);
      else if (request.command === 'observation_subscribe') {
        void page.call('Network.getResponseBody', {requestId:params.requestId})
          .then(({body}) => {
            const value = JSON.parse(body);
            const token = browserUrl ? value.subscriptionId : value;
            opens.push(token);
            if (browserUrl) browserTokens.set(request.body.requestId, token);
          }).catch(() => {});
      }
    }
  });
  const callGateway = async (request, clientId = 'activity-observation-native') => {
    const body = JSON.stringify({...request, clientId});
    const response = await page.evaluate(browserUrl
      ? `fetch('/api/gateway/',{method:'POST',headers:{'Content-Type':'application/json','x-osg-client':'1'},body:${JSON.stringify(body)}}).then(response=>response.json())`
      : `window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${body}})`);
    assert.equal(response.ok, true, response.error);
    return response.state;
  };
  const section = 'document.querySelector(\'[aria-label="Activity log"]\')';
  const navigate = view => page.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="${view}"]').click()`);
  async function openActivity(expectedOpens) {
    await navigate('Activity');
    await until(async () => opens.length === expectedOpens && baselines.has(opens.at(-1)));
    await until(() => page.evaluate(`!!${section} && ${section}.getAttribute('aria-busy')!=='true'`));
    return opens.at(-1);
  }
  try {
    if (!browserUrl) await until(() => page.evaluate(`typeof window.__TAURI_INTERNALS__?.invoke === 'function'`));
    // Startup discovery commits real background Activity events. An idle
    // assertion is meaningful only once that producer has finished.
    await until(() => callGateway({command:'game_get_state'}),
      state => !['scanning','cancelling'].includes(state.games.discovery.status), 90000);
    await navigate('Overview');
    await until(() => page.evaluate(`!${section}`));
    await page.call('Network.enable', {maxPostDataSize:65536});
    if (!browserUrl) {
      await page.evaluate('window.__activityReads=[]');
      await page.call('Debugger.enable');
      const target = await page.call('Runtime.evaluate', { expression: 'window.__TAURI_INTERNALS__.invoke' });
      ({ breakpointId: breakpoint } = await page.call('Debugger.setBreakpointOnFunctionCall', {
        objectId: target.result.objectId,
        condition: "(cmd==='gateway_request' && payload?.request?.command==='activity_query' && payload.request.clientId!=='activity-observation-fixture-read' && window.__activityReads.push(payload.request.committedOnly===true), false)",
      }));
    }
    let subscription = await openActivity(1);
    assert.deepEqual(await reads(true), [true], 'The baseline triggers one initial read');
    await callGateway({ command: 'activity_query', committedOnly: true });
    assert.deepEqual(await reads(), [true]);
    await reads(true);
    await new Promise(resolve => setTimeout(resolve, 6500));
    assert.deepEqual(await reads(), [], 'Activity must not refresh while idle');
    const name = `Activity observation ${randomUUID()}`;
    profile = (await callGateway({ command: 'profile_create', name })).profiles.document;
    await until(() => page.evaluate(`${section}?.innerText.includes(${JSON.stringify(name)})`));
    await new Promise(resolve => setTimeout(resolve, 250));
    assert.deepEqual(await reads(), [true], 'One visible profile commit triggers one committed-only read');
    const renamed = `Activity while hidden ${randomUUID()}`;
    for (let cycle=0; cycle<3; cycle++) {
      await navigate('Overview');
      await until(async () => closed.has(subscription));
      if (cycle===0) {
        await reads(true);
        profile = (await callGateway({command:'profile_rename',id:profile.id,expectedRevision:profile.revision,name:renamed})).profiles.document;
        await until(() => callGateway({command:'activity_query', committedOnly:true, activityFilter:{search:renamed}}, 'activity-observation-fixture-read'),
          state => state.activity.events.length > 0);
        assert.deepEqual(await reads(), [], 'Hidden Activity must not query a committed mutation');
      }
      await reads(true);
      const next = await openActivity(cycle+2);
      assert.notEqual(next,subscription);
      subscription = next;
      await until(() => page.evaluate(`${section}?.innerText.includes(${JSON.stringify(renamed)})`));
      assert.deepEqual(await reads(), [true], 'Each reopened baseline triggers exactly one read');
    }
    const search = value => page.evaluate(`(()=>{
      const input=document.querySelector('[aria-label="Search activity"]');
      input.value=${JSON.stringify(value)};
      input.dispatchEvent(new Event('input',{bubbles:true}));
    })()`);
    await reads(true);
    await search(`No Activity match ${randomUUID()}`);
    await until(() => page.evaluate(`${section}?.innerText.includes('No matching activity')`));
    assert.deepEqual(await reads(true), [true]);
    await search('');
    await until(() => page.evaluate(`${section}?.innerText.includes(${JSON.stringify(renamed)})`));
    assert.deepEqual(await reads(), [true]);
    await navigate('Overview');
    await until(async () => closed.has(subscription));
    assert.equal(new Set(opens).size,4);
  } finally {
    try {
      stopTrace();
      if (!browserUrl) {
        if (breakpoint) await page.call('Debugger.removeBreakpoint', { breakpointId: breakpoint });
        await page.call('Debugger.disable');
        await page.evaluate('delete window.__activityReads');
      }
      if (profile) await callGateway({ command: 'profile_delete', id: profile.id, expectedRevision: profile.revision });
    } finally { await page.close(); }
  }
});
