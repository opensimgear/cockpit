import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const source=path=>readFile(new URL(path,import.meta.url),'utf8');
test('App starts and disposes a passive workspace without catalogue polling or selection hydration',async()=>{
 const app=await source('../../App.svelte');
 assert.match(app,/workspace\.start\(\)/);assert.match(app,/workspace\?\.dispose\(\)/);
 assert.doesNotMatch(app,/workspace\??\.refresh\(/);assert.doesNotMatch(app,/setInterval\(/);
 const selection=app.slice(app.indexOf('async function selectDevice('),app.indexOf('async function toggleSession('));
 assert.doesNotMatch(selection,/config_read|gatewayRequest|sendGateway/);
 assert.match(app,/selectedEntry\?\.stale/);assert.match(app,/Status unavailable · cached gateway/);
});
test('workspace owns only a documented lease timer and never performs automatic gateway lifecycle work',async()=>{
 const workspace=await source('../gateway-workspace.ts');
 assert.doesNotMatch(workspace,/command:\s*['"](?:ports|status|start|config_read|action_describe)['"]/);
 assert.doesNotMatch(workspace,/async refresh\(|private async discover\(|private async poll\(/);
 assert.match(workspace,/command:\s*['"]workspace_client_renew['"]/);
 assert.match(workspace,/timer cannot acquire claims, connect, start, or activate output/);
});
