import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import test from 'node:test';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { nativePage, until } from '../../scripts/reactive/cdp.mjs';

test('desktop status, recording and settings agree across MCP and the portal', {
  skip: process.env.OSG_MCP_DESKTOP_STATUS_NATIVE !== '1', timeout: 60000,
}, async () => {
  const page = await nativePage();
  const client = new Client({name:'Desktop status parity fixture',version:'1'}, {capabilities:{}});
  const writer = new Client({name:'Desktop settings parity fixture',version:'1'}, {capabilities:{}});
  const invoke = (command, args = {}) => page.evaluate(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`,
  );
  const native = async (command, fields = {}) => {
    const result = await invoke('gateway_request', {request:{command,clientId:'desktop-status-parity',...fields}});
    assert.equal(result.ok,true,result.error);
    return result.state;
  };
  const settings = () => invoke('mcp_settings_get');
  const configure = async (enabled, port) => invoke('mcp_settings_update', {settings:{
    enabled,port,expectedSettingsRevision:(await settings()).settingsRevision,
  }});
  const call = async (client, name, args) => {
    const reply = await client.callTool({name,arguments:args});
    assert.ok(!reply.isError, JSON.stringify(reply.structuredContent));
    assert.equal(reply.structuredContent.ok,true);
    return reply.structuredContent.data;
  };
  const desktopSettings = () => call(writer,'local_settings_get',{scope:'desktop'});
  const save = (current, enabled) => call(writer,'local_settings_save',{
    scope:'desktop',expectedSettingsRevision:current.revision,expectedActiveOwner:null,
    settings:{scope:current.settings.scope,enabled},operationId:randomUUID(),
  });
  let initial, credential, writeCredential, restore, enabledByFixture = false;
  try {
    initial = await settings();
    credential = await invoke('mcp_credential_create', {
      name:'Temporary desktop status reader',scope:'read',expectedSettingsRevision:initial.settingsRevision,
    });
    const status = initial.enabled ? initial : await configure(true, initial.port);
    enabledByFixture = !initial.enabled;
    const transport = new StreamableHTTPClientTransport(new URL(status.endpoint), {
      requestInit:{headers:{Authorization:`Bearer ${credential.token}`}},
    });
    await client.connect(transport);
    const {tools} = await client.listTools();
    const descriptor = tools.find(tool => tool.name === 'profile_desktop_status');
    assert.ok(descriptor,'Tool must be discoverable with a read credential');
    assert.equal(descriptor.inputSchema.additionalProperties,false);
    assert.equal(descriptor.annotations.readOnlyHint,true);
    const recorder = tools.find(tool => tool.name === 'profile_desktop_preview');
    assert.equal(recorder?.annotations.readOnlyHint,true);
    assert.equal(recorder.inputSchema.properties.frames.maxItems,256);
    const before = await native('profile_status');
    const {desktop} = await native('profile_desktop_status');
    const reply = await client.callTool({name:'profile_desktop_status',arguments:{}});
    assert.ok(!reply.isError);
    const result = reply.structuredContent;
    assert.equal(result.ok,true);
    for (const field of ['enabled','qualified','state','reason','scope','error']) {
      assert.deepEqual(result.data[field],desktop[field],field);
    }
    for (const field of ['application','recentApplication','applications']) {
      assert.ok(!Object.hasOwn(result.data,field),field);
    }
    if (desktop.runtime === null) assert.equal(result.data.runtime,null);
    assert.deepEqual((await native('profile_status')).profiles.profileSession.owner,before.profiles.profileSession.owner);
    assert.equal(before.profiles.profileSession.owner,null,'The settings mutation requires an idle dev profile');
    const {resources} = await client.listResources();
    assert.ok(resources.some(resource => resource.uri==='cockpit://observations/settings.desktop'));
    assert.ok(resources.some(resource => resource.uri==='cockpit://observations/profiles.desktop'));
    await page.evaluate(`(async()=>{
      const {SharedObservations}=await import('/src/lib/streams/shared.ts');
      const observations=new SharedObservations();
      window.__desktopReadinessFixture={observations,state:null};
      observations.subscribe('profiles.desktop',state=>{window.__desktopReadinessFixture.state=state;});
    })()`);
    const readiness=await until(()=>page.evaluate('window.__desktopReadinessFixture.state'),state=>state?.status==='ready');
    assert.deepEqual(readiness.frame.payload,{state:desktop.state});
    await until(()=>page.evaluate(`(()=>{const button=[...document.querySelectorAll('nav[aria-label="Main navigation"] button')].find(item=>item.textContent.trim()==='Debug');if(!button)return false;button.click();return true;})()`));
    await until(()=>page.evaluate(`(()=>{const tab=[...document.querySelectorAll('[role="tab"]')].find(item=>item.textContent.trim()==='Runtime & recovery');if(!tab)return false;tab.click();return true;})()`));
    const desktopDetails = '[...document.querySelectorAll("details")].find(item=>item.querySelector("summary")?.textContent.trim()==="Native runtime and desktop output details")';
    await until(()=>page.evaluate(`(()=>{const details=${desktopDetails};if(!details)return null;details.open=true;return details.querySelector('strong')?.textContent;})()`),
      state=>state===desktop.state);
    writeCredential = await invoke('mcp_credential_create', {
      name:'Temporary desktop settings writer',scope:'write',expectedSettingsRevision:(await settings()).settingsRevision,
    });
    await writer.connect(new StreamableHTTPClientTransport(new URL(status.endpoint), {
      requestInit:{headers:{Authorization:`Bearer ${writeCredential.token}`}},
    }));
    const seed = await call(writer,'profile_prepare_changes',{name:'Temporary desktop recording',expectedRevision:null});
    const document = await page.evaluate(`(async()=>{
      const {desktopPreset}=await import('/src/lib/profile-desktop.ts');
      const document=${JSON.stringify(seed.document)};
      const {output}=desktopPreset('key-hold','recording-key');
      const port=output.ports[0];
      document.desktopOutputs=[output];
      document.resources=[{id:'recording-source',name:'Synthetic source',kind:'device',required:false,
        ports:[{...port,id:'button',name:'Button',direction:'output'}]}];
      document.bindings=[{id:'recorded-hold',from:{nodeId:'recording-source',portId:'button'},to:{nodeId:output.id,portId:port.id}}];
      return document;
    })()`);
    const prepared = await call(writer,'profile_prepare_changes',{document,assignments:[],expectedRevision:null});
    assert.equal(prepared.validation.valid,true,JSON.stringify(prepared.validation));
    const compilationId = prepared.compilation.compilationId;
    const {items:sources} = await call(client,'compilation_inspect',{compilationId,section:'sources'});
    assert.equal(sources.length,1);
    const samples = await page.evaluate(`(async()=>{
      const {desktopPreviewFrames,previewSourceKey}=await import('/src/lib/profile-desktop-preview.ts');
      const sources=${JSON.stringify(sources)};
      return desktopPreviewFrames(sources,previewSourceKey(sources[0].endpoint),true);
    })()`);
    const recordingArgs = {compilationId,...samples};
    const recorded = await call(client,'profile_desktop_preview',recordingArgs);
    assert.deepEqual(recorded,(await native('profile_desktop_preview',recordingArgs)).profileDesktopPreview);
    assert.equal(recorded.nativeSubmission,false);
    const events = recorded.operations.flatMap(operation=>operation.events);
    assert.ok(events.some(event=>event.kind==='key'&&event.down===true));
    assert.ok(events.some(event=>event.kind==='key'&&event.down===false));
    const missing = await client.callTool({name:'profile_desktop_preview',arguments:{compilationId:randomUUID()}});
    assert.ok(missing.isError);
    assert.equal((await native('profile_status')).profiles.profileSession.owner,null);
    const baseline = await desktopSettings();
    assert.equal(baseline.settings.enabled,desktop.enabled);
    await page.evaluate(`[...document.querySelectorAll('button')].find(button=>button.textContent.trim()==='Cockpit')?.click()`);
    await until(() => page.evaluate(`(()=>{const button=[...document.querySelectorAll('.sidebar-link')].find(item=>item.textContent.trim()==='Settings');if(!button)return false;button.click();return true;})()`));
    await until(() => page.evaluate(`(()=>{const button=document.getElementById('settings-tab-outputs');if(!button)return false;button.click();return true;})()`));
    const desktopToggle = 'document.querySelector(\'input[aria-describedby="desktop-output-help"]\')';
    const toggleState = () => page.evaluate(`${desktopToggle} && {enabled:${desktopToggle}.checked,disabled:${desktopToggle}.disabled}`);
    await until(toggleState,state=>state?.enabled===desktop.enabled&&!state.disabled);
    restore = desktop.enabled;
    await save(baseline,!restore);
    await until(toggleState,state=>state?.enabled===!restore&&!state.disabled);
    await page.evaluate(`${desktopToggle}.click()`);
    await until(toggleState,state=>state?.enabled===restore&&!state.disabled);
    assert.equal((await desktopSettings()).settings.enabled,restore);
    restore = undefined;
  } finally {
    try {
      try {
        if (restore !== undefined) {
          const current = await desktopSettings();
          if (current.settings.enabled !== restore) await save(current,restore);
        }
      }
      finally {
        await Promise.allSettled([client.close(),writer.close()]);
        for (const item of [credential,writeCredential]) {
          if (item) await invoke('mcp_credential_revoke', {
            id:item.credential.id,expectedSettingsRevision:(await settings()).settingsRevision,
          });
        }
      }
    } finally {
      try { if (enabledByFixture) await configure(initial.enabled, initial.port); }
      finally {
        await page.evaluate('window.__desktopReadinessFixture?.observations.close();delete window.__desktopReadinessFixture;').catch(()=>{});
        page.close();
      }
    }
  }
});
