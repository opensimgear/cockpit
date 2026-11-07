import assert from 'node:assert/strict';
import {until} from '../../scripts/reactive/cdp.mjs';

export async function qualifyDcsTelemetryReopen(cdp, browserUrl, nativeValues, committed) {
  if (process.env.OSG_DCS_TELEMETRY_REOPEN !== '1') return;
  const channel = 'flight.airspeed.true';
  const previous = nativeValues[channel], current = 241.5;
  const subscribe = browserUrl ? '/api/observe/subscribe' : '/observation_subscribe';
  const unsubscribe = browserUrl ? '/api/observe/unsubscribe' : '/observation_unsubscribe';
  const gateway = browserUrl ? '/api/gateway/' : '/gateway_request';
  const opens = [], closes = [], reads = [], pending = new Map();
  const key = body => JSON.stringify([body.connectionId, body.requestId]);
  const stop = cdp.listen(({method,params}) => {
    if (method === 'Network.requestWillBeSent' && params.request.postData) {
      const path = new URL(params.request.url).pathname;
      if (![subscribe,unsubscribe,gateway].includes(path)) return;
      const body = JSON.parse(params.request.postData);
      if (path === subscribe && body.topic === 'games.telemetry') {
        opens.push(browserUrl ? key(body) : params.requestId);
      } else if (path === unsubscribe) {
        pending.set(params.requestId,browserUrl ? key(body) : body.subscriptionId);
      } else if (path === gateway) {
        const request = body.request ?? body;
        if (request.command === 'game_telemetry_inspect' && request.observationId) reads.push(request.observationId);
      }
    } else if (method === 'Network.responseReceived' && pending.has(params.requestId)) {
      const token = pending.get(params.requestId);
      pending.delete(params.requestId);
      if (params.response.status === 200) closes.push(token);
    }
  });
  const tab = name => cdp.evaluate(`Array.from(document.querySelectorAll('[aria-label="Debug sections"] [role="tab"]'))
    .find(button=>button.textContent.trim()===${JSON.stringify(name)}).click()`);
  const value = () => cdp.evaluate(`(()=>{
    const view=document.querySelector('[aria-label="Incoming game telemetry"]');
    if(!view||view.closest('[hidden]')||!view.querySelector('.source-status.live'))return null;
    return [...view.querySelectorAll('tbody tr')].find(row=>row.querySelector('code')?.textContent===${JSON.stringify(channel)})
      ?.querySelector('.channel-value')?.textContent.trim();
  })()`);
  const waitFor = expected => until(value, text=>text===String(expected));
  try {
    await cdp.call('Network.enable',{maxPostDataSize:65536});
    await cdp.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="Debug"]').click()`);
    await until(()=>cdp.evaluate(`(()=>{
      const select=document.querySelector('[aria-label="Telemetry game"]');
      const search=document.querySelector('[aria-label="Search game telemetry"]');
      if(!select?.querySelector('[value="dcs"]')||!search)return false;
      if(select.value!=='dcs'){select.value='dcs';select.dispatchEvent(new Event('change',{bubbles:true}));}
      search.value=${JSON.stringify(channel)};search.dispatchEvent(new Event('input',{bubbles:true}));return true;
    })()`));
    await waitFor(previous);
    assert.equal(opens.length,1);
    const first = reads.at(-1);
    assert.ok(first);
    await tab('Profile inspection');
    await until(()=>closes.includes(browserUrl ? opens[0] : first));
    const hiddenReads = reads.length;
    nativeValues[channel] = current;
    const capture = await committed(channel,current);
    assert.equal(capture.row.value,current);
    assert.equal(opens.length,1);
    assert.equal(reads.length,hiddenReads,'Hidden telemetry must not inspect a newly committed capture');
    const reopenedAt = performance.now();
    await tab('Incoming telemetry');
    await waitFor(current);
    const reopenMs = performance.now()-reopenedAt;
    const reopened = reads.slice(hiddenReads);
    assert.equal(opens.length,2);
    assert.ok(reopened.length&&reopened.every(token=>token!==first&&token===reopened[0]));
    await tab('Profile inspection');
    await until(()=>closes.includes(browserUrl ? opens[1] : reopened[0]));
    return {channel,previous,current,reopenMs,opens:2,retirements:2,hiddenReads:0};
  } finally {
    nativeValues[channel] = previous;
    stop();
    await cdp.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="Overview"]').click()`);
  }
}
