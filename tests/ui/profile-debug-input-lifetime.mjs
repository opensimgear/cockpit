import assert from 'node:assert/strict';
import test from 'node:test';
import {setTimeout as delay} from 'node:timers/promises';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_DEBUG_INPUT_LIFETIME_BROWSER_URL;
const topic = 'profiles.debug-inputs';
const subscribe = browserUrl ? '/api/observe/subscribe' : '/observation_subscribe';
const unsubscribe = browserUrl ? '/api/observe/unsubscribe' : '/observation_unsubscribe';
const gateway = browserUrl ? '/api/gateway/' : '/gateway_request';
const nodeId = 'fixture-debug-input';
const editor = '[aria-label="Debug controller settings"]';

test('Debug controls retire while hidden and reopen from the exact current snapshot', {
  skip: !browserUrl && process.env.OSG_DEBUG_INPUT_LIFETIME_NATIVE !== '1',
  timeout: 90000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  let saved, owner, paused, traceError, reads = 0, holdOpen = false;
  const opens = [], closes = [], buttonWrites = [];
  const request = value => page.evaluate(`import('/src/lib/gateway.ts').then(
    ({gatewayRequest})=>gatewayRequest(${JSON.stringify(value)}))`);
  const visible = show => page.evaluate(`(() => {
    if (${show}) delete document.hidden;
    else Object.defineProperty(document,'hidden',{configurable:true,value:true});
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const select = selector => until(() => page.evaluate(`(() => {
    const button=document.querySelector(${JSON.stringify(selector)});
    if(!button||button.disabled||button.closest('[hidden]'))return false;
    button.click();return true;
  })()`));
  const controls = () => page.evaluate(`document.querySelector('${editor} .live-heading span')?.textContent`);
  const stopTrace = page.listen(({method,params}) => {
    if (method === 'Network.loadingFinished' && !browserUrl) {
      const open = opens.find(item => item.id === params.requestId);
      if (open) void page.call('Network.getResponseBody',{requestId:open.id}).then(({body}) => {
        open.token = JSON.parse(body);
      }).catch(error => {traceError=error;});
      return;
    }
    if (!params.request?.postData) return;
    const path = new URL(params.request.url).pathname;
    if (![subscribe,unsubscribe,gateway].includes(path)) return;
    const body = JSON.parse(params.request.postData);
    if (method === 'Fetch.requestPaused') {
      if (holdOpen && path === subscribe && body.topic === topic) paused = params;
      else void page.call('Fetch.continueRequest',{requestId:params.requestId})
        .catch(error => {traceError=error;});
    } else if (method === 'Network.requestWillBeSent') {
      if (path === subscribe && body.topic === topic) {
        opens.push({id:params.requestId, token:browserUrl ? [body.connectionId,body.requestId] : undefined});
      } else if (path === unsubscribe) {
        closes.push(browserUrl ? [body.connectionId,body.requestId] : body.subscriptionId);
      } else if (path === gateway) {
        const request = body.request ?? body;
        if (request.command === 'profile_debug_controller_status') reads++;
        if (request.command === 'profile_debug_controller_input' && request.nodeId === nodeId) {
          buttonWrites.push(...request.changes.filter(change => change.portId === 'button-1').map(change => change.value));
        }
      }
    }
  });
  const retired = index => closes.some(token => JSON.stringify(token) === JSON.stringify(opens[index].token));
  const trace = async (read,label) => until(() => {
    if(traceError)throw traceError;
    return read();
  },Boolean,4500).catch(error => {throw Error(label,{cause:error});});
  try {
    await select('nav[aria-label="Main navigation"] button[title="Profiles"]');
    const name = 'Debug visibility fixture ' + Date.now();
    const {document} = (await request({command:'profile_create',name})).profiles;
    saved = document;
    const resource = await page.evaluate(`import('/src/lib/profile-debug-controller.ts').then(
      ({debugControllerResource,debugControllerPorts})=>({
        ...debugControllerResource('${nodeId}','Debug visibility controls'),
        ports:debugControllerPorts({buttons:['button-1'],axes:[],hats:['hat-1']}),
      }))`);
    document.resources.push(resource);
    saved = (await request({command:'profile_editor_save',document,assignments:[],
      expectedRevision:document.revision})).profiles.document;
    await select(`button[aria-label="Edit ${name}"]`);
    owner = (await request({command:'profile_apply',id:saved.id,expectedRevision:saved.revision,
      expectedActiveOwner:null})).profiles.profileSession.owner;
    assert.equal(owner?.profileId,saved.id);

    await page.call('Network.enable',{maxPostDataSize:65536});
    await select('.resource-block .block-heading');
    await trace(() => opens[0]?.token,'Initial debug subscription did not open');
    await until(async () => (await controls()) === 'Active');
    assert.equal(opens.length,1);
    assert.equal(reads,1);

    await page.evaluate(`document.querySelector('${editor} button[aria-label="Button 1"]')
      .dispatchEvent(new KeyboardEvent('keydown',{key:' ',bubbles:true}))`);
    await trace(() => buttonWrites[0] === true,'The inspector did not submit its held button');
    await visible(false);
    await trace(() => retired(0),'Hidden debug inspector kept its original reader');
    await trace(() => buttonWrites.includes(false),'Hiding the inspector did not release its held button');
    assert.equal(await controls(),'Inactive');
    const before = reads;
    await request({command:'profile_debug_controller_input',owner,nodeId,
      streamId:crypto.randomUUID(),sequence:'1',changes:[{portId:'hat-1',value:2}]});
    await delay(350);
    assert.equal(reads,before,'A hidden inspector must make no status reads');
    assert.equal(opens.length,1);

    holdOpen = true;
    await page.call('Fetch.enable',{patterns:[{urlPattern:'*'+subscribe}]});
    await visible(true);
    await trace(() => paused,'Showing Cockpit did not open a fresh reader');
    await delay(200);
    assert.equal(reads,before,'Reopened controls must wait for the new baseline');
    assert.equal(await controls(),'Inactive');
    await page.call('Fetch.continueRequest',{requestId:paused.requestId});
    paused = undefined;
    holdOpen = false;
    await page.call('Fetch.disable');
    await trace(() => opens[1]?.token,'Reopened debug subscription did not complete');
    await until(() => page.evaluate(`document.querySelector(
      '${editor} button[aria-label="Hat 1: E"]')?.getAttribute('aria-pressed')==='true'`));
    assert.equal(await controls(),'Active');
    assert.equal(await page.evaluate(`document.querySelector(
      '${editor} button[aria-label="Button 1"]')?.getAttribute('aria-pressed')`),'false');
    assert.equal(reads,before+1);
    assert.equal(opens.length,2);
    assert.notDeepEqual(opens[1].token,opens[0].token);

    await select('nav[aria-label="Main navigation"] button[title="Overview"]');
    await trace(() => retired(1),'Leaving Profiles kept the reopened debug reader');
    assert.equal(reads,before+1);
    console.log(JSON.stringify({transport:browserUrl?'browser-dev':'native',opens:2,exactRetirements:2,
      initialStatusReads:1,hiddenStatusReads:0,reopenStatusReads:1,externalHatRestored:true,buttonWrites}));
  } finally {
    if (paused) await page.call('Fetch.continueRequest',{requestId:paused.requestId}).catch(() => {});
    await page.call('Fetch.disable').catch(() => {});
    await visible(true).catch(() => {});
    stopTrace();
    try {
      if (owner) await request({command:'profile_stop'});
      if (saved) await request({command:'profile_delete',id:saved.id,expectedRevision:saved.revision});
    } finally {await page.close();}
  }
});
