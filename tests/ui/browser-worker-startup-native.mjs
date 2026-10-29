// Exercises real companion pairing/authentication against the running dev Cockpit.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {verifiedServerKey} from './native/browser-trust.mjs';

const {chromium}=createRequire(import.meta.url)(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');

test('browser companion connects when its shared-worker download stalls', {timeout:60000}, async()=>{
  const native=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const cockpit=native.contexts().flatMap(context=>context.pages()).find(page=>page.url().startsWith('http://127.0.0.1:1420/'));
  assert.ok(cockpit,'Start the actual native development Cockpit with CDP enabled.');
  const send=async(command,companion={})=>{
    const reply=await cockpit.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request}),{command,companion,clientId:'browser-worker-startup-native'});
    assert.equal(reply.ok,true,reply.error);
    return reply.state.companions;
  };
  let browser;
  const created=[];
  try{
    const initial=await send('companion_status');
    assert.equal(initial.listener.enabled,true);
    const certificate=(await send('companion_browser_certificate')).certificatePem;
    const spki=await verifiedServerKey(initial.browserUrl,certificate);
    browser=await chromium.launch({headless:true,channel:process.env.OSG_BROWSER_CHANNEL??'msedge',args:['--ignore-certificate-errors-spki-list='+spki]});
    for(const stalled of [false,true]){
      const context=await browser.newContext(),page=await context.newPage();
      let workerRequests=0;
      // Hold only the worker asset; HTML, auth and WSS use the real native listener.
      if(stalled)await context.route('**/connection-worker-*.js',async route=>{
        workerRequests++;
        await new Promise(resolve=>context.once('close',resolve));
        await route.abort().catch(()=>{});
      });
      const before=new Set((await send('companion_status')).host.clients.map(client=>client.companionId));
      const offer=await send('companion_pair_offer',{kind:'browser'});
      await page.goto(offer.pairing.browserUrl);
      await page.getByText('No panel assigned',{exact:true}).first().waitFor({timeout:12000});
      const state=await send('companion_status');
      const client=state.host.clients.find(client=>!before.has(client.companionId));
      assert.ok(client,'Real browser pairing must be persisted by Cockpit.');
      created.push(client.companionId);
      assert.equal(client.connected,true,'The real native listener must authenticate the browser.');
      if(stalled)assert.equal(workerRequests,1,'The shared-worker download must actually be held.');
      await context.close();
    }
  }finally{
    await browser?.close();
    for(const companionId of created)await send('companion_revoke',{companionId});
    await native.close();
  }
});
