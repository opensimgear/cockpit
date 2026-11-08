// Exercises the production Devices -> controller settings -> Live inputs preview.
import assert from 'node:assert/strict';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_CONTROLLER_PREVIEW_VISIBILITY_BROWSER_URL;
const privateDatabase = process.env.OSG_CONTROLLER_PREVIEW_VISIBILITY_PRIVATE_DATABASE === '1';
const transport = browserUrl ? 'browser-dev' : 'native';
const topicPrefix = 'controllers.input.';
const endpoints = new Map([
  ['/observation_subscribe', 'subscribe'], ['/observation_unsubscribe', 'unsubscribe'],
  ['/observation_acknowledge', 'ack'], ['/api/observe/subscribe', 'subscribe'],
  ['/api/observe/unsubscribe', 'unsubscribe'], ['/api/observe/ack', 'ack'],
]);
const openKey = body => JSON.stringify([body.connectionId, body.requestId]);

test(transport + ' controller preview retires while hidden and resumes from a fresh input baseline', {
  skip: !browserUrl && process.env.OSG_CONTROLLER_PREVIEW_VISIBILITY_NATIVE !== '1',
  timeout: 70_000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const pending = new Map(), subscriptions = [], unsubscriptions = [], acknowledgements = [];
  const errors = [], inspectionReads = [];
  let sourceId, original, renamed, restoreMetadata = false;
  const evaluate = source => page.evaluate(source);
  const request = body => evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest}) =>
    gatewayRequest(${JSON.stringify(body)}))`);
  const navigate = label => until(() => evaluate(`(() => {
    const button=[...document.querySelectorAll('nav[aria-label="Main navigation"] button')]
      .find(value=>(value.title||value.textContent.trim())===${JSON.stringify(label)}&&!value.disabled);
    if(!button)return false;button.click();return true;
  })()`));
  const setHidden = hidden => evaluate(`(() => {
    Object.defineProperty(document,'hidden',{configurable:true,value:${hidden}});
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const liveCard = 'section[aria-label="Selected host controller"]';
  const liveInputsVisible = () => evaluate(`(() => {
    const card=document.querySelector(${JSON.stringify(liveCard)});
    return [...(card?.querySelectorAll('.controls-heading h3') ?? [])].some(heading=>heading.textContent.trim()==='Live inputs')
      &&card.querySelector('.live-input-layout');
  })()`);
  const saveName = name => request({command: 'controller_update', sourceId, name, role: original.role});

  const stopTrace = page.listen(({method, params}) => {
    if (method === 'Network.requestWillBeSent' && params.request.postData) {
      const path = new URL(params.request.url).pathname;
      if (path === '/gateway_request' || path === '/api/gateway/') {
        try {
          const body = JSON.parse(params.request.postData);
          const value = body.request ?? body;
          if (value.command === 'controller_inspect') inspectionReads.push(value.sourceId);
        } catch {}
      }
      const operation = endpoints.get(path);
      if (!operation) return;
      try { pending.set(params.requestId, {operation, body: JSON.parse(params.request.postData)}); }
      catch (error) { errors.push(error.message); }
      return;
    }
    const entry = pending.get(params.requestId);
    if (!entry) return;
    if (method === 'Network.responseReceived') entry.status = params.response.status;
    if (method !== 'Network.loadingFinished' && method !== 'Network.loadingFailed') return;
    const {operation, body, status} = entry;
    if (status !== 200 || method !== 'Network.loadingFinished') {
      if (body.topic?.startsWith(topicPrefix) && !params.canceled) errors.push(`${operation} failed (${status})`);
      pending.delete(params.requestId);
      return;
    }
    if (operation === 'subscribe' && body.topic === topicPrefix + sourceId) {
      void page.call('Network.getResponseBody', {requestId: params.requestId}).then(({body: response}) => {
        const reply = JSON.parse(response);
        const token = browserUrl ? reply.subscriptionId : reply;
        const row = {token, key: browserUrl ? openKey(body) : token, topic: body.topic};
        subscriptions.push(row);
      }).catch(error => errors.push(error.message)).finally(() => pending.delete(params.requestId));
      return;
    }
    if (body.topic && body.topic.startsWith(topicPrefix) && body.topic !== topicPrefix + sourceId) {
      errors.push(`Unexpected controller input topic ${body.topic}`);
    }
    if (operation === 'unsubscribe' && subscriptions.some(row => row.key === (browserUrl ? openKey(body) : body.subscriptionId))) {
      unsubscriptions.push({key: browserUrl ? openKey(body) : body.subscriptionId, token: body.subscriptionId});
    } else if (operation === 'ack') {
      acknowledgements.push({token: body.subscriptionId, delivery: String(body.delivery)});
    }
    pending.delete(params.requestId);
  });

  try {
    await navigate('Overview');
    await page.call('Network.enable', {maxPostDataSize: 65_536});
    await page.call('Page.reload');
    await until(async () => {
      try { return await evaluate('document.readyState === "complete" && !!document.querySelector("nav")'); }
      catch { return false; }
    });
    await setHidden(false);
    await navigate('Devices');
    await until(() => evaluate('!!document.querySelector("[aria-label=\\"Devices presentation\\"]")'));
    await evaluate(`document.querySelector('[aria-label="Devices presentation"] button[aria-label="List"]')?.click()`);
    const tileId = await until(() => evaluate('document.querySelector(\'.component-tile[data-component-id^="controller:"]\')?.dataset.componentId'));
    sourceId = tileId.slice('controller:'.length);
    await evaluate(`document.querySelector(${JSON.stringify(`[data-component-id="${tileId}"]`)})?.click()`);
    await until(liveInputsVisible);
    assert.ok((await evaluate(`document.querySelector(${JSON.stringify(liveCard + ' .controller-identity dd code')})?.textContent`)) === sourceId,
      'The selected production controller settings must match the tile identity.');
    await until(() => subscriptions.length === 1 && acknowledgements.some(row => row.token === subscriptions[0]?.token && row.delivery === '1'));
    const first = subscriptions[0];
    assert.equal(first.topic, topicPrefix + sourceId);
    const cardBefore = await evaluate(`(() => {
      const card=document.querySelector(${JSON.stringify(liveCard)});
      const input=card?.querySelector('.metadata input');
      input.value='Unsaved preview draft';input.dispatchEvent(new Event('input',{bubbles:true}));
      window.__controllerPreviewVisibility={card,input};
      return input?.value;
    })()`);
    assert.equal(cardBefore, 'Unsaved preview draft');
    if (privateDatabase) {
      const inventory = await request({command: 'controller_list', offset: 0});
      original = inventory.controllers.sources.find(source => source.id === sourceId);
      assert.ok(original, 'The selected controller must exist in the live inventory read model.');
    }

    await setHidden(true);
    await until(() => unsubscriptions.some(row => row.key === first.key && (!browserUrl ? row.token === first.token : true)), Boolean, 5_000)
      .catch(cause => { throw new Error('Hidden controller preview retained its input subscription.', {cause}); });
    assert.equal(await evaluate(`window.__controllerPreviewVisibility.card.isConnected`), true,
      'Hiding the document must retain the selected controller card.');
    const hiddenReads = inspectionReads.length;
    if (privateDatabase) {
      renamed = `Hidden preview check ${Date.now()}`;
      const updated = await saveName(renamed);
      restoreMetadata = true;
      assert.equal(updated.controllers.source.name, renamed);
    }
    assert.equal(inspectionReads.length, hiddenReads, 'No controller_inspect query may be used for the preview.');

    await setHidden(false);
    await until(() => subscriptions.length === 2 && acknowledgements.some(row => row.token === subscriptions[1]?.token && row.delivery === '1'));
    const reopened = subscriptions[1];
    assert.equal(reopened.topic, topicPrefix + sourceId);
    assert.notEqual(reopened.key, first.key, 'Reveal must acquire a fresh transport identity.');
    assert.notEqual(reopened.token, first.token, 'Reveal must acquire a fresh subscription id.');
    const reopenedInput = await evaluate(`(async()=>{
      const {observeControllerInputs}=await import('/src/lib/controller-observations.ts');
      const state=window.__controllerPreviewVisibility;
      state.stopObserved=observeControllerInputs(${JSON.stringify(sourceId)},value=>{
        if(value.status==='ready')state.observed??={id:value.detail.source.id,name:value.detail.source.name,
          sequence:value.detail.source.sequence};
      });
      return true;
    })()`);
    assert.equal(reopenedInput, true);
    const currentReport = await until(() => evaluate(`(() => {
      const state=window.__controllerPreviewVisibility,report=document.querySelector(${JSON.stringify(liveCard + ' .controller-status')})?.textContent;
      const sequence=report?.match(/report\\s+([0-9]+)/)?.[1],observed=state.observed;
      return observed&&sequence&&BigInt(sequence)>=BigInt(observed.sequence)?{sequence,observed}:null;
    })()`));
    assert.equal(currentReport.observed.id, sourceId);
    if (privateDatabase) assert.equal(currentReport.observed.name, renamed,
      'The reopened input baseline must carry the committed identity from the hidden rename.');
    const afterReveal = await evaluate(`(() => ({
      cardRetained:window.__controllerPreviewVisibility.card.isConnected,
      draft:window.__controllerPreviewVisibility.input.value,
      report:document.querySelector(${JSON.stringify(liveCard + ' .controller-status')})?.textContent,
      sourceId:document.querySelector(${JSON.stringify(liveCard + ' .controller-identity dd code')})?.textContent
    }))()`);
    assert.equal(afterReveal.cardRetained, true);
    assert.equal(afterReveal.draft, 'Unsaved preview draft', 'Visibility changes must preserve editor drafts.');
    assert.equal(afterReveal.sourceId, sourceId);
    const visibleSequence = afterReveal.report.match(/report\s+([0-9]+)/)?.[1];
    assert.ok(visibleSequence, `No current report sequence rendered: ${afterReveal.report}`);
    assert.equal(inspectionReads.length, 0, 'Controller preview must not query controller_inspect.');
    assert.equal(subscriptions.length, 2, 'The probe shares the reopened input subscription.');
    assert.deepEqual(errors, []);

    // Leave the selected production detail before restoring its local metadata.
    await evaluate('window.__controllerPreviewVisibility.stopObserved?.()');
    await evaluate('document.querySelector(".component-details .header-back")?.click()');
    if (restoreMetadata) {
      const restored = await saveName(original.name);
      assert.equal(restored.controllers.source.name, original.name.trim());
      restoreMetadata = false;
    }
    await until(() => unsubscriptions.some(row => row.key === reopened.key && (!browserUrl ? row.token === reopened.token : true)));
    console.log(JSON.stringify({transport, sourceId, topic: first.topic,
      subscriptionIds: [first.token, reopened.token], baselineAcknowledgements: [first.token, reopened.token],
      hiddenUnsubscribeMatched: true, revealSequence: visibleSequence, cardRetained: afterReveal.cardRetained,
      draftRetained: afterReveal.draft === 'Unsaved preview draft', hiddenRenameObserved: privateDatabase,
      controllerInspectQueries: inspectionReads.length}));
  } finally {
    try {
      await evaluate('window.__controllerPreviewVisibility?.stopObserved?.()').catch(() => {});
      await setHidden(false);
      if (restoreMetadata && original) await saveName(original.name);
    } finally {
      await evaluate('delete window.__controllerPreviewVisibility; delete document.hidden').catch(() => {});
      stopTrace();
      await page.close();
    }
  }
});
