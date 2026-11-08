import assert from 'node:assert/strict';
import test from 'node:test';
import { browserPage, nativePage, until } from '../../scripts/reactive/cdp.mjs';

const nativeEnabled = process.env.OSG_OBSERVATION_TERMINAL_REOPEN_NATIVE === '1';
const browserUrl = process.env.OSG_OBSERVATION_TERMINAL_REOPEN_BROWSER_URL;
const topic = 'preferences.theme';
const privateDatabase = process.env.OSG_OBSERVATION_REOPEN_PRIVATE_DATABASE === '1';

async function browserGateway(origin, request) {
  const response = await fetch(`${origin}/api/gateway/`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'x-osg-client': '1', Origin: origin },
    body: JSON.stringify({ ...request, clientId: 'terminal-reopen-acceptance' }),
  });
  const result = await response.json();
  assert.equal(result.ok, true, result.error);
  return result.state;
}

test('browser terminal topic reopens for an existing and a new listener', {
  skip: !browserUrl,
  timeout: 90_000,
}, async () => {
  assert.ok(privateDatabase, 'Use a private Cockpit database for the preference commits');
  const origin = new URL(browserUrl).origin;
  const original = (await browserGateway(origin, { command: 'ui_preferences_get', key: 'theme' })).uiPreferences;
  const cockpit = await browserPage(browserUrl);
  await until(() => cockpit.evaluate(`document.title === 'OpenSimGear Cockpit'`));
  await cockpit.evaluate(`(async()=>{
    const {SharedObservations}=await import('/src/lib/streams/shared.ts');
    const {createBrowserObservationTransport}=await import('/src/lib/streams/browser.ts');
    const calls={streamOpens:0,subscribes:[],unsubscribes:[],ackAttempts:0,failedAcks:0,ackSuccesses:0};
    let armed=false;
    const realFetch=window.fetch.bind(window);
    const transport=createBrowserObservationTransport({fetch:async(url,args={})=>{
      const path=new URL(String(url),location.href).pathname;
      if(path==='/api/observe/')calls.streamOpens++;
      const body=args.body?JSON.parse(args.body):{};
      if(path==='/api/observe/ack'){
        calls.ackAttempts++;
        if(armed&&calls.failedAcks===0){calls.failedAcks++;return new Response('',{status:503});}
      }
      const response=await realFetch(url,args);
      if(path==='/api/observe/subscribe'&&response.ok){
        const value=await response.clone().json();calls.subscribes.push({requestId:body.requestId,topic:body.topic,token:value.subscriptionId});
      }
      if(path==='/api/observe/ack'&&response.ok)calls.ackSuccesses++;
      if(path==='/api/observe/unsubscribe'&&response.ok){
        calls.unsubscribes.push({requestId:body.requestId,token:calls.subscribes.find(item=>item.requestId===body.requestId)?.token});
      }
      return response;
    }});
    const observations=new SharedObservations(transport);
    const owned=window.__terminalReopen={calls,observations,first:[],second:[],other:[],armFailure(){armed=true;}};
    owned.stopFirst=observations.subscribe('preferences.theme',value=>owned.first.push(value));
    owned.stopOther=observations.subscribe('preferences.progress',value=>owned.other.push(value));
  })()`);
  let finalEvents;
  let finalUnsubscribes;
  try {
    const initial = await until(() => cockpit.evaluate(`({a:window.__terminalReopen.first.at(-1),other:window.__terminalReopen.other.at(-1),calls:window.__terminalReopen.calls})`),
      value => value.a?.status === 'ready' && value.other?.status === 'ready' && value.calls.ackSuccesses >= 2);
    assert.equal(initial.a.frame.kind, 'snapshot');
    await cockpit.evaluate(`window.__terminalReopen.armFailure()`);
    const changed = (await browserGateway(origin, {
      command: 'ui_preferences_save', key: 'theme', expectedRevision: original.revision, value: !original.value,
    })).uiPreferences;
    await until(() => cockpit.evaluate(`window.__terminalReopen.first.at(-1)?.status === 'error'`));
    const retired = await until(() => cockpit.evaluate(`window.__terminalReopen.calls.unsubscribes.length`), value => value === 1);
    assert.equal(retired, 1, 'Failed ACK retires and unsubscribes the original token');
    const terminal = await cockpit.evaluate(`({state:window.__terminalReopen.first.at(-1),calls:window.__terminalReopen.calls})`);
    assert.equal(terminal.state.frame, undefined, 'Terminal state withdraws the prior ready frame');
    assert.equal(terminal.calls.failedAcks, 1);
    assert.equal(terminal.calls.unsubscribes[0].token,
      terminal.calls.subscribes.find(item => item.topic === topic)?.token, 'The failed topic token was retired');
    const otherState = await cockpit.evaluate(`window.__terminalReopen.other.at(-1)`);
    assert.equal(otherState.status, 'ready', 'Unrelated reader remains ready after exact-topic failure');
    const committed = (await browserGateway(origin, {
      command: 'ui_preferences_save', key: 'theme', expectedRevision: changed.revision, value: original.value,
    })).uiPreferences;
    await cockpit.evaluate(`window.__terminalReopen.stopSecond=window.__terminalReopen.observations.subscribe('preferences.theme',value=>window.__terminalReopen.second.push(value))`);
    try {
      const ready = await until(() => cockpit.evaluate(`({a:window.__terminalReopen.first.at(-1),b:window.__terminalReopen.second.at(-1),calls:window.__terminalReopen.calls})`),
        value => value.a?.status === 'ready' && value.b?.status === 'ready'
          && value.a.frame?.payload?.revision === String(committed.revision)
          && value.b.frame?.payload?.revision === String(committed.revision));
      assert.equal(ready.a.frame.kind, 'snapshot');
      assert.equal(ready.b.frame.kind, 'snapshot');
      assert.equal(ready.a.frame.session, ready.b.frame.session);
      assert.equal(ready.calls.streamOpens, 1, 'The unrelated reader preserves one physical multiplexed stream');
      assert.equal(ready.calls.subscribes.length, 3, 'One progress subscription and two theme subscriptions');
      assert.equal(ready.calls.subscribes.filter(item => item.topic === topic).length, 2, 'Exactly one fresh theme subscription');
    } finally {
      await cockpit.evaluate(`window.__terminalReopen.stopFirst();window.__terminalReopen.stopSecond()`);
    }
    finalUnsubscribes = await until(() => cockpit.evaluate(`window.__terminalReopen.calls.unsubscribes.length`), value => value === 2);
    finalEvents = await cockpit.evaluate(`({a:window.__terminalReopen.first,b:window.__terminalReopen.second,unsubscribes:window.__terminalReopen.calls.unsubscribes,subscriptions:window.__terminalReopen.calls.subscribes})`);
    assert.deepEqual(finalEvents.unsubscribes.map(item => item.token),
      finalEvents.subscriptions.filter(item => item.topic === topic).map(item => item.token), 'Both theme tokens were accepted by unsubscribe');
    await cockpit.evaluate(`window.__terminalReopen.stopOther();window.__terminalReopen.observations.close()`);
  } finally {
    await cockpit.evaluate(`window.__terminalReopen?.observations.close()`);
    const current = (await browserGateway(origin, { command: 'ui_preferences_get', key: 'theme' })).uiPreferences;
    if (current.value !== original.value) await browserGateway(origin, {
      command: 'ui_preferences_save', key: 'theme', expectedRevision: current.revision, value: original.value,
    });
    await cockpit.close();
  }
  assert.equal(finalUnsubscribes, 2, 'Both theme tokens are unsubscribed while the unrelated reader keeps the stream open');
  assert.equal(finalEvents.a.filter(value => value.status === 'ready' && value.frame?.kind === 'snapshot').length, 2);
  assert.equal(finalEvents.b.filter(value => value.status === 'ready' && value.frame?.kind === 'snapshot').length, 1);
  console.log(JSON.stringify({ transport: 'browser-dev', failedAcks: 1, replacementTopics: 1, themeUnsubscribes: 2 }));
});

