import assert from 'node:assert/strict';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_RECOVERY_OBSERVATION_BROWSER_URL;
const topic = 'profiles.retired-recovery';
const subscribe = browserUrl ? '/api/observe/subscribe' : '/observation_subscribe';
const unsubscribe = browserUrl ? '/api/observe/unsubscribe' : '/observation_unsubscribe';
const gateway = browserUrl ? '/api/gateway/' : '/gateway_request';

test('Debug recovery reads once from its baseline and retires the exact reader', {
  skip: !browserUrl && process.env.OSG_RECOVERY_OBSERVATION_NATIVE !== '1',
  timeout: 90_000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const closed = [];
  let paused, reads = 0, traceError, openingRequest, subscriptionId;
  const stopTrace = page.listen(({method, params}) => {
    if (method === 'Network.loadingFinished' && params.requestId === openingRequest) {
      void page.call('Network.getResponseBody', {requestId:params.requestId}).then(({body}) => {
        const result = JSON.parse(body);
        subscriptionId = browserUrl ? result.subscriptionId : result;
      }).catch(error => { traceError = error; });
      return;
    }
    if (!params.request?.postData) return;
    const path = new URL(params.request.url).pathname;
    if (method === 'Fetch.requestPaused') {
      const body = JSON.parse(params.request.postData);
      if (path === subscribe && body.topic === topic) paused = params;
      else void page.call('Fetch.continueRequest', {requestId:params.requestId}).catch(error => { traceError = error; });
    } else if (method === 'Network.requestWillBeSent') {
      if (path === subscribe && JSON.parse(params.request.postData).topic === topic) {
        openingRequest = params.requestId;
      } else if (path === gateway) {
        const body = JSON.parse(params.request.postData);
        if ((body.request ?? body).command === 'profile_retired_recovery_status') reads++;
      } else if (path === unsubscribe) {
        closed.push(JSON.parse(params.request.postData));
      }
    }
  });
  const debugTab = label => page.evaluate(`[...document.querySelectorAll('[aria-label="Debug sections"] [role="tab"]')]
    .find(button => button.textContent.trim() === ${JSON.stringify(label)})?.click()`);
  const hidden = value => page.evaluate(`(() => {
    if (${value}) Object.defineProperty(document, 'hidden', {configurable:true, value:true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const settle = () => new Promise(resolve => setTimeout(resolve, 200));
  try {
    await until(() => page.evaluate(`(() => {
      const button = document.querySelector('nav[aria-label="Main navigation"] button[title="Debug"]');
      if (!button) return false;
      button.click();
      return true;
    })()`));
    await until(() => page.evaluate(`!!document.querySelector('[aria-label="Debug sections"]')`));
    await debugTab('Incoming telemetry');
    await page.call('Network.enable', {maxPostDataSize:65536});
    for (let cycle = 0; cycle < 3; cycle++) {
      paused = undefined;
      openingRequest = undefined;
      subscriptionId = undefined;
      const before = reads;
      await page.call('Fetch.enable', {patterns:[{urlPattern:'*' + subscribe}]});
      let token;
      try {
        if (cycle === 1) await hidden(false);
        else await debugTab('Runtime & recovery');
        const open = await until(() => paused);
        await settle();
        if (traceError) throw traceError;
        assert.equal(reads, before, 'Recovery must wait for the observation baseline');
        const body = JSON.parse(open.request.postData);
        if (browserUrl) token = [body.connectionId, body.requestId];
        await page.call('Fetch.continueRequest', {requestId:open.requestId});
        paused = undefined;
        await until(() => {
          if (traceError) throw traceError;
          return reads > before && typeof subscriptionId === 'string';
        });
        await settle();
        assert.equal(reads, before + 1, 'One status read per opened baseline');
      } finally {
        if (paused) await page.call('Fetch.continueRequest', {requestId:paused.requestId}).catch(() => {});
        await page.call('Fetch.disable');
      }
      const beforeClose = closed.length;
      if (cycle === 0) await hidden(true);
      else await debugTab('Incoming telemetry');
      await until(() => closed.slice(beforeClose).some(entry => browserUrl
        ? entry.connectionId === token[0] && entry.requestId === token[1]
        : entry.subscriptionId === subscriptionId));
      await settle();
      assert.equal(reads, before + 1, 'Hidden recovery tab must stay quiet');
    }
  } finally {
    await hidden(false).catch(() => {});
    stopTrace();
    await page.close();
  }
});
