import assert from 'node:assert/strict';
import {createHash, randomUUID, X509Certificate} from 'node:crypto';
import {createRequire} from 'node:module';
import {createServer} from 'node:net';
import {resolve} from 'node:path';
import {connect} from 'node:tls';
import {setTimeout as delay} from 'node:timers/promises';
import test from 'node:test';
import {pathToFileURL} from 'node:url';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_PANEL_ASSIGNMENT_BROWSER_URL;
const enabled = process.env.OSG_PANEL_ASSIGNMENT_REOPEN === '1'
  && process.env.OSG_NAVIGATION_PRIVATE_LISTENER === '1'
  && process.env.OSG_NAVIGATION_ISOLATED_DATABASE === '1';

async function loopbackPort() {
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const {port} = server.address();
  await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  return port;
}

async function listenerPin(port) {
  return new Promise((resolve, reject) => {
    const socket = connect({host: '127.0.0.1', port, rejectUnauthorized: false}, () => {
      try {
        const key = new X509Certificate(socket.getPeerCertificate().raw).publicKey;
        resolve(createHash('sha256').update(key.export({type: 'spki', format: 'der'})).digest('base64'));
      } catch (error) { reject(error); } finally { socket.destroy(); }
    });
    socket.once('error', reject);
    socket.setTimeout(5000, () => socket.destroy(Error('Listener certificate check timed out')));
  });
}