test('native terminal topic reopens for an existing and a new listener', {
  skip: !nativeEnabled,
  timeout: 90_000,
}, async () => {
  assert.ok(privateDatabase, 'Use a private Cockpit database for the preference commits');
  const page = await nativePage();
  let original;
  try {
    await until(() => page.evaluate(`document.title === 'OpenSimGear Cockpit' && !!window.__TAURI_INTERNALS__`));
    original = await page.evaluate(`(async()=>{
      const result=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'ui_preferences_get',key:'theme',clientId:'terminal-reopen-acceptance'}});
      if(!result.ok)throw Error(result.error);return result.state.uiPreferences;
    })()`);
    await page.evaluate(`(async()=>{
      const [{SharedObservations},{createNativeObservationTransport},core]=await Promise.all([
        import('/src/lib/streams/shared.ts'),import('/src/lib/streams/native.ts'),import('/node_modules/@tauri-apps/api/core.js')
      ]);
      const raw=(command,args)=>window.__TAURI_INTERNALS__.invoke(command,args);
      const calls={subscribeAttempts:0,successfulOpens:0,openedSubscriptions:[],unsubscribes:[],ackAttempts:0,failedAcks:0,ackSuccesses:0};
      let armed=false;
      const invoke=async(command,args)=>{
        if(command==='observation_subscribe'){
          calls.subscribeAttempts++;
          const token=await raw(command,args);calls.successfulOpens++;calls.openedSubscriptions.push({topic:args.topic,token});return token;
        }
        if(command==='observation_acknowledge'){
          calls.ackAttempts++;
          if(armed&&calls.failedAcks===0){calls.failedAcks++;throw Error('Injected one-shot observation ACK failure');}
        }
        const result=await raw(command,args);
        if(command==='observation_acknowledge')calls.ackSuccesses++;
        if(command==='observation_unsubscribe')calls.unsubscribes.push(args.subscriptionId);
        return result;
      };
      const transport=createNativeObservationTransport({invoke,channel(receive){const channel=new core.Channel();channel.onmessage=receive;return channel;}});
      const observations=new SharedObservations(transport);
      const owned=window.__terminalReopen={observations,calls,first:[],second:[],other:[],armFailure(){armed=true;}};
      owned.stopFirst=observations.subscribe('preferences.theme',value=>owned.first.push(value));
      owned.stopOther=observations.subscribe('preferences.progress',value=>owned.other.push(value));
    })()`);
    const initial = await until(() => page.evaluate(`({a:window.__terminalReopen.first.at(-1),other:window.__terminalReopen.other.at(-1),calls:window.__terminalReopen.calls})`),
      value => value.a?.status === 'ready' && value.other?.status === 'ready' && value.calls.ackSuccesses >= 2);
    assert.equal(initial.a.frame.kind, 'snapshot');
    await page.evaluate(`window.__terminalReopen.armFailure()`);
    const changed = await page.evaluate(`(async()=>{
      const result=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'ui_preferences_save',key:'theme',expectedRevision:${original.revision},value:${!original.value},clientId:'terminal-reopen-acceptance'}});
      if(!result.ok)throw Error(result.error);return result.state.uiPreferences;
    })()`);
    await until(() => page.evaluate(`window.__terminalReopen.first.at(-1)?.status === 'error'`));
    await until(() => page.evaluate(`window.__terminalReopen.calls.unsubscribes.length`), value => value === 1);
    const terminal = await page.evaluate(`({state:window.__terminalReopen.first.at(-1),calls:window.__terminalReopen.calls})`);
    assert.equal(terminal.state.frame, undefined, 'Terminal state withdraws the prior ready frame');
    assert.equal(terminal.calls.failedAcks, 1);
    assert.equal(terminal.calls.unsubscribes[0],
      terminal.calls.openedSubscriptions.find(value => value.topic === topic)?.token, 'The original backend token was retired');
    assert.equal((await page.evaluate(`window.__terminalReopen.other.at(-1)`)).status, 'ready', 'Unrelated reader stays ready');
    const committed = await page.evaluate(`(async()=>{
      const result=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'ui_preferences_save',key:'theme',expectedRevision:${changed.revision},value:${original.value},clientId:'terminal-reopen-acceptance'}});
      if(!result.ok)throw Error(result.error);return result.state.uiPreferences;
    })()`);
    await page.evaluate(`window.__terminalReopen.stopSecond=window.__terminalReopen.observations.subscribe('preferences.theme',value=>window.__terminalReopen.second.push(value))`);
    const ready = await until(() => page.evaluate(`({a:window.__terminalReopen.first.at(-1),b:window.__terminalReopen.second.at(-1),calls:window.__terminalReopen.calls})`),
      value => value.a?.status === 'ready' && value.b?.status === 'ready'
        && value.a.frame?.payload?.revision === String(committed.revision)
        && value.b.frame?.payload?.revision === String(committed.revision));
    assert.equal(ready.a.frame.kind, 'snapshot');
    assert.equal(ready.b.frame.kind, 'snapshot');
    assert.equal(ready.a.frame.session, ready.b.frame.session);
    assert.equal(ready.calls.subscribeAttempts, 3, 'Progress plus initial and retried theme subscriptions');
    assert.equal(ready.calls.successfulOpens, 3, 'Every subscription reached the real backend');
    assert.equal(ready.calls.openedSubscriptions.filter(value => value.topic === topic).length, 2, 'One original and one retried theme open');
    assert.equal(ready.calls.openedSubscriptions.filter(value => value.topic === 'preferences.progress').length, 1);
    await page.evaluate(`window.__terminalReopen.stopFirst();window.__terminalReopen.stopSecond();window.__terminalReopen.stopOther();window.__terminalReopen.observations.close()`);
    const closed = await until(() => page.evaluate(`window.__terminalReopen.calls.unsubscribes.length`), value => value === 3);
    assert.equal(closed, 3, 'Retired theme, fresh theme, and unrelated tokens are unsubscribed');
    const events = await page.evaluate(`({a:window.__terminalReopen.first,b:window.__terminalReopen.second})`);
    assert.equal(events.a.filter(value => value.status === 'ready' && value.frame?.kind === 'snapshot').length, 2);
    assert.equal(events.b.filter(value => value.status === 'ready' && value.frame?.kind === 'snapshot').length, 1);
    console.log(JSON.stringify({ transport: 'native', failedAcks: 1, replacementTopics: 1, unsubscribes: closed }));
  } finally {
    if (original) await page.evaluate(`(async()=>{
      const invoke=window.__TAURI_INTERNALS__.invoke;
      const current=await invoke('gateway_request',{request:{command:'ui_preferences_get',key:'theme',clientId:'terminal-reopen-cleanup'}});
      const value=current.state?.uiPreferences;
      if(value&&value.value!==${original.value})await invoke('gateway_request',{request:{command:'ui_preferences_save',key:'theme',expectedRevision:value.revision,value:${original.value},clientId:'terminal-reopen-cleanup'}});
      window.__terminalReopen?.observations.close();delete window.__terminalReopen;
    })()`).catch(() => {});
    page.close();
  }
});
