import assert from 'node:assert/strict';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_USB_DETAILS_BROWSER_URL;

// Author just the USB endpoint. Other commands and streams continue to use
// the running app's gateway, and both real observation transports parse and
// acknowledge the fixture deliveries.
function installUsbFixture(browser) {
  const name = 'USB visibility fixture';
  const serial = '00112233445566778899aabb';
  const key = serial + ':COM8';
  const identity = {model:'device-template', board:'RP2040', profile:name,
    moduleId:'4f534744'+serial, serial, protocol:3, buttonCount:1, axisCount:1, hatCount:0};
  const route = {moduleId:identity.moduleId, serial, connectionGeneration:1, boot:7, configurationGeneration:1};
  const config = {generation:1, savedGeneration:1, owner:0, claimExpiresMs:0, dirty:false, pendingApply:false,
    valid:true, view:'active', config:{name,expanderMask:0,buttons:[[0,0,0]],axes:[],hats:[],encoders:[],selectors:[]},
    layout:{buttons:1,axes:0,hats:0,reportBytes:1}, descriptorHex:'0501', revision:1, revisionExhausted:false};
  const metadata = {key, connected:true, port:'COM8', identity, route, config, observedAtMs:20, error:null};
  const module = {key, port:{path:'COM8',friendlyName:name}, present:true, automatic:false, admissible:true, diagnostics:[],
    snapshot:{...metadata, claimedBy:null, draft:null, actions:null, events:null}};
  const trace = window.__usbDetails = {opens:[], closes:[], acks:[], reads:0, inventories:0, axis:14000};
  const connections = new Map(), registrations = new Map();
  const isTopic = topic => /^usb\.(inputs|metadata)\.[0-9a-f]{64}$/.test(topic ?? '');
  const subscribe = topic => {
    const subscriptionId = 'usb-fixture-' + (trace.opens.length + 1);
    trace.opens.push({topic, subscriptionId});
    const input = topic.startsWith('usb.inputs.');
    const source = input ? 'usb-inputs' : 'usb-metadata';
    const payload = input ? {key,route,ready:true,error:null,state:{ok:true,schemaVersion:1,stream:1,head:2,oldest:1,
      overflow:0,buttonCount:1,held:[false],contactButtonCount:1,maintainedButtons:[],encoderButtons:[],selectorButtons:[],
      axisCount:1,axes:[trace.axis],axisNeutral:[32767],axisUsage:[48],axisAvailable:[true],sampledAtMs:20}} : metadata;
    return {subscriptionId,delivery:'1',frame:{kind:'snapshot',continuity:'baseline',topic,source,session:'usb-fixture',
      clockId:source+':capture',generation:'1',revision:'1',sequence:'1',capturedUs:'1',payload}};
  };
  const gateway = request => {
    if (request?.command === 'module_usb_list') {
      trace.inventories++;
      return {ok:true,state:{moduleUsb:{offset:0,total:1,modules:[module]}}};
    }
    if (request?.command === 'module_usb_status' && request.key === key) {
      trace.reads++;
      return {ok:true,state:{moduleUsb:{module}}};
    }
  };
  const original = window.fetch;
  const json = body => new Response(JSON.stringify(body), {headers:{'content-type':'application/json','Tauri-Response':'ok'}});
  if (!browser) {
    window.fetch = async (url, options) => {
      const endpoint = new URL(url, location.href);
      if (endpoint.hostname !== 'ipc.localhost' && endpoint.protocol !== 'ipc:') return original(url,options);
      const command = endpoint.pathname.slice(1);
      const args = typeof options?.body === 'string' ? JSON.parse(options.body) : undefined;
      const fixture = command === 'gateway_request' && gateway(args?.request);
      if (fixture) return json(fixture);
      if (command === 'observation_subscribe' && isTopic(args?.topic)) {
        const envelope = subscribe(args.topic);
        const channel = Number(args.channel.split(':').at(-1));
        window.__TAURI_INTERNALS__.runCallback(channel,{index:0,message:envelope});
        window.__TAURI_INTERNALS__.runCallback(channel,{index:1,end:true});
        return json(envelope.subscriptionId);
      }
      if (args?.subscriptionId?.startsWith('usb-fixture-')) {
        if (command === 'observation_unsubscribe') trace.closes.push(args.subscriptionId);
        if (command === 'observation_acknowledge') trace.acks.push(args.subscriptionId);
        return json(null);
      }
      return original(url,options);
    };
    trace.restore = () => { window.fetch = original; };
    return;
  }
  window.fetch = async (url, options) => {
    const path = new URL(url, location.href).pathname;
    const body = path.startsWith('/api/') && typeof options?.body === 'string' ? JSON.parse(options.body) : undefined;
    if (path === '/api/gateway/') {
      const fixture = gateway(body);
      if (fixture) return json(fixture);
    }
    if (path === '/api/observe/subscribe' && isTopic(body?.topic)) {
      const envelope = subscribe(body.topic);
      registrations.set(body.requestId, envelope.subscriptionId);
      connections.get(body.connectionId)({requestId:body.requestId,envelope});
      return json({ok:true,subscriptionId:envelope.subscriptionId});
    }
    if (path === '/api/observe/unsubscribe' && registrations.has(body?.requestId)) {
      trace.closes.push(registrations.get(body.requestId));
      registrations.delete(body.requestId);
      return json({ok:true});
    }
    if (path === '/api/observe/ack' && body?.subscriptionId?.startsWith('usb-fixture-')) {
      trace.acks.push(body.subscriptionId);
      return json({ok:true});
    }
    const response = await original(url, options);
    if (path !== '/api/observe/' || !body?.connectionId || !response.ok) return response;
    const reader = response.body.getReader();
    const stream = new ReadableStream({
      async start(controller) {
        const encoder = new TextEncoder(), decoder = new TextDecoder();
        const send = line => controller.enqueue(encoder.encode(line + '\n'));
        connections.set(body.connectionId, value => send(JSON.stringify(value)));
        let remaining = '';
        try {
          while (true) {
            const {value,done} = await reader.read();
            if (done) break;
            const lines = (remaining + decoder.decode(value,{stream:true})).split('\n');
            remaining = lines.pop();
            for (const line of lines) send(line);
          }
          controller.close();
        } catch (error) { controller.error(error); }
        finally { connections.delete(body.connectionId); }
      },
      cancel() { return reader.cancel(); },
    });
    return new Response(stream,{headers:response.headers,status:response.status});
  };
  trace.restore = () => { window.fetch = original; };
}

