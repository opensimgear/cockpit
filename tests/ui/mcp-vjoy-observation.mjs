// Real HTTP observation clients used by the Windows Raw Input fixture.
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';

function frames(response) {
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  return {
    async next(predicate) {
      for (;;) {
        const newline = buffer.indexOf('\n');
        if (newline < 0) {
          const {value, done} = await reader.read();
          assert.equal(done, false, 'MCP stream ended before the expected frame');
          buffer += decoder.decode(value, {stream:true});
          continue;
        }
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        if (!line.startsWith('data:')) continue;
        const event = JSON.parse(line.slice(5));
        assert.ok(!event.error, JSON.stringify(event.error));
        if (predicate(event)) return event;
      }
    },
    close: () => reader.cancel(),
  };
}

export async function mcpVjoyObservation(page, owner, {nodeId = 'fixture-vjoy', capturePhysical = true} = {}) {
  const invoke = (command, args = {}) => page.evaluate(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`,
  );
  const initial = await invoke('mcp_settings_get');
  const physicalSources = () => page.evaluate(`(async()=>{
    const {gatewayRequest}=await import('/src/lib/gateway.ts');
    const {controllers}=await gatewayRequest({command:'controller_list',offset:0,limit:32});
    return controllers.sources.filter(source=>source.online&&source.baselineReady&&source.vendorId!==0x1234);
  })()`);
  const settings = async enabled => {
    const current = await invoke('mcp_settings_get');
    return invoke('mcp_settings_update', {settings:{enabled, port:initial.port, expectedSettingsRevision:current.settingsRevision}});
  };
  const abort = new AbortController();
  const streams = [];
  let credential;
  const close = async () => {
    abort.abort();
    await Promise.allSettled(streams.map(stream => stream.close()));
    try {
      if (credential) {
        const current = await invoke('mcp_settings_get');
        await invoke('mcp_credential_revoke', {id:credential.credential.id, expectedSettingsRevision:current.settingsRevision});
      }
    } finally {
      await settings(initial.enabled);
    }
  };
  try {
    credential = await invoke('mcp_credential_create', {
      name:'Temporary slow-reader fixture', scope:'write', expectedSettingsRevision:initial.settingsRevision,
    });
    const {endpoint} = await settings(true);
    const version = '2026-07-28';
    const post = (method, params) => fetch(endpoint, {
      method:'POST',
      signal:AbortSignal.any([abort.signal, AbortSignal.timeout(30000)]),
      headers:{
        Authorization:`Bearer ${credential.token}`,
        Accept:'application/json, text/event-stream',
        'Content-Type':'application/json',
        'MCP-Protocol-Version':version,
        'Mcp-Method':method,
        ...(params.name ? {'Mcp-Name':params.name} : {}),
      },
      body:JSON.stringify({jsonrpc:'2.0', id:randomUUID(), method, params:{...params, _meta:{
        'io.modelcontextprotocol/protocolVersion':version,
        'io.modelcontextprotocol/clientInfo':{name:'Cockpit Raw Input isolation fixture',version:'1'},
        'io.modelcontextprotocol/clientCapabilities':{},
      }}}),
    });
    const topic = await page.evaluate(`import('/src/lib/profile-samples/models.ts').then(
      ({profileNodeSamplesTopic}) => profileNodeSamplesTopic(${JSON.stringify(owner)},${JSON.stringify(nodeId)}))`);
    const uri = 'cockpit://observations/' + topic;
    const sampleFrame = event => event.params?._meta?.['opensimgear.com/observation'];
    const subscribe = async () => {
      const response = await post('subscriptions/listen', {notifications:{resourceSubscriptions:[uri]}});
      assert.equal(response.status, 200);
      assert.match(response.headers.get('content-type'), /text\/event-stream/);
      const stream = frames(response);
      streams.push(stream);
      await stream.next(event => event.method === 'notifications/subscriptions/acknowledged');
      await stream.next(event => event.params?.uri === uri && sampleFrame(event)?.kind === 'snapshot');
      return stream;
    };
    const slow = await subscribe(); // Keep the first response open with no further reads until Stop.
    const slowPausedAt = performance.now();
    const captureBefore = capturePhysical
      ? (await physicalSources()).sort((a,b) => b.controlCount-a.controlCount)[0]
      : undefined;
    const fast = await subscribe();
    const retired = event => event.params?.uri === uri && sampleFrame(event)?.kind === 'loss';
    return {
      async sample(value, portId = 'button-1') {
        const event = await fast.next(event => event.params?.uri === uri && sampleFrame(event)?.payload?.ports?.some(
          port => port.endpoint.portId === portId && port.sample.value?.value === value,
        ));
        const frame = sampleFrame(event);
        const port = frame.payload.ports.find(port => port.endpoint.portId === portId);
        return {
          value: port.sample.value.value,
          kind: frame.kind,
          topic: frame.topic,
          portId,
          ...(frame.generation !== undefined ? {generation: frame.generation} : {}),
          ...(frame.revision !== undefined ? {revision: frame.revision} : {}),
        };
      },
      async stop() {
        const startedAt = performance.now();
        const response = await post('tools/call', {name:'profile_stop', arguments:{operationId:randomUUID()}});
        assert.equal(response.status, 200);
        assert.equal((await response.json()).result.structuredContent.ok, true);
        const stopReplyMs = performance.now() - startedAt;
        await fast.next(retired);
        return stopReplyMs;
      },
      async resumeSlow() {
        const captureAfter = captureBefore && (await physicalSources()).find(source => source.id === captureBefore.id);
        const unreadMs = performance.now() - slowPausedAt;
        await slow.next(retired);
        return {unreadMs, captureBefore, captureAfter};
      },
      close,
    };
  } catch (error) {
    await close();
    throw error;
  }
}
