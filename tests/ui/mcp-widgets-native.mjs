// Real SDK client -> loopback MCP -> the running desktop's shared manager.
// CDP is used only by this integration test to inspect native IPC/UI agreement.
import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { softwareScenarios } from './mcp-software-scenarios.mjs';

const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(operation, description, timeout = 20000) {
  const deadline = Date.now() + timeout; let last;
  while (Date.now() < deadline) {
    try { const value = await operation(); if (value) return value; } catch (error) { last = error; }
    await pause(100);
  }
  throw new Error(`${description}${last ? `: ${last.message}` : ''}`);
}
async function nativePage() {
  const targets = await (await fetch(`${process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9227'}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
  const origin = process.env.OSG_NATIVE_DEV_ORIGIN ?? 'http://127.0.0.1:1420';
  const target = targets.find(target => target.type === 'page' && !target.url.includes('profile-activation-window') && (target.url.startsWith(`${origin}/`) || target.url.startsWith('http://tauri.localhost/')));
  assert.ok(target, 'Start the actual native dev Cockpit with WebView debugging before running this acceptance test.');
  const socket = new WebSocket(target.webSocketDebuggerUrl), pending = new Map(); let sequence = 0;
  await new Promise((resolve, reject) => { const timer = setTimeout(() => { socket.close(); reject(new Error('Native debugger connection timed out')); }, 8000); socket.addEventListener('open', () => { clearTimeout(timer); resolve(); }, { once: true }); socket.addEventListener('error', error => { clearTimeout(timer); reject(error); }, { once: true }); });
  socket.addEventListener('message', event => { const message = JSON.parse(event.data), task = pending.get(message.id); if (!task) return; clearTimeout(task.timer); pending.delete(message.id); message.error ? task.reject(new Error(message.error.message)) : task.resolve(message.result); });
  socket.addEventListener('close', () => { for (const task of pending.values()) { clearTimeout(task.timer); task.reject(new Error('Native debugger connection closed')); } pending.clear(); });
  const send = (method, params = {}) => new Promise((resolve, reject) => { const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, 45000); pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params })); });
  return {
    send,
    async evaluate(fn, ...args) { const reply = await send('Runtime.evaluate', { expression: `(${fn.toString()})(...${JSON.stringify(args)})`, awaitPromise: true, returnByValue: true }); if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.value ?? reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text); return reply.result.value; },
    async invoke(command, args = {}) { return this.evaluate((command, args) => window.__TAURI_INTERNALS__.invoke(command, args), command, args); },
    async native(request) { const response = await this.invoke('gateway_request', { request: { ...request, clientId: 'mcp-native-acceptance-ui' } }); assert.equal(response.ok, true, response.error); return response.state; },
    close() { for (const task of pending.values()) clearTimeout(task.timer); socket.close(); },
  };
}
async function connect(endpoint, token) {
  const client = new Client({ name: 'OpenSimGear live acceptance', version: '1.32.1' }, { capabilities: {} });
  const transport = new StreamableHTTPClientTransport(new URL(endpoint), { requestInit: { headers: { Authorization: `Bearer ${token}` } }, reconnectionOptions: { maxRetries: 0 } });
  await client.connect(transport); return client;
}
async function call(client, name, args = {}) {
  const reply = await client.callTool({ name, arguments: args }, undefined, { timeout: 135000 });
  const result = reply.structuredContent ?? JSON.parse(reply.content.find(part => part.type === 'text').text);
  return { ...result, isError: reply.isError ?? false };
}
async function ok(client, name, args = {}) { const result = await call(client, name, args); assert.equal(result.ok, true, `${name}: ${JSON.stringify(result)}`); return result.data; }
const mutation = args => ({ ...args, operationId: randomUUID() });


test('MCP widget saves replace plain sources in the running Widgets editor', {timeout:120000}, async()=>{
  const page=await until(()=>nativePage(),'Running cockpit debugging endpoint did not respond',30000);let settings;const credentials=[],clients=[];
  try {
    await until(()=>page.evaluate(()=>typeof window.__TAURI_INTERNALS__?.invoke==='function'&&!!document.querySelector('nav[aria-label="Main navigation"]')),'Native cockpit did not finish startup');
    settings=await page.invoke('mcp_settings_get');
    const write=await page.invoke('mcp_credential_create',{name:'Widget acceptance writer',scope:'write',expectedSettingsRevision:(await page.invoke('mcp_settings_get')).settingsRevision});credentials.push(write.credential.id);
    const read=await page.invoke('mcp_credential_create',{name:'Widget acceptance reader',scope:'read',expectedSettingsRevision:(await page.invoke('mcp_settings_get')).settingsRevision});credentials.push(read.credential.id);
    const status=await page.invoke('mcp_settings_update',{settings:{enabled:true,port:settings.port,expectedSettingsRevision:(await page.invoke('mcp_settings_get')).settingsRevision}});
    const writer=await connect(status.endpoint,write.token);clients.push(writer);
    const reader=await connect(status.endpoint,read.token);clients.push(reader);
    assert.ok((await writer.listTools()).tools.some(t=>t.name==='widget_save'));
    assert.ok(!(await reader.listTools()).tools.some(t=>t.name==='widget_save'));
    const guide=JSON.parse((await writer.readResource({uri:'cockpit://guide/widgets'})).contents[0].text);
    const data=guide.data??guide;
    const bundle=data.example;assert.ok(bundle);
    const id='mcp-widget-acceptance';
    const definition=bundle.definitions[0];definition.id=id;definition.title='MCP toggle acceptance';
    const current=await ok(reader,'widgets_list');const args=mutation({bundle,expectedRevision:current.revision});
    await assert.rejects(()=>reader.callTool({name:'widget_save',arguments:args}));
    const staleWorkspace=await page.invoke('persistence_workspace',{request:{operation:'read'}});
    const result=await call(writer,'widget_save',args);assert.equal(result.ok,true,JSON.stringify(result));
    assert.deepEqual(await call(writer,'widget_save',args),result,'Exact reconnect retry recovers original save');
    assert.equal((await call(writer,'widget_save',mutation({bundle,expectedRevision:(await ok(reader,'widgets_list')).revision}))).ok,true,'Saving the same ID replaces the current definition');
    assert.equal((await call(writer,'widget_save',mutation({bundle,expectedRevision:current.revision}))).ok,false,'A fresh operation cannot reuse a stale SQL library revision');
    await assert.rejects(()=>page.invoke('persistence_workspace',{request:{operation:'write',expectedRevision:staleWorkspace.revision,expectedLibraryRevision:staleWorkspace.libraryRevision,value:staleWorkspace.value}}),/revision conflict/);
    const acknowledged=await page.invoke('persistence_workspace',{request:{operation:'read'}});assert.ok(acknowledged.value.sources.some(source=>source.id===id));
    const malformed=structuredClone(bundle);malformed.definitions[0].scene[0].kind='script';
    assert.equal((await call(writer,'widget_save',mutation({bundle:malformed,expectedRevision:(await ok(reader,'widgets_list')).revision}))).ok,false);
    const listed=await ok(reader,'widgets_list');assert.equal(listed.widgets.filter(w=>w.id===id).length,1);assert.equal('version'in listed.widgets.find(w=>w.id===id),false);
    assert.equal((await ok(reader,'widget_get',{id})).definitions[0].title,definition.title);
    await page.send('Page.reload');
    await until(()=>page.evaluate(()=>!!document.querySelector('nav[aria-label="Main navigation"]')),'Native UI did not reload');
    await page.evaluate(()=>Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='Companions')?.click());
    await until(()=>page.evaluate(()=>!!document.querySelector('[aria-label="Edit design MCP toggle acceptance"]')),'MCP widget did not reach the running UI');
    await page.evaluate(()=>document.querySelector('[aria-label="Edit design MCP toggle acceptance"]').click());
    await until(()=>page.evaluate(()=>document.querySelector('[aria-label="Widget name"]')?.value==='MCP toggle acceptance'),'Widget did not open in the designer');
    await page.evaluate(()=>{const input=document.querySelector('[aria-label="Widget name"]');input.value='MCP toggle edited in cockpit';input.dispatchEvent(new Event('change',{bubbles:true}));});
    await page.evaluate(()=>Array.from(document.querySelectorAll('.generic-designer .buildbar button')).find(b=>b.textContent.trim()==='Compile').click());
    await until(()=>page.evaluate(()=>!document.querySelector('.generic-designer .save-action button')?.disabled),'Compilation did not enable save');
    await page.evaluate(()=>document.querySelector('.generic-designer .save-action button').click());
    await until(async()=>{const saved=await ok(reader,'widget_get',{id});return saved.definitions[0].title==='MCP toggle edited in cockpit'&&!('revision'in saved.definitions[0]);},'UI save did not reach MCP');
    console.log('Real MCP SDK: discovery, source guide, replacement, replay, malformed rejection, reader scope, editor and bidirectional plain source save passed.');
  } finally {
    try{await page.evaluate(()=>Array.from(document.querySelectorAll('.generic-designer button')).find(b=>b.textContent.trim()==='Back')?.click());}catch{}
    for(const client of clients)await client.close().catch(()=>{});
    for(const id of credentials)await page.invoke('mcp_credential_revoke',{id,expectedSettingsRevision:(await page.invoke('mcp_settings_get')).settingsRevision});
    if(settings)await page.invoke('mcp_settings_update',{settings:{enabled:settings.enabled,port:settings.port,expectedSettingsRevision:(await page.invoke('mcp_settings_get')).settingsRevision}});
    page.close();
  }
});
