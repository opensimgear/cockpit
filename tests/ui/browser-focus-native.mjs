import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {compilePanelDraft,newCompiledDraft} from '../../src/lib/compiled-companion-library.ts';
import {createToggleSource} from '../../packages/widget-compiler/src/seeds.ts';
import {sendPublication} from '../../src/lib/companions.ts';
import {verifiedServerKey} from './native/browser-trust.mjs';
const {chromium}=createRequire(import.meta.url)(process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');

test('native browser keeps authenticated presence and neutral controls across focus loss', {timeout:90000},async()=>{
  const native=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  const page=native.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith('http://127.0.0.1:1420/'));
  assert.ok(page,'Start the dev Cockpit');
  const send=async(command,companion={})=>page.evaluate(async request=>{
    const reply=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'browser-focus-native'}});
    if(!reply.ok)throw Error(reply.error);return reply.state.companions;
  },{command,companion});
  let browser,id,instanceId;
  try{
    const state=await send('companion_status');assert.ok(state.listener.enabled);
    const source=createToggleSource(),draft=newCompiledDraft('Browser focus verification',800,480);
    draft.widgets.push({id:'control',label:'Toggle',pageId:draft.pages[0].id,sourceId:source.id,parameters:{},bounds:{x:.1,y:.1,width:.3,height:.3}});
    const publication=await compilePanelDraft(draft,[source],{motion:false,audio:false});
    await sendPublication(publication,send,{saveAsLatest:false});
    const certificate=(await send('companion_browser_certificate')).certificatePem;
    const spki=await verifiedServerKey(state.browserUrl,certificate);
    browser=await chromium.launch({headless:true,channel:'msedge',args:['--ignore-certificate-errors-spki-list='+spki]});
    const context=await browser.newContext(),companion=await context.newPage(),wire=[];
    companion.on('websocket',socket=>socket.on('framesent',event=>{const frame=JSON.parse(event.payload);wire.push({type:frame.type,sessionId:frame.sessionId});}));
    const known=new Set(state.host.clients.map(c=>c.companionId));
    const offer=await send('companion_pair_offer',{kind:'browser'});await companion.goto(offer.pairing.browserUrl);
    await page.waitForFunction(async previous=>{
      const r=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'companion_status',clientId:'browser-focus-native'}});
      return r.state.companions.host.clients.some(c=>!previous.includes(c.companionId));
    },[...known]);
    id=(await send('companion_status')).host.clients.find(c=>!known.has(c.companionId)).companionId;
    instanceId='focus-'+Date.now();await send('companion_assign',{companionId:id,instanceId,publicationHash:publication.publicationHash});
    const waitValues=async predicate=>page.waitForFunction(async({instanceId,predicate})=>{
      const r=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'companion_status',clientId:'browser-focus-native'}});
      const instance=r.state.companions.host.instances.find(i=>i.instanceId===instanceId);
      return instance?.connected&&instance.armed&&Object.values(instance.values??{}).includes(predicate);
    },{instanceId,predicate},{polling:100});
    await companion.getByText('Connected · panel active',{exact:true}).waitFor();
    await companion.locator('.compiled-hit').click();await waitValues(true);
    const original=(await send('companion_status')).host.instances.find(i=>i.instanceId===instanceId),auths=wire.filter(f=>f.type==='auth').length;
    await companion.evaluate(()=>window.dispatchEvent(new Event('blur')));await waitValues(false);
    await companion.getByText('Connected · feedback active · controls paused',{exact:true}).waitFor();
    await companion.waitForTimeout(2000);
    const background=(await send('companion_status')).host.instances.find(i=>i.instanceId===instanceId);
    assert.equal(background.sessionId,original.sessionId);assert.equal(background.connected,true);assert.ok(wire.some(f=>f.type==='heartbeat'));
    await companion.evaluate(()=>window.dispatchEvent(new Event('focus')));
    await companion.getByText('Connected · panel active',{exact:true}).waitFor();await waitValues(false);
    const resumed=(await send('companion_status')).host.instances.find(i=>i.instanceId===instanceId);
    assert.notEqual(resumed.sessionId,original.sessionId);assert.equal(wire.filter(f=>f.type==='auth').length,auths);
    assert.equal(wire.filter(f=>f.type==='baseline').length,2);
    await companion.locator('.compiled-hit').click();await waitValues(true);
  }finally{
    await browser?.close();if(instanceId)await send('companion_unassign',{instanceId}).catch(()=>{});
    if(id)await send('companion_revoke',{companionId:id}).catch(()=>{});await native.close();
  }
});
