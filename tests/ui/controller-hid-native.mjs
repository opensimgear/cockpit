// Opt-in running Windows Cockpit acceptance. Default: metadata only; capture requires an exact interface ID.
import test from 'node:test';
import assert from 'node:assert/strict';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';

async function nativePage(){
  const targets=await(await fetch(`${process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9227'}/json/list`,{signal:AbortSignal.timeout(5000)})).json();
  const origin=process.env.OSG_NATIVE_DEV_ORIGIN??'http://127.0.0.1:1420';
  const target=targets.find(row=>row.type==='page'&&!row.url.includes('profile-activation-window')&&(row.url.startsWith(origin)||row.url.startsWith('http://tauri.localhost/')));
  assert.ok(target,'Start the running native dev Cockpit with WebView debugging.');
  const socket=new WebSocket(target.webSocketDebuggerUrl),pending=new Map();let sequence=0;
  await new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>{socket.close();reject(Error('Native debugger connection timed out'));},8000);
    socket.addEventListener('open',()=>{clearTimeout(timer);resolve();},{once:true});
    socket.addEventListener('error',error=>{clearTimeout(timer);reject(error);},{once:true});
  });
  socket.addEventListener('message',event=>{const reply=JSON.parse(event.data),task=pending.get(reply.id);if(!task)return;pending.delete(reply.id);clearTimeout(task.timer);reply.error?task.reject(Error(reply.error.message)):task.resolve(reply.result);});
  return {
    async evaluate(fn,...args){
      const id=++sequence,result=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>{pending.delete(id);reject(Error('Native HID acceptance timed out'));},45000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method:'Runtime.evaluate',params:{expression:`(${fn.toString()})(...${JSON.stringify(args)})`,awaitPromise:true,returnByValue:true}}));});
      if(result.exceptionDetails)throw Error(result.exceptionDetails.exception?.description??result.exceptionDetails.text);return result.result.value;
    },
    invoke(command,args={}){return this.evaluate((command,args)=>window.__TAURI_INTERNALS__.invoke(command,args),command,args);},
    close(){for(const task of pending.values())clearTimeout(task.timer);socket.close();},
  };
}
async function until(work,label){
  const deadline=Date.now()+20000;let last;
  while(Date.now()<deadline){try{const result=await work();if(result)return result;}catch(error){last=error;}await new Promise(resolve=>setTimeout(resolve,100));}
  throw Error(`${label}${last?`: ${last.message}`:''}`);
}
async function connect(endpoint,token){const client=new Client({name:'HID shared-service acceptance',version:'1.0.0'});await client.connect(new StreamableHTTPClientTransport(new URL(endpoint),{requestInit:{headers:{Authorization:`Bearer ${token}`}}}));return client;}
async function call(client,name,args={}){const reply=await client.callTool({name,arguments:args});return reply.structuredContent??JSON.parse(reply.content.find(part=>part.type==='text').text);}
async function ok(client,name,args={}){const result=await call(client,name,args);assert.equal(result.ok,true,JSON.stringify(result));return result.data;}

