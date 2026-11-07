import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {writeFile} from 'node:fs/promises';
import {setTimeout as delay} from 'node:timers/promises';
import test from 'node:test';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_FIRMWARE_RELEASE_OBSERVATION_BROWSER_URL;
const catalog = process.env.OSG_TEST_RELEASE_CATALOG;
const topic = 'companion.firmware-releases';
const uri = `cockpit://observations/${topic}`;
const gateway = browserUrl ? '/api/gateway/' : '/gateway_request';
const subscribe = browserUrl ? '/api/observe/subscribe' : '/observation_subscribe';
const unsubscribe = browserUrl ? '/api/observe/unsubscribe' : '/observation_unsubscribe';
const board = 'sunton-3248s035r';
const deviceId = 'firmware-release-fixture';
const deviceName = 'Firmware release fixture';

async function writeCatalog(version) {
  const tag = `esp32-companion-v${version}`;
  const name = `esp32-companion-${version}-${board}.zip`;
  await writeFile(catalog, JSON.stringify([{
    tag_name: tag, draft: false, prerelease: false, published_at: '2026-11-07T00:00:00Z',
    assets: [{name, size: 1, browser_download_url: `https://github.com/opensimgear/cockpit/releases/download/${tag}/${name}`}],
  }]));
}

test('Firmware release commits reach the open view and a reopened reader', {
  skip: !catalog, timeout: 60_000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const opens = [], closes = [];
  let statusReads = 0, traceError;
  const readBody = ({body, base64Encoded}) => base64Encoded ? Buffer.from(body, 'base64').toString('utf8') : body;
  const stop = page.listen(({method, params}) => {
    if (method === 'Network.loadingFinished' && !browserUrl) {
      const open = opens.find(item => item.id === params.requestId);
      if (open) void page.call('Network.getResponseBody', {requestId:open.id}).then(body => {
        open.token = JSON.parse(readBody(body));
      }).catch(error => { traceError = error; });
    }
    if (method === 'Fetch.requestPaused') {
      void authorDevice(params).catch(error => { traceError = error; });
    }
    if (method !== 'Network.requestWillBeSent' || !params.request?.postData) return;
    const path = new URL(params.request.url).pathname;
    if (![subscribe, unsubscribe, gateway].includes(path)) return;
    const body = JSON.parse(params.request.postData);
    if (path === subscribe && body.topic === topic) {
      opens.push({id:params.requestId, token:browserUrl ? [body.connectionId, body.requestId] : undefined});
    }
    if (path === unsubscribe) closes.push(browserUrl ? [body.connectionId, body.requestId] : body.subscriptionId);
    if (path === gateway && (body.request ?? body).command === 'companion_status') statusReads++;
  });
  async function authorDevice(params) {
    const {requestId, request, responseStatusCode, responseHeaders} = params;
    const body = JSON.parse(request.postData ?? '{}');
    if ((body.request ?? body).command !== 'companion_status') {
      await page.call('Fetch.continueRequest', {requestId});
      return;
    }
    // Supply a previously paired board without touching pairing or USB. The
    // status response always has old release metadata, so only the real
    // observation can update the displayed available version.
    const response = JSON.parse(readBody(await page.call('Fetch.getResponseBody', {requestId})));
    assert.equal(response.ok, true, response.error);
    const status = response.state.companions;
    status.host.clients.push({companionId:deviceId, name:deviceName, kind:'esp32', connected:false});
    status.firmware[deviceId] = {profile:board, version:'1.0.0'};
    status.firmwareProfiles = [{id:board, name:'Sunton', available:false}];
    status.firmwareVersion = null;
    await page.call('Fetch.fulfillRequest', {
      requestId, responseCode:responseStatusCode,
      responseHeaders:responseHeaders.filter(header => header.name.toLowerCase() !== 'content-length'),
      body:Buffer.from(JSON.stringify(response)).toString('base64'),
    });
  }
  const waitTrace = check => until(() => {
    if (traceError) throw traceError;
    return check();
  });
  const closed = open => open.token && closes.some(token => JSON.stringify(token) === JSON.stringify(open.token));
  const click = selector => until(() => page.evaluate(`(() => {
    const target = document.querySelector(${JSON.stringify(selector)});
    if (!target || target.closest('[hidden]') || target.disabled) return false;
    target.click();
    return true;
  })()`));
  const nav = name => click(`nav[aria-label="Main navigation"] button[title="${name}"]`);
  const device = () => click(`button[aria-label="Edit ${deviceName}"]`);
  const value = expected => until(() => page.evaluate("document.querySelector('.firmware-version-value span')?.textContent?.trim()"), actual => actual === expected);
  const check = () => page.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest}) => gatewayRequest({command:'companion_firmware_check'}))`);
  const invoke = (command, args = {}) => page.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`);
  const settings = () => invoke('mcp_settings_get');
  const configure = async (enabled, port) => invoke('mcp_settings_update', {settings:{
    enabled, port, expectedSettingsRevision:(await settings()).settingsRevision,
  }});
  let mcp, credentialId, initialSettings, enabledByFixture = false;
  const mcpFrame = async () => {
    const resource = await mcp.readResource({uri});
    return JSON.parse(resource.contents[0].text).frame;
  };
  try {
    if (!browserUrl) {
      await until(() => page.evaluate('typeof window.__TAURI_INTERNALS__?.invoke === "function"'));
      initialSettings = await settings();
      const credential = await invoke('mcp_credential_create', {name:'Firmware release fixture', scope:'write',
        expectedSettingsRevision:initialSettings.settingsRevision});
      credentialId = credential.credential.id;
      const status = initialSettings.enabled ? initialSettings : await configure(true, initialSettings.port);
      enabledByFixture = !initialSettings.enabled;
      mcp = new Client({name:'Firmware release fixture', version:'1'}, {capabilities:{}});
      await mcp.connect(new StreamableHTTPClientTransport(new URL(status.endpoint), {
        requestInit:{headers:{Authorization:`Bearer ${credential.token}`}}, reconnectionOptions:{maxRetries:0},
      }));
      assert.ok((await mcp.listResources()).resources.some(resource => resource.uri === uri));
      assert.equal((await mcpFrame()).payload.firmwareChecked, false);
    }
    await page.call('Network.enable', {maxPostDataSize:65536});
    await page.call('Fetch.enable', {patterns:[{urlPattern:`*${gateway}`, requestStage:'Response'}]});
    await nav('Companions');
    await device();
    await waitTrace(() => opens.length === 1 && opens[0].token);
    await value('Unknown');
    await delay(350);
    const visibleReads = statusReads;
    await writeCatalog('2.3.4');
    if (mcp) {
      const result = await mcp.callTool({name:'companion_firmware_check', arguments:{operationId:randomUUID()}});
      assert.equal(result.structuredContent?.ok, true, JSON.stringify(result.structuredContent));
      const first = await mcpFrame();
      assert.equal(first.payload.firmwareVersion, '2.3.4');
      assert.equal((await check()).companions.firmwareVersion, '2.3.4');
      assert.equal((await mcpFrame()).revision, first.revision, 'An unchanged check must not publish a new revision');
    } else assert.equal((await check()).companions.firmwareVersion, '2.3.4');
    await value('2.3.4');
    assert.equal(statusReads, visibleReads, 'Release metadata must render without reading companion status again');

    await nav('Overview');
    await waitTrace(() => closed(opens[0]));
    const hiddenReads = statusReads;
    await writeCatalog('2.3.5');
    assert.equal((await check()).companions.firmwareVersion, '2.3.5');
    if (mcp) assert.equal((await mcpFrame()).payload.firmwareVersion, '2.3.5');
    await delay(350);
    assert.equal(statusReads, hiddenReads, 'Hidden view must not read companion status');
    assert.equal(opens.length, 1);
    await nav('Companions');
    await device();
    await value('2.3.5');
    await waitTrace(() => opens.length === 2 && opens[1].token);
    await nav('Overview');
    await waitTrace(() => opens.every(closed));
    console.log(JSON.stringify({transport:browserUrl ? 'browser-dev' : 'native', mcp:!!mcp, exactReaders:2, retired:2,
      initial:'Unknown', visible:'2.3.4', reopened:'2.3.5', visibleExtraStatusReads:0, hiddenStatusReads:0}));
  } finally {
    await page.call('Fetch.disable').catch(() => {});
    stop();
    try {
      await mcp?.close();
      if (credentialId) await invoke('mcp_credential_revoke', {id:credentialId,
        expectedSettingsRevision:(await settings()).settingsRevision});
      if (enabledByFixture) await configure(false, initialSettings.port);
    } finally { await page.close(); }
  }
});
