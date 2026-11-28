import assert from 'node:assert/strict';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import {until} from '../../scripts/reactive/cdp.mjs';

// CDP is used only to provision credentials. Every HID operation uses authenticated HTTP MCP.
export async function hidMcpSession(page) {
  const invoke=(command,args={})=>page.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)})`);
  const initial=await invoke('mcp_settings_get');
  const clients=[],credentials=[];
  const admin=async(command,args={})=>{
    const status=await invoke('mcp_settings_get');
    return invoke(command,command==='mcp_settings_update'
      ?{settings:{...args,expectedSettingsRevision:status.settingsRevision}}
      :{...args,expectedSettingsRevision:status.settingsRevision});
  };
  async function close() {
    await Promise.allSettled(clients.map(client=>client.close()));
    for(const id of credentials.reverse())await admin('mcp_credential_revoke',{id}).catch(()=>{});
    await admin('mcp_settings_update',{enabled:initial.enabled,port:initial.port});
  }
  try {
    const secrets=[];
    for(const scope of ['read','write']) {
      const secret=await admin('mcp_credential_create',{name:`Temporary authored HID ${scope}`,scope});
      credentials.push(secret.credential.id);secrets.push(secret);
    }
    const settings=await admin('mcp_settings_update',{enabled:true,port:initial.port});
    await until(async()=>(await invoke('mcp_settings_get')).running);
    for(const secret of secrets) {
      const client=new Client({name:'Authored HID MCP acceptance',version:'1.0.0'});
      clients.push(client);
      await client.connect(new StreamableHTTPClientTransport(new URL(settings.endpoint),{
        requestInit:{headers:{Authorization:`Bearer ${secret.token}`}},
      }));
    }
    const [reader,writer]=clients;
    assert.ok((await reader.listTools()).tools.every(tool=>tool.annotations.readOnlyHint));
    return {reader,writer,close};
  }catch(error){await close();throw error;}
}
