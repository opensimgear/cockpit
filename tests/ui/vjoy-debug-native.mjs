// Read-only observations of an existing installed slot; never provision or write joystick outputs.
import test from 'node:test';
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';
import {hidMcpSession} from './hid-mcp-session.mjs';

const prefix='controller_vjoy_debug_';
async function call(client,operation,args={}){
  const reply=await client.callTool({name:prefix+operation,arguments:args});
  assert.ok(Buffer.byteLength(JSON.stringify(reply))<1024*1024);
  return reply.structuredContent??JSON.parse(reply.content.find(row=>row.type==='text').text);
}
async function ok(client,operation,args={}){
  const result=await call(client,operation,args);assert.equal(result.ok,true,JSON.stringify(result));return result.data;
}
async function click(page,label,scoped=true){
  await until(()=>page.evaluate(`(() => {
    const parent=${scoped?`document.querySelector('[aria-label="vJoy force feedback inspection"]')`:'document'};
    const button=[...parent?.querySelectorAll('button')??[]].find(node=>(node.textContent.trim()===${JSON.stringify(label)}||node.getAttribute('aria-label')===${JSON.stringify(label)})&&!node.disabled&&node.getClientRects().length);
    if(button)button.click();return !!button;
  })()`));
}
const view=page=>page.evaluate(`(() => {
  const root=document.querySelector('[aria-label="vJoy force feedback inspection"]');
  const json=label=>{const text=root?.querySelector('[aria-label="'+label+'"]')?.textContent;return text?JSON.parse(text):null;};
  return {capabilities:json('vJoy capabilities'),page:json('vJoy native page'),recording:json('vJoy recording status'),
    error:root?.querySelector('[aria-label="vJoy error"]')?.textContent??null,busy:root?.textContent.includes('Reading native vJoy service')??false,text:root?.textContent??''};
})()`);
async function importRecording(page,recording){
  await until(()=>page.evaluate(`(() => {
    const input=document.querySelector('[aria-label="Import vJoy recording"]');if(!input||input.disabled)return false;
    const transfer=new DataTransfer();transfer.items.add(new File([${JSON.stringify(JSON.stringify(recording))}],'authored-vjoy.json',{type:'application/json'}));
    input.files=transfer.files;input.dispatchEvent(new Event('change',{bubbles:true}));return true;
  })()`));
  return until(async()=>{const value=await view(page);if(value.error)throw Error(value.error);return !value.busy&&value.page?value.page:null;});
}