test('running Debug and credentialed MCP share selected HID contract and capture generation',{
  skip:process.env.OSG_HID_NATIVE!=='1',timeout:180000,
},async()=>{
  const page=await nativePage(),clients=[],credentials=[];let initial;
  const admin=async(command,args={})=>{const status=await page.invoke('mcp_settings_get');return page.invoke(command,command==='mcp_settings_update'?{settings:{...args,expectedSettingsRevision:status.settingsRevision}}:{...args,expectedSettingsRevision:status.settingsRevision});};
  const click=label=>page.evaluate(label=>{const buttons=[...document.querySelectorAll('button')];const button=buttons.find(button=>button.textContent.trim()===label&&!button.disabled&&button.getClientRects().length);if(!button)return false;button.click();return true;},label);
  const select=id=>page.evaluate(id=>{const select=document.querySelector('select[aria-label="HID interface"]');if(!select||select.disabled||![...select.options].some(option=>option.value===`live:${id}`))return false;select.value=`live:${id}`;select.dispatchEvent(new Event('change',{bubbles:true}));return true;},id);
  const displayed=summary=>page.evaluate(summary=>{const inspector=document.querySelector('[aria-label="HID service inspection"]');const detail=[...inspector?.querySelectorAll('details')??[]].find(node=>node.querySelector('summary')?.textContent===summary);const text=detail?.querySelector('pre')?.textContent;return text?JSON.parse(text):null;},summary);
  const captureView=()=>page.evaluate(()=>{
    const inspector=document.querySelector('[aria-label="HID service inspection"]');
    const read=[...inspector?.querySelectorAll('button')??[]].find(button=>button.textContent.trim()==='Read capture');
    const value=summary=>{const detail=[...inspector?.querySelectorAll('details')??[]].find(node=>node.querySelector('summary')?.textContent===summary);const text=detail?.querySelector('pre')?.textContent;return text?JSON.parse(text):null;};
    return {ready:!!read&&!read.disabled,error:inspector?.querySelector('[role="alert"]')?.textContent??null,current:value('Current decoded state'),reports:value('Captured reports · current page')};
  });
  try{
    initial=await page.invoke('mcp_settings_get');
    const readerSecret=await admin('mcp_credential_create',{name:'Temporary HID reader',scope:'read'});credentials.push(readerSecret.credential.id);
    const writerSecret=await admin('mcp_credential_create',{name:'Temporary HID writer',scope:'write'});credentials.push(writerSecret.credential.id);
    const settings=await admin('mcp_settings_update',{enabled:true,port:initial.port});
    await until(async()=>(await page.invoke('mcp_settings_get')).running,'MCP server did not start');
    const reader=await connect(settings.endpoint,readerSecret.token),writer=await connect(settings.endpoint,writerSecret.token);clients.push(reader,writer);
    const readTools=(await reader.listTools()).tools,writeTools=(await writer.listTools()).tools;
    for(const name of ['controller_hid_list','controller_hid_inspect','controller_hid_catalog','controller_hid_snapshot','controller_hid_state','controller_hid_reports'])assert.ok(readTools.some(tool=>tool.name===name),name);
    for(const name of ['controller_hid_open','controller_hid_get','controller_hid_close']){assert.ok(!readTools.some(tool=>tool.name===name),name);assert.ok(writeTools.some(tool=>tool.name===name),name);}
    const inventory=await ok(reader,'controller_hid_list');assert.ok(Array.isArray(inventory.interfaces));
    const catalog=await ok(reader,'controller_hid_catalog');assert.ok(Array.isArray(catalog.snapshots));
    await until(()=>click('Debug'),'Debug navigation is unavailable');await until(()=>click('HID interfaces'),'HID Debug tab is unavailable');
    await until(()=>click('Refresh'),'HID inventory refresh is unavailable');
    const explicit=process.env.OSG_HID_INTERFACE_ID;
    const selected=explicit?inventory.interfaces.find(row=>row.interfaceId===explicit):inventory.interfaces.find(row=>typeof row.interfaceId==='string'&&row.access==='metadata-only')??inventory.interfaces.find(row=>typeof row.interfaceId==='string');
    if(explicit)assert.ok(selected,'OSG_HID_INTERFACE_ID must exactly match current inventory.');
    if(!selected){assert.equal(inventory.interfaces.filter(row=>row.interfaceId).length,0);return;}
    await until(()=>select(selected.interfaceId),'Selected HID interface is unavailable in Debug');
    const detail=await until(()=>displayed('Native contract'),'Native contract was not rendered');
    const mcpDetail=await ok(reader,'controller_hid_inspect',{interfaceId:selected.interfaceId});
    assert.equal(detail.contractFingerprint,mcpDetail.contractFingerprint);assert.equal(detail.interfaceId,mcpDetail.interfaceId);
    const snapshot=await ok(reader,'controller_hid_snapshot',{interfaceId:selected.interfaceId});
    assert.equal(snapshot.contractFingerprint,detail.contractFingerprint);assert.equal(snapshot.status,'last-observed');assert.equal(snapshot.availability,'offline');
    if(!explicit)return; // No receiver opens during the default metadata-only scenario.
    assert.ok(!selected.captureGeneration,'Use an interface with no pre-existing capture for owned-open acceptance.');
    await until(()=>click('Open'),'Explicit Open is unavailable');
    const opened=await until(async()=>{const rows=(await ok(reader,'controller_hid_list')).interfaces;return rows.find(row=>row.interfaceId===explicit&&row.captureGeneration);},'UI did not open a native generation');
    const identity={interfaceId:explicit,generation:opened.captureGeneration};
    const state=await ok(reader,'controller_hid_state',identity);assert.equal(state.contractFingerprint,detail.contractFingerprint);
    let lastView,observedReports,uiState;
    try{
      await until(async()=>{
        // A Read clears both rendered panes. Finish and consume this read before dispatching another.
        await until(()=>click('Read capture'),'Capture read is unavailable');
        lastView=await until(async()=>{
          const view=await captureView();
          if(view.error)throw Error(view.error);
          return view.ready&&view.current&&view.reports?view:null;
        },'UI capture read did not complete');
        if(lastView.reports.result.reports?.length)observedReports=lastView.reports;
        uiState=lastView.current;
        return observedReports&&BigInt(uiState.result.latest)>0n;
      },'Waiting for an actually observed HID report; zero-valued samples alone do not prove a packet was received');
    }catch(reason){
      const summarize=value=>({interfaceId:value?.interfaceId,generation:value?.generation,status:value?.result?.status,
        latest:value?.result?.latest,receivedUs:value?.result?.receivedUs,values:value?.result?.values?.length,
        reportCount:value?.result?.reports?.length,first:value?.result?.reports?.[0]?.sequence,last:value?.result?.reports?.at(-1)?.sequence,
        oldest:value?.result?.oldest,gap:value?.result?.gap,terminal:value?.result?.terminal,diagnostics:value?.result?.diagnostics});
      const currentView=await captureView().catch(error=>({error:error.message}));
      const nativeState=await call(reader,'controller_hid_state',identity).catch(error=>({error:error.message}));
      const nativeReports=await call(reader,'controller_hid_reports',{...identity,after:'0'}).catch(error=>({error:error.message}));
      throw Error(`${reason.message}\nHID diagnostics: ${JSON.stringify({ui:{ready:currentView.ready,error:currentView.error,current:summarize(currentView.current),reports:summarize(currentView.reports)},lastCompleted:{current:summarize(lastView?.current),reports:summarize(lastView?.reports)},mcpState:nativeState.ok?summarize(nativeState.data):nativeState,mcpReports:nativeReports.ok?summarize(nativeReports.data):nativeReports})}`,{cause:reason});
    }
    const uiReports=observedReports;
    assert.equal(uiState.generation,state.generation);assert.equal(uiState.interfaceId,state.interfaceId);assert.equal(uiState.contractFingerprint,state.contractFingerprint);
    assert.equal(uiReports.generation,identity.generation);assert.equal(uiReports.result.gap,false,'UI capture history has a gap');
    assert.ok(BigInt(uiState.result.latest)>0n,'Waiting state must not be treated as an observed zero');
    const uiRows=uiReports.result.reports;
    assert.ok(uiRows.length>0,'Explicit hardware acceptance requires at least one captured report');
    // latest is the journal high-water mark, not the final sequence in this bounded page.
    assert.ok(BigInt(uiRows.at(-1).sequence)<=BigInt(uiReports.result.latest));
    assert.notEqual(uiReports.result.truncated,true,'UI declared truncated report evidence');
    const reports=await ok(reader,'controller_hid_reports',{...identity,after:(BigInt(uiRows[0].sequence)-1n).toString()});
    assert.equal(reports.generation,identity.generation);assert.equal(reports.result.gap,false,'MCP capture history has a gap');
    assert.ok(Array.isArray(reports.result.reports)&&reports.result.reports.length>0,'MCP must return observed packets');
    assert.notEqual(reports.result.truncated,true,'MCP declared truncated report evidence');
    const bySequence=new Map(reports.result.reports.map(row=>[row.sequence,row]));
    for(const row of uiRows){
      assert.ok(bySequence.has(row.sequence),`No MCP overlap for UI report ${row.sequence}`);
      assert.deepEqual(bySequence.get(row.sequence),row,`UI/MCP payload differs for exact sequence ${row.sequence}`);
      assert.ok(typeof row.wireHex==='string'&&row.wireHex.length>0,'Observed raw packets must retain their bytes, including all-zero packets');
    }
    await until(()=>click('Close'),'Owned capture close is unavailable');
    await until(async()=>(await call(reader,'controller_hid_state',identity)).ok===false,'Closed generation remained readable');
  }finally{
    // Leaving the tab retires only generations opened by this view.
    await click('Incoming telemetry').catch(()=>{});
    await Promise.allSettled(clients.map(client=>client.close()));
    for(const id of credentials.reverse())await admin('mcp_credential_revoke',{id}).catch(()=>{});
    if(initial)await admin('mcp_settings_update',{enabled:initial.enabled,port:initial.port});
    page.close();
  }
});