test('Direct USB retires both readers and reopens on the current input baseline', {
  skip: !browserUrl && process.env.OSG_USB_DETAILS_NATIVE !== '1', timeout: 60000,
}, async () => {
  const fixture = '(' + installUsbFixture.toString() + ')(' + Boolean(browserUrl) + ')';
  const page = browserUrl ? await browserPage(browserUrl, fixture) : await nativePage();
  const trace = () => page.evaluate('JSON.parse(JSON.stringify(window.__usbDetails))');
  const click = selector => until(() => page.evaluate(`(() => {
    const target = document.querySelector(${JSON.stringify(selector)});
    if (!target || target.closest('[hidden]')) return false;
    target.click();
    return true;
  })()`));
  const nav = name => click(`nav[aria-label="Main navigation"] button[title="${name}"]`);
  const openEndpoint = () => click('button[data-component-id][aria-label^="USB visibility fixture"]');
  const input = value => until(() => page.evaluate(`document.querySelector('.usb-input-status')?.textContent.includes('${value}')`));
  try {
    if (!browserUrl) await page.evaluate(fixture);
    await nav('Devices');
    await until(() => page.evaluate(`(() => {
      const refresh = [...document.querySelectorAll('.system-inventory button')]
        .find(button => button.textContent.trim() === 'Refresh');
      if (!refresh || refresh.disabled) return false;
      refresh.click();
      return true;
    })()`));
    await openEndpoint();
    await input(14000);
    const first = await until(trace, state => state?.acks.length === 2);
    assert.equal(first.opens.length, 2);
    assert.equal(first.reads, 1);
    await nav('Games');
    await until(trace, state => state.closes.length === 2);
    await page.evaluate('window.__usbDetails.axis = 24000');
    assert.equal((await trace()).opens.length, 2);
    await nav('Devices');
    await openEndpoint();
    await input(24000);
    await until(trace, state => state.acks.length === 4);
    await page.evaluate(`Object.defineProperty(document,'hidden',{configurable:true,value:true});
      document.dispatchEvent(new Event('visibilitychange'))`);
    await until(trace, state => state.closes.length === 4);
    await page.evaluate('window.__usbDetails.axis = 34000; delete document.hidden; document.dispatchEvent(new Event("visibilitychange"))');
    await input(34000);
    await nav('Games');
    const final = await until(trace, state => state.closes.length === 6);
    assert.equal(final.opens.length, 6);
    assert.equal(final.reads, 2);
    assert.deepEqual(final.closes.sort(), final.opens.map(open => open.subscriptionId).sort());
    console.log(JSON.stringify({transport:browserUrl?'browser-dev':'native', opens:6, retirements:6, statusReads:2, baselines:[14000,24000,34000]}));
  } catch (error) {
    console.log(await trace().catch(() => undefined), await page.evaluate(`document.querySelector('[aria-label="System devices"]')?.innerText.slice(0,1500)`).catch(() => undefined));
    throw error;
  } finally {
    await page.evaluate('window.__usbDetails?.restore()').catch(() => {});
    await page.close();
  }
});