test('running vJoy Debug and credentialed MCP share offline decoding and installed slot outcomes',{
  skip:process.env.OSG_VJOY_DEBUG_NATIVE!=='1',timeout:180000,
},async t=>{
  const page=await nativePage();let mcp,opened=false;
  const slot=Number(process.env.OSG_VJOY_DEBUG_SLOT??1);
  assert.ok(Number.isInteger(slot)&&slot>=1&&slot<=16,'Select an existing slot in the range 1–16.');
  try{
    await until(()=>page.evaluate("typeof window.__TAURI_INTERNALS__?.invoke === 'function' && !!document.querySelector('nav')"));
    mcp=await hidMcpSession(page);
    const tools=(await mcp.reader.listTools()).tools.map(row=>row.name);
    for(const name of ['inspect','read','export','replay'])assert.ok(tools.includes(prefix+name),name);
    for(const name of ['open','close','record_start','record_stop'])assert.ok(!tools.includes(prefix+name),name);
    await assert.rejects(()=>mcp.reader.callTool({name:prefix+'open',arguments:{slot,operationId:randomUUID()}}));
    await click(page,'Debug',false);await click(page,'HID interfaces',false);
    // Unknown commands must retain exact bytes and sequence strings through the same native decoder.
    // Wide sequences specifically exercise the JavaScript precision boundary.
    const fixture={format:'osg-vjoy-ffb',slot,records:[
      {sequence:'9007199254740993',command:0xffffffff,wireHex:'fe010203'},
      {sequence:'9007199254740994',command:0xfffffffe,wireHex:'fd'},
    ]};
    const expected=await ok(mcp.reader,'replay',{recording:fixture,limit:128});
    const rendered=await importRecording(page,fixture);
    assert.deepEqual(rendered,expected,'Offline UI must show the MCP/native decoder result exactly.');
    assert.equal(rendered.records[0].sequence,fixture.records[0].sequence);
    assert.equal(rendered.records[0].wireHex,fixture.records[0].wireHex);
    const large={...fixture,capabilities:{descriptorHex:'ab'.repeat(65536)}};
    const largePage=await ok(mcp.reader,'replay',{recording:large});
    assert.equal(largePage.capabilities.descriptorHex,large.capabilities.descriptorHex);
    assert.deepEqual(await importRecording(page,large),largePage);
    const first=await ok(mcp.reader,'replay',{recording:fixture,limit:1});
    assert.equal(first.records.length,1);assert.equal(first.hasMore,true);
    const second=await ok(mcp.reader,'replay',{recording:fixture,after:first.next,limit:1});
    assert.equal(second.records[0].sequence,fixture.records[1].sequence);
    for(const after of ['01','-1','18446744073709551616']){
      const rejected=await call(mcp.reader,'replay',{recording:fixture,after});assert.equal(rejected.ok,false,after);
    }
    const authored=JSON.parse(readFileSync(new URL('../hid-contract/vjoy/effects-lifecycle.json',import.meta.url),'utf8'));
    const lifecycle={format:'osg-vjoy-ffb',slot:authored.slot,records:authored.packets.map((packet,index)=>({
      sequence:String(index+1),command:packet.command??authored.command,wireHex:packet.bytesHex,
    }))};
    const decoded=await ok(mcp.reader,'replay',{recording:lifecycle,limit:128});
    for(const [index,packet] of authored.packets.entries()){
      assert.equal(decoded.records[index].event.status,packet.expect.status,packet.name);
      if(packet.expect.reportName)assert.equal(decoded.records[index].event.reportName,packet.expect.reportName,packet.name);
      if(packet.expect.effectType)assert.equal(decoded.records[index].event.fields.effectType,packet.expect.effectType,packet.name);
      if(packet.expect.control)assert.equal(decoded.records[index].event.fields.control,packet.expect.control,packet.name);
    }
    assert.deepEqual(decoded.effects,authored.expectedSnapshot);
    assert.deepEqual(await importRecording(page,lifecycle),decoded,'UI effects must equal the authored native/MCP lifecycle result.');
    const layouts=JSON.parse(readFileSync(new URL('../hid-contract/vjoy/report-layouts.json',import.meta.url),'utf8'));
    const layoutArchive={format:'osg-vjoy-ffb',slot:layouts.slot,records:layouts.packets.map((packet,index)=>({
      sequence:String(index+1),command:packet.command??layouts.command,wireHex:packet.bytesHex,
    }))};
    const layoutPage=await ok(mcp.reader,'replay',{recording:layoutArchive});
    for(const [index,packet] of layouts.packets.entries()){
      const event=layoutPage.records[index].event;
      assert.equal(event.status,packet.expect.status,packet.name);
      for(const [field,value] of Object.entries(packet.expect.fields??{}))assert.deepEqual(event.fields[field],value,packet.name);
    }
    assert.deepEqual(await importRecording(page,layoutArchive),layoutPage);
    await page.evaluate(`(() => {const input=document.querySelector('[aria-label="vJoy slot"]');input.value=${JSON.stringify(String(slot))};input.dispatchEvent(new Event('input',{bubbles:true}));})()`);
    const inspected=await call(mcp.reader,'inspect',{slot});
    await click(page,'Inspect');
    const inspectedView=await until(async()=>{const value=await view(page);return !value.busy&&(value.capabilities||value.error)?value:null;});
    if(!inspected.ok){assert.ok(inspectedView.error);t.diagnostic(`Installed slot inspection unavailable: ${inspectedView.error}`);return;}
    assert.deepEqual(inspectedView.capabilities,inspected.data);
    if(inspected.data.captureGeneration){
      t.diagnostic('Existing capture observed without taking ownership.');
    }else{
      await click(page,'Open');
      const result=await until(async()=>{const value=await view(page);return !value.busy&&(value.error||value.text.includes('capture generation'))?value:null;});
      if(result.error){
        const denied=await call(mcp.writer,'open',{slot,operationId:randomUUID()});assert.equal(denied.ok,false,JSON.stringify(denied));
        t.diagnostic(`Installed slot capture unavailable: ${result.error}`);return;
      }
      opened=true;
    }
    await page.evaluate(`(() => {const select=document.querySelector('[aria-label="vJoy source"]');select.value='live';select.dispatchEvent(new Event('change',{bubbles:true}));})()`);
    await click(page,'Read vJoy capture');
    const live=await until(async()=>{const value=await view(page);if(value.error)throw Error(value.error);return !value.busy&&value.page?.generation?value.page:null;});
    const observation=await ok(mcp.reader,'read',{slot,generation:live.generation,limit:128});
    assert.equal(observation.generation,live.generation);assert.equal(observation.slot,slot);
    // No producer is installed by acceptance; retain evidence of any naturally arriving packets.
    for(const row of live.records){const same=observation.records.find(value=>value.sequence===row.sequence);if(same)assert.deepEqual(same,row);}
    t.diagnostic(`Installed capture status ${live.status}; ${live.records.length} received commands, SDK capabilities ${JSON.stringify(inspected.data)}.`);
    if(opened){
      await click(page,'Start vJoy recording');
      await until(async()=>(await view(page)).recording?.status==='recording');
      await click(page,'Stop vJoy recording');
      await until(async()=>(await view(page)).recording?.status==='stopped');
      const archive=await ok(mcp.reader,'export',{slot,generation:live.generation});
      assert.equal(archive.recording.format,'osg-vjoy-ffb');
      await ok(mcp.reader,'replay',{recording:archive.recording,limit:128});
    }
  }finally{
    if(opened)await click(page,'Close').catch(()=>{});
    await mcp?.close();page.close();
  }
});
