import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { createServer } from 'node:net';
import test from 'node:test';
import { promisify } from 'node:util';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { nativePage, until } from '../../scripts/reactive/cdp.mjs';

const fixtureId = process.env.OSG_AUTOSTART_QUALIFICATION_ID;
const runKey = String.raw`HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run`;
const approvedKey = String.raw`HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run`;
const reg = (...args) => promisify(execFile)('reg.exe', args, {windowsHide:true});
const uri = 'cockpit://observations/settings.autostart';

async function freePort() {
  const server = createServer();
  await new Promise((resolve, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolve));
  const {port} = server.address();
  await new Promise(resolve => server.close(resolve));
  return port;
}

test('native auto-start follows both Windows registry sources and retires its hidden reader', {
  skip: process.platform !== 'win32' || !fixtureId,
  timeout: 60_000,
}, async () => {
  assert.match(fixtureId, /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/);
  const name = `OpenSimGear-Autostart-Test-${fixtureId}`;
  const add = (key, type, value) => reg('add', key, '/v', name, '/t', type, '/d', value, '/f');
  const remove = key => reg('delete', key, '/v', name, '/f');
  // The dedicated qualification plugin identity must start empty. Do not
  // create startup registry keys or replace an existing registration.
  for (const key of [runKey, approvedKey]) {
    await reg('query', key);
    await assert.rejects(reg('query', key, '/v', name), {code:1});
  }
  const page = await nativePage();
  let reads = 0;
  let runCreated = false, overrideCreated = false;
  let mcp, credential, initialMcp;
  const opening = new Set(), tokens = [], retired = new Set(), errors = [];
  const stop = page.listen(({method, params}) => {
    if (method === 'Network.loadingFinished' && opening.delete(params.requestId)) {
      void page.call('Network.getResponseBody', {requestId:params.requestId})
        .then(({body}) => tokens.push(JSON.parse(body))).catch(error => errors.push(error));
    }
    if (method !== 'Network.requestWillBeSent' || params.request.method !== 'POST') return;
    const path = new URL(params.request.url).pathname;
    if (path === '/desktop_preferences') reads++;
    if (path === '/observation_subscribe' && JSON.parse(params.request.postData).topic === 'settings.autostart') opening.add(params.requestId);
    if (path === '/observation_unsubscribe') retired.add(JSON.parse(params.request.postData).subscriptionId);
  });
  const settingsButton = `Array.from(document.querySelectorAll('.sidebar-bottom button'))
    .find(button => button.textContent.trim() === 'Settings')`;
  const tab = id => page.evaluate(`document.querySelector('#settings-tab-${id}').click()`);
  const control = '[aria-label="Start Cockpit when I sign in"]';
  const current = () => page.evaluate(`document.querySelector('${control}:not(:disabled)')?.getAttribute('aria-checked')`);
  const expect = enabled => until(current, value => value === String(enabled));
  const invoke = (command, args = {}) => page.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`);
  const mcpStatus = () => invoke('mcp_settings_get');
  const mcpEnabled = async enabled => {
    const {contents} = await mcp.readResource({uri});
    assert.deepEqual(JSON.parse(contents[0].text).frame.payload, {enabled,error:null});
  };
  try {
    await until(() => page.evaluate(`!!(${settingsButton})`));
    await page.call('Network.enable', {maxPostDataSize:65536});
    await page.evaluate(`(${settingsButton}).click()`);
    await expect(false);
    await until(() => tokens.length === 1);
    initialMcp = await mcpStatus();
    assert.equal(initialMcp.enabled, false, 'Use a private dev database with MCP disabled initially');
    credential = await invoke('mcp_credential_create', {
      name:`Auto-start fixture ${fixtureId}`, scope:'read', expectedSettingsRevision:initialMcp.settingsRevision,
    });
    const mcpSettings = await invoke('mcp_settings_update', {settings:{enabled:true, port:await freePort(),
      expectedSettingsRevision:credential.status.settingsRevision}});
    mcp = new Client({name:'Auto-start fixture',version:'1'});
    await mcp.connect(new StreamableHTTPClientTransport(new URL(mcpSettings.endpoint), {
      requestInit:{headers:{Authorization:`Bearer ${credential.token}`}}, reconnectionOptions:{maxRetries:0},
    }));
    assert.ok((await mcp.listResources()).resources.some(resource => resource.uri === uri));
    await mcpEnabled(false);

    await add(runKey, 'REG_SZ', 'cmd.exe /c exit 0');
    runCreated = true;
    await expect(true);
    await mcpEnabled(true);
    await add(approvedKey, 'REG_BINARY', '030000000000000000000001');
    overrideCreated = true;
    await expect(false);
    await mcpEnabled(false);
    assert.equal(reads, 1, 'Visible registry changes must deliver values without status requests');

    await tab('styling');
    await until(() => retired.has(tokens[0]));
    await remove(approvedKey);
    overrideCreated = false;
    assert.equal(await current(), undefined);
    assert.equal(reads, 1);
    await tab('general');
    await expect(true);
    await until(() => tokens.length === 2);
    assert.notEqual(tokens[0], tokens[1]);
    assert.equal(reads, 2);

    // Use the real disable button only. The fixture never installs Cockpit
    // as a startup app, even under the dedicated qualification name.
    await page.evaluate(`document.querySelector('${control}').click()`);
    await expect(false);
    await assert.rejects(reg('query', runKey, '/v', name), {code:1});
    runCreated = false;
    assert.equal(reads, 2);
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({runValueChanged:true,taskManagerOverrideChanged:true,hiddenReaderRetired:true,
      visibleStatusReads:0,visibleOpenReads:reads,manualDisable:true,mcpResource:true}));
  } finally {
    try {
      if (overrideCreated) await remove(approvedKey);
      if (runCreated) await remove(runKey).catch(async error => {
        if (error.code !== 1) throw error;
        await assert.rejects(reg('query', runKey, '/v', name), {code:1});
      });
    } finally {
      stop();
      try {
        await mcp?.close();
        if (credential) {
          const status = await mcpStatus();
          const revoked = await invoke('mcp_credential_revoke', {id:credential.credential.id, expectedSettingsRevision:status.settingsRevision});
          await invoke('mcp_settings_update', {settings:{enabled:false,port:initialMcp.port,expectedSettingsRevision:revoked.settingsRevision}});
        }
        await page.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="Overview"]').click()`);
      } finally { await page.close(); }
    }
  }
});
