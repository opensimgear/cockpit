import assert from 'node:assert/strict';
import test from 'node:test';
import {setTimeout as delay} from 'node:timers/promises';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_PROFILE_INSPECTION_BROWSER_URL;
const subscribe = browserUrl ? '/api/observe/subscribe' : '/observation_subscribe';
const unsubscribe = browserUrl ? '/api/observe/unsubscribe' : '/observation_unsubscribe';
const gateway = browserUrl ? '/api/gateway/' : '/gateway_request';
const path = '[aria-label="Applied source to destination activity"]';
const samples = '[aria-label="Native applied port samples"]';

test('Applied topology and port samples follow the replacement owner after reopening', {
  skip: !browserUrl && process.env.OSG_PROFILE_INSPECTION_NATIVE !== '1', timeout: 120000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const profiles = [], opens = [], closes = [], reads = [];
  let applied = false, traceError;
  const request = value => page.evaluate(`import('/src/lib/gateway.ts').then(
    ({gatewayRequest})=>gatewayRequest(${JSON.stringify(value)}))`);
  const click = selector => until(() => page.evaluate(`(() => {
    const target = document.querySelector(${JSON.stringify(selector)});
    if (!target || target.disabled || target.closest('[hidden]')) return false;
    target.click();return true;
  })()`));
  const tab = label => until(() => page.evaluate(`(() => {
    const target = [...document.querySelectorAll('[aria-label="Debug sections"] [role="tab"]')]
      .find(tab=>tab.textContent.trim()===${JSON.stringify(label)});
    if(!target)return false;
    target.click();return true;
  })()`));
  const visible = show => page.evaluate(`(() => {
    if (${show}) delete document.hidden;
    else Object.defineProperty(document,'hidden',{configurable:true,value:true});
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const stopTrace = page.listen(({method,params}) => {
    if (method === 'Network.loadingFinished' && !browserUrl) {
      const open = opens.find(item => item.id === params.requestId);
      if (open) void page.call('Network.getResponseBody',{requestId:open.id}).then(({body}) => {
        open.token = JSON.parse(body);
      }).catch(error => {traceError=error;});
      return;
    }
    if (method !== 'Network.requestWillBeSent' || !params.request?.postData) return;
    const url = new URL(params.request.url).pathname;
    if (![subscribe,unsubscribe,gateway].includes(url)) return;
    const body = JSON.parse(params.request.postData);
    if (url === subscribe && body.topic?.startsWith('profiles.samples.')) {
      opens.push({id:params.requestId,topic:body.topic,
        token:browserUrl ? [body.connectionId,body.requestId] : undefined});
    } else if (url === unsubscribe) {
      closes.push(browserUrl ? [body.connectionId,body.requestId] : body.subscriptionId);
    } else if (url === gateway) {
      const {command,owner} = body.request ?? body;
      if (['profile_runtime_inspect','profile_runtime_topology'].includes(command)) reads.push({command,owner});
    }
  });
  const waitTrace = check => until(() => {
    if (traceError) throw traceError;
    return check();
  });
  const closed = open => open.token && closes.some(token => JSON.stringify(token) === JSON.stringify(open.token));
  const texts = () => page.evaluate(`[...document.querySelectorAll('${path} .activity-block, ${samples}')]
    .map(node=>node.innerText).join('\\n')`);
  const sourceValue = source => page.evaluate(`[...document.querySelectorAll('${samples} tbody tr')]
    .find(row=>row.querySelector('code')?.textContent===${JSON.stringify(source+' / button-1')})
    ?.children[2]?.innerText.trim()`);
  const fixture = async id => {
    const name = 'Inspection fixture ' + id + ' ' + Date.now();
    const {document} = (await request({command:'profile_create',name})).profiles;
    profiles.push(document);
    const source = 'debug-' + id, processor = 'identity-' + id;
    const resource = await page.evaluate(`import('/src/lib/profile-debug-controller.ts').then(
      ({debugControllerResource,debugControllerPorts})=>({
        ...debugControllerResource('${source}','Source ${id}'),
        ports:debugControllerPorts({buttons:['button-1'],axes:[],hats:[]})
      }))`);
    const contract = {...resource.ports[0].contract,freshnessMs:10000};
    resource.ports[0].contract = contract;
    document.resources.push(resource);
    document.processingNodes.push({id:processor,name:'Identity '+id,operation:{kind:'identity'},
      inputs:[{id:'in',name:'Input',direction:'input',contract}],
      outputs:[{id:'out',name:'Output',direction:'output',contract}]});
    document.bindings.push({id:'edge-'+id,from:{nodeId:source,portId:'button-1'},to:{nodeId:processor,portId:'in'}});
    const saved = (await request({command:'profile_editor_save',document,assignments:[],
      expectedRevision:document.revision})).profiles.document;
    profiles[profiles.length-1] = saved;
    return {saved,source,processor};
  };
  const apply = async fixture => {
    const {owner} = (await request({command:'profile_apply',id:fixture.saved.id,
      expectedRevision:fixture.saved.revision,expectedActiveOwner:null})).profiles.profileSession;
    applied = true;
    assert.equal(owner?.profileId,fixture.saved.id);
    return owner;
  };
  const input = (owner, nodeId) => request({command:'profile_debug_controller_input',owner,nodeId,
    streamId:crypto.randomUUID(),sequence:'1',changes:[{portId:'button-1',value:true}]});
  const topics = (owner, fixture) => page.evaluate(`import('/src/lib/profile-samples/models.ts').then(
    ({profileNodeSamplesTopic})=>Promise.all(${JSON.stringify([fixture.source,fixture.processor])}
      .map(node=>profileNodeSamplesTopic(${JSON.stringify(owner)},node))))`);
  const checkReads = (items, owner) => {
    assert.ok(items.every(item => item.owner?.session === owner.session && item.owner?.generation === owner.generation));
    assert.equal(items.filter(item => item.command === 'profile_runtime_inspect').length,1);
    assert.equal(items.filter(item => item.command === 'profile_runtime_topology').length,2);
  };
  try {
    await click('nav[aria-label="Main navigation"] button[title="Debug"]');
    const first = await fixture('old'), second = await fixture('new');
    const old = await apply(first);
    await page.call('Network.enable',{maxPostDataSize:65536});
    await tab('Profile inspection');
    await waitTrace(() => opens.length >= 2 && opens.every(open => open.token));
    await input(old,first.source);
    await until(() => sourceValue(first.source), value => value === 'true');
    assert.match(await texts(),/Identity old/);
    assert.deepEqual(opens.map(open => open.topic).sort(),(await topics(old,first)).sort());
    checkReads(reads,old);

    await visible(false);
    await waitTrace(() => opens.every(closed));
    assert.equal(await texts(),'');
    const hiddenCounts = {opens:opens.length,reads:reads.length};
    await request({command:'profile_stop'});
    applied = false;
    const current = await apply(second);
    await input(current,second.source);
    await delay(500);
    assert.equal(opens.length,hiddenCounts.opens);
    assert.equal(reads.length,hiddenCounts.reads);

    await visible(true);
    await waitTrace(() => opens.length >= 4 && opens.every(open => open.token));
    await until(() => sourceValue(second.source), value => value === 'true');
    const shown = await texts();
    assert.match(shown,/Identity new/);
    assert.doesNotMatch(shown,/Identity old|debug-old/);
    assert.deepEqual(opens.slice(hiddenCounts.opens).map(open => open.topic).sort(),(await topics(current,second)).sort());
    checkReads(reads.slice(hiddenCounts.reads),current);
    await tab('Incoming telemetry');
    await waitTrace(() => opens.every(closed));
    assert.equal(opens.length,4);
    console.log(JSON.stringify({transport:browserUrl?'browser-dev':'native',
      exactNodeReaders:4,exactRetirements:4,hiddenReads:0,hiddenOpens:0,
      topologyPagesPerOpen:2,inspectionPagesPerOpen:1,replacementValue:'true',oldOwnerAbsent:true}));
  } finally {
    await visible(true).catch(() => {});
    stopTrace();
    try {
      if (applied) await request({command:'profile_stop'});
      for (const saved of profiles) await request({command:'profile_delete',id:saved.id,expectedRevision:saved.revision});
    } finally {await page.close();}
  }
});
