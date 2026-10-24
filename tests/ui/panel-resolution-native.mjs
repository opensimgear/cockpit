import {verifiedServerKey} from './native/browser-trust.mjs';
import assert from 'node:assert/strict';
import test from 'node:test';
import {createRequire} from 'node:module';
import {mkdir} from 'node:fs/promises';
import {compilePanelDraft,newCompiledDraft} from '../../src/lib/compiled-companion-library.ts';
import {createToggleSource} from '../../packages/widget-compiler/src/seeds.ts';
import {sendPublication} from '../../src/lib/companions.ts';
const {chromium}=createRequire(import.meta.url)(process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');

test('running cockpit: resolution warning follows selected page, display scale, resize and fullscreen', {timeout:120000},async()=>{
  const endpoint=(await(await fetch((process.env.OSG_GENERIC_CDP??'http://127.0.0.1:9222')+'/json/version')).json()).webSocketDebuggerUrl;
  const native=await chromium.connectOverCDP(endpoint,{noDefaults:true});
  const page=native.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith(process.env.OSG_GENERIC_ORIGIN??'http://127.0.0.1:1420'));
  assert.ok(page,'Start the dev Cockpit before running this qualification.');
  const send=(command,companion={})=>page.evaluate(async request=>{
    const {gatewayRequest}=await import('/src/lib/gateway.ts');return (await gatewayRequest(request)).companions;
  },{command,companion});
  let browser,companionId,instanceId;
  try{
    await page.evaluate(async()=>{const reply=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'companion_status',clientId:'native-widget-qualification'}});if(!reply.ok)throw Error(reply.error);});
    const listener=(await send('companion_inventory')).listener;if(!listener.enabled)await send('companion_enable',{address:'127.0.0.1',port:54449,advertisedHost:'127.0.0.1',discovery:false,expectedSettingsRevision:listener.settingsRevision});
    const source=createToggleSource(),draft=newCompiledDraft('Resolution warning review',800,480);
    draft.pages.push({id:'large',title:'Large',width:1600,height:960});
    for(const [index,p] of draft.pages.entries())draft.widgets.push({id:'toggle-'+index,label:'Toggle',pageId:p.id,sourceId:source.id,parameters:{},bounds:{x:.1,y:.1,width:240/p.width,height:144/p.height}});
    const publication=await compilePanelDraft(draft,[source],{motion:false,audio:false});
    assert.equal(publication.document.presentation.artifacts[0].artifact.width,240,'Compilation must keep the authored resolution');
    await sendPublication(publication,send,{saveAsLatest:false});
    const previous=new Set((await send('companion_status')).host.clients.map(c=>c.companionId));
    const host=(await send('companion_status')),certificate=(await send('companion_browser_certificate')).certificatePem,spki=await verifiedServerKey(host.browserUrl,certificate);browser=await chromium.launch({headless:true,channel:process.env.OSG_BROWSER_CHANNEL??'msedge',args:['--ignore-certificate-errors-spki-list='+spki]});
    const context=await browser.newContext({viewport:{width:840,height:800},deviceScaleFactor:2});
    const companion=await context.newPage(),errors=[];companion.on('pageerror',e=>errors.push(e.message));
    const offer=await send('companion_pair_offer',{kind:'browser'});await companion.goto(offer.pairing.browserUrl,{waitUntil:'domcontentloaded'});
    for(let i=0;i<100;i++){companionId=(await send('companion_status')).host.clients.find(c=>!previous.has(c.companionId))?.companionId;if(companionId)break;await page.waitForTimeout(100);}
    assert.ok(companionId);instanceId='resolution-'+Date.now();
    await send('companion_assign',{companionId,instanceId,publicationHash:publication.publicationHash});
    await companion.getByText('Connected · panel active',{exact:true}).waitFor({timeout:30000});
    const warning=companion.locator('.resolution-warning');await warning.waitFor();
    assert.match(await warning.innerText(),/800 × 480 px, displayed at 1600 × 960 px/);
    assert.equal(await companion.locator('.compiled-widget canvas').evaluate(c=>c.width),240);
    const widget=companion.locator('.compiled-widget'),initial=await widget.getAttribute('data-frame-id');
    await widget.locator('.compiled-hit').click();await companion.waitForTimeout(100);
    assert.notEqual(await widget.getAttribute('data-frame-id'),initial,'Warning must not disable controls');
    await companion.getByRole('combobox',{name:/^Page/}).selectOption('large');await warning.waitFor({state:'hidden'});
    await companion.getByRole('combobox',{name:/^Page/}).selectOption(draft.pages[0].id);await warning.waitFor();
    await companion.getByRole('button',{name:'Dismiss resolution warning'}).click();await warning.waitFor({state:'hidden'});
    await companion.setViewportSize({width:440,height:800});await warning.waitFor({state:'hidden'});
    await companion.setViewportSize({width:940,height:800});await warning.waitFor();assert.match(await warning.innerText(),/1800 × 1080 px/);
    await companion.getByRole('button',{name:/Fullscreen$/}).click();await companion.locator('.shell.fullscreen').waitFor();
    await companion.waitForFunction(()=>document.querySelector('.resolution-warning')?.textContent.includes('1880 × 1600 px'));
    assert.equal(await warning.isVisible(),true);
    await mkdir('.research/panel-resolution',{recursive:true});await companion.screenshot({path:'.research/panel-resolution/fullscreen.png'});
    assert.deepEqual(errors,[]);
  }finally{
    await browser?.close();if(instanceId)await send('companion_unassign',{instanceId}).catch(()=>{});
    if(companionId)await send('companion_revoke',{companionId}).catch(()=>{});
    await native.close();
  }
});