test('Panel editor reconciles background assignments and preserves unsaved selections on reopen', {
  skip: !enabled, timeout: 120_000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const companion = (command, companion = {}) => page.evaluate(`import('/src/lib/gateway.ts')
    .then(({gatewayRequest}) => gatewayRequest(${JSON.stringify({command, companion})}))
    .then(result => result.companions)`);
  const click = (selector, text) => until(() => page.evaluate(`(() => {
    const target = [...document.querySelectorAll(${JSON.stringify(selector)})]
      .find(node => node.checkVisibility() && !node.disabled
        && (${!text} || node.textContent.trim() === ${JSON.stringify(text ?? '')}));
    if (!target) return false;
    target.click(); return true;
  })()`));
  const visibility = hidden => page.evaluate(`(() => {
    if (${hidden}) Object.defineProperty(document, 'hidden', {configurable: true, value: true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const checkbox = id => `[aria-label="Panel configuration"] input[value="${id}"]`;
  const selected = id => page.evaluate(`document.querySelector(${JSON.stringify(checkbox(id))})?.checked`);
  const panelTitle = 'Assignment fixture ' + randomUUID();
  const peerIds = [];
  let browser, listener, statusReads = 0, stopTrace = () => {};
  try {
    const initial = await companion('companion_status');
    assert.equal(initial.listener.configuredEnabled, false, 'Use a fresh disposable database');
    assert.equal(initial.host.clients.length, 0, 'Use a fresh disposable host');
    const port = await loopbackPort();
    ({listener} = await companion('companion_enable', {address: '127.0.0.1', advertisedHost: '127.0.0.1', port,
      discovery: false, expectedSettingsRevision: initial.listener.settingsRevision}));
    assert.equal(listener.enabled, true);
    const pin = await listenerPin(port);
    const require = createRequire(import.meta.url);
    const {chromium} = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');
    browser = await chromium.launch({channel: process.env.OSG_BROWSER_CHANNEL ?? 'msedge', headless: true,
      args: ['--ignore-certificate-errors-spki-list=' + pin]});
    for (let index = 0; index < 2; index++) {
      const {pairing} = await companion('companion_pair_offer', {kind: 'browser'});
      const url = new URL(pairing.browserUrl);
      assert.equal(url.origin, 'https://127.0.0.1:' + port);
      const client = await browser.newPage();
      await client.goto(url.href, {waitUntil: 'domcontentloaded'});
      const peer = await until(async () => (await companion('companion_status')).host.clients
        .find(row => !peerIds.includes(row.companionId)));
      peerIds.push(peer.companionId);
    }
    const [local, external] = peerIds;
    const fixtureLibrary = pathToFileURL(resolve(process.env.OSG_PANEL_FIXTURE_SOURCE ?? '.',
      'src/lib/compiled-companion-library.ts'));
    const {newCompiledDraft, blankWidgetSource, compilePanelDraft} = await import(fixtureLibrary.href);
    const draft = newCompiledDraft(panelTitle), source = blankWidgetSource();
    draft.widgets.push({id: 'placement', pageId: draft.pages[0].id, sourceId: source.id,
      label: source.title, parameters: {}, bounds: {x: 0, y: 0, width: 0.5, height: 0.5}});
    const panel = await compilePanelDraft(draft, [source]);
    const {publicationHash} = panel;
    await page.evaluate(`(async () => {
      const library = await import('/src/lib/compiled-companion-library.ts');
      const {sendPublication} = await import('/src/lib/companions.ts');
      const {gatewayRequest} = await import('/src/lib/gateway.ts');
      const store = new library.CompiledWorkspaceStore();
      const workspace = await store.load();
      const {draft, source, panel} = ${JSON.stringify({draft, source, panel})};
      await store.save({...workspace, drafts: [...workspace.drafts, draft], sources: [...workspace.sources, source],
        publications: {...workspace.publications, [draft.id]: [panel]}});
      await sendPublication(panel, async (command, companion) => (await gatewayRequest({command, companion})).companions);
    })()`);
    const instanceId = () => 'assignment-' + randomUUID();
    const assign = async companionId => {
      const state = await companion('companion_assign', {companionId, publicationHash, instanceId: instanceId()});
      assert.ok(state.host.instances.some(row => row.companionId === companionId && row.publicationHash === publicationHash));
    };
    await click('nav[aria-label="Main navigation"] button[title="Companions"]');
    const open = () => click(`[aria-label="Panels"] button[aria-label="Edit ${panelTitle}"]`);
    await open();
    await until(() => selected(local), value => value === false);
    await assign(local);
    await until(() => selected(local), value => value === true, 5000)
      .catch(cause => { throw Error('The visible editor missed a committed assignment.', {cause}); });
    await click(checkbox(local));
    assert.equal(await selected(local), false);

    await page.call('Network.enable', {maxPostDataSize: 65536});
    stopTrace = page.listen(({method, params}) => {
      if (method !== 'Network.requestWillBeSent' || !params.request?.postData) return;
      const body = JSON.parse(params.request.postData);
      if ((body.request ?? body).command === 'companion_status') statusReads++;
    });
    await visibility(true);
    const before = statusReads;
    await assign(external);
    await delay(350);
    assert.equal(statusReads, before, 'A hidden editor must not request status for a background assignment');
    assert.equal(await selected(external), false);
    await visibility(false);
    await until(() => selected(external));
    assert.equal(await selected(local), false, 'The fresh baseline must preserve the deliberate local uncheck');

    await click('.compiled-designer button.primary-button', 'Save');
    await until(() => page.evaluate(`document.body.innerText.includes('Saved and assigned')`));
    const saved = await companion('companion_status');
    assert.deepEqual(saved.host.instances.filter(row => row.publicationHash === publicationHash)
      .map(row => row.companionId), [external]);
    await click('.compiled-designer button.header-back');
    await open();
    await until(() => selected(external));
    assert.equal(await selected(local), false);
    console.log(JSON.stringify({transport: browserUrl ? 'browser-dev' : 'native', visibleCommit: true,
      hiddenReads: 0, localEditPreserved: true, savedAndReopened: true}));
  } finally {
    stopTrace();
    await visibility(false);
    try {
      await browser?.close();
      for (const companionId of peerIds) await companion('companion_revoke', {companionId});
      if (listener) {
        await companion('companion_pair_cancel');
        await companion('companion_disable', {expectedSettingsRevision: listener.settingsRevision});
      }
    } finally { await page.close(); }
  }
});
