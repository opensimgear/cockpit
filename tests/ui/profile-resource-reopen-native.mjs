// Opt-in running Cockpit check. Use a private dev database and an attached controller.
import assert from 'node:assert/strict';
import { setTimeout as delay } from 'node:timers/promises';
import test from 'node:test';
import { browserPage, nativePage, until } from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_RESOURCE_REOPEN_BROWSER_URL;
const transport = browserUrl ? 'browser-dev' : 'native';
const enabled = !!browserUrl || process.env.OSG_RESOURCE_REOPEN_NATIVE === '1';

test(`${transport} profile editor reuses the current resource catalogue on reopen`, {
  skip: !enabled,
  timeout: 90_000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const request = value => page.evaluate(
    `import('/src/lib/gateway.ts').then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(value)}))`,
  );
  const navigate = async label => {
    await until(() => page.evaluate(`(() => {
      const button = [...document.querySelectorAll('nav[aria-label="Main navigation"] button')]
        .find(button => button.title === ${JSON.stringify(label)});
      if (!button) return false;
      button.click();
      return true;
    })()`));
    await until(() => page.evaluate(`document.querySelector('[aria-label="Breadcrumb"] [aria-current="page"]')
      ?.textContent.trim() === ${JSON.stringify(label)}`));
  };
  const profileName = `Resource reopen ${Date.now()}`;
  const openEditor = async () => {
    await navigate('Profiles');
    await until(() => page.evaluate(`(() => {
      const button = [...document.querySelectorAll('.profiles-workspace button.profile-row')]
        .find(button => button.title === ${JSON.stringify(profileName)});
      if (!button || button.disabled) return false;
      button.click();
      return true;
    })()`));
  };
  const palette = () => page.evaluate(`[...document.querySelectorAll('[aria-label="Component palette"] button[data-resource-id]')]
    .map(button => ({id:button.dataset.resourceId,name:button.title}))`);
  const setName = (source, name) => request({
    command: 'controller_update', sourceId: source.id, name, role: source.role,
  });
  const resourceRequests = new Set();
  const resourceReplies = [];
  let resourceOpens = 0, resourceReads = 0;
  const traceErrors = [];
  const stopTrace = page.listen(({method, params}) => {
    if (method === 'Network.requestWillBeSent' && params.request.method === 'POST' && params.request.postData) {
      const path = new URL(params.request.url).pathname;
      if (!['/gateway_request', '/api/gateway/', '/observation_subscribe', '/api/observe/subscribe'].includes(path)) return;
      try {
        const body = JSON.parse(params.request.postData);
        if (body.topic === 'profiles.resources') resourceOpens++;
        if ((body.request ?? body).command === 'profile_resources_list') {
          resourceReads++;
          resourceRequests.add(params.requestId);
        }
      } catch (error) { traceErrors.push(String(error)); }
    }
    if (method === 'Network.loadingFinished' && resourceRequests.delete(params.requestId)) {
      void page.call('Network.getResponseBody', {requestId:params.requestId}).then(({body}) => {
        const reply = JSON.parse(body);
        assert.equal(reply.ok, true, reply.error);
        resourceReplies.push(reply.state.profileResources);
      }).catch(error => traceErrors.push(String(error)));
    }
  });
  let source, profile, renamed = false;
  try {
    await navigate('Overview');
    const {controllers} = await request({command:'controller_list',offset:0,limit:32});
    source = controllers.sources.find(value => value.online && !value.osgHid);
    assert.ok(source, 'Connect a controller to the isolated dev Cockpit before running this test.');
    const inventory = (await request({command:'profile_resources_list',offset:0,limit:128})).profileResources;
    const resource = inventory.resources.find(value => value.resourceType === 'host-controller'
      && value.connections.some(link => link.scope === 'host-controllers' && link.key === source.id));
    assert.ok(resource, 'The controller must have a committed profile resource.');
    const readName = async () => (await palette()).find(row => row.id === resource.id)?.name;
    ({profiles:{document:profile}} = await request({command:'profile_create',name:profileName}));
    await openEditor();
    await until(readName, name => name === resource.name);
    await page.call('Network.enable', {maxPostDataSize:65536});

    const visibleName = `Visible ${profileName}`;
    renamed = true;
    await setName(source, visibleName);
    await until(readName, name => name === visibleName);

    await navigate('Overview');
    assert.deepEqual(await palette(), [], 'The closed editor must withdraw its component palette.');
    const hiddenName = `Hidden ${profileName}`;
    await setName(source, hiddenName);
    await until(() => resourceReplies.find(reply => reply?.resources.some(row =>
      row.id === resource.id && row.name === hiddenName)));
    assert.deepEqual(await palette(), []);

    const readsBeforeReopen = resourceReads;
    await openEditor();
    await until(readName, name => name === hiddenName);
    await delay(250);
    assert.deepEqual(traceErrors, []);
    assert.equal(resourceOpens, 0, 'The editor must reuse the shell resource reader.');
    assert.equal(resourceReads, readsBeforeReopen, 'Reopening must reuse the already committed resource page.');

    const restored = (await setName(source, source.name)).controllers.source.name;
    await until(readName, name => name === restored);
    renamed = false;
    console.log(JSON.stringify({transport,visibleRename:true,hiddenRename:true,reopenReadDelta:0,newResourceReaders:0}));
  } finally {
    stopTrace();
    try {
      if (renamed) await setName(source, source.name);
      await navigate('Overview');
      if (profile) await request({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});
    } finally { await page.close(); }
  }
});
