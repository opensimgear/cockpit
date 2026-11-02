import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('real native controller observers share capture and reopen without inspector queries',{timeout:90000},async()=>{
  await until(async()=>{try{
    const pages=await(await fetch(`${process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222'}/json`)).json();
    return pages.some(page=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url));
  }catch{return false;}});
  const page=await nativePage(),calls=[];
  let recording=false;
  const unlisten=page.listen(event=>{
    if(recording&&event.method==='Network.requestWillBeSent'){
      const request=event.params.request;
      if(/ipc\.localhost|ipc:\/\//.test(request.url)){
        let command,topic;try{const payload=JSON.parse(request.postData);command=payload.request?.command;topic=payload.topic;}catch{}
        calls.push({url:request.url,command,topic});
      }
    }
  });
  try{
    await until(()=>page.evaluate(`document.title==='OpenSimGear Cockpit'&&!!window.__TAURI_INTERNALS__`));
    await page.call('Network.enable');
    const selected=await page.evaluate(`(async()=>{
      const {gatewayRequest}=await import('/src/lib/gateway.ts');
      const {observeControllerInputs}=await import('/src/lib/controller-observations.ts');
      const result=await gatewayRequest({command:'controller_list',offset:0});
      const source=result.controllers?.sources?.find(source=>source.online);
      if(!source)throw Error('This running acceptance requires an observed physical controller.');
      window.__controllerAcceptance={source,observeControllerInputs,first:[],second:[]};
      return source;
    })()`);
    assert.equal(selected.origin,'host-controller');
    recording=true;
    await page.evaluate(`(()=>{
      const state=window.__controllerAcceptance;
      state.stopFirst=state.observeControllerInputs(state.source.id,value=>state.first.push(value));
      state.stopSecond=state.observeControllerInputs(state.source.id,value=>state.second.push(value));
    })()`);
    const initial=await until(()=>page.evaluate(`(()=>{const s=window.__controllerAcceptance;return {first:s.first.at(-1),second:s.second.at(-1)}})()`),
      value=>value.first?.status==='ready'&&value.second?.status==='ready');
    assert.equal(initial.first.detail.source.id,selected.id);
    assert.deepEqual(initial.first,initial.second);
    const controls=initial.first.detail.controls;
    assert.equal(controls.length,initial.first.detail.total);
    assert.ok(controls.length>0,'The physical descriptor must have controls.');
    assert.ok(controls.every(control=>Number.isInteger(control.capability.min)&&Number.isInteger(control.capability.max)));
    const before=await page.evaluate(`(()=>{const s=window.__controllerAcceptance;s.stopFirst();return s.first.length;})()`);
    await page.evaluate(`(()=>{const s=window.__controllerAcceptance;s.third=[];s.stopThird=s.observeControllerInputs(s.source.id,value=>s.third.push(value));})()`);
    await until(()=>page.evaluate(`window.__controllerAcceptance.third.at(-1)?.status==='ready'`));
    assert.equal(await page.evaluate(`window.__controllerAcceptance.first.length`),before);
    const subscribes=()=>calls.filter(call=>call.url.endsWith('/observation_subscribe')&&call.topic===`controllers.input.${selected.id}`);
    assert.equal(subscribes().length,1,'Consumers must share one native subscription.');
    await page.evaluate(`(()=>{const s=window.__controllerAcceptance;s.stopSecond();s.stopThird();s.reopened=[];s.stopReopened=s.observeControllerInputs(s.source.id,value=>s.reopened.push(value));})()`);
    const reopened=await until(()=>page.evaluate(`window.__controllerAcceptance.reopened.at(-1)`),value=>value?.status==='ready');
    assert.equal(reopened.detail.source.id,selected.id);
    assert.equal(subscribes().length,2);
    assert.equal(calls.filter(call=>call.command==='controller_inspect').length,0);
    await mkdir('.research/reactive',{recursive:true});
    const executable=await readFile('src-tauri/target/debug/osg-cockpit.exe');
    await writeFile('.research/reactive/controller-native.json',JSON.stringify({at:new Date().toISOString(),
      sourceTree:process.env.OSG_REACTIVE_SOURCE_TREE,executableSha256:createHash('sha256').update(executable).digest('hex'),
      selected,initial,reopened,calls,qualification:'Passive real capture only; no sweep, unplug or output qualification.'},null,2));
    const screenshot=await page.call('Page.captureScreenshot',{format:'png'});
    await writeFile('.research/reactive/controller-native.png',Buffer.from(screenshot.data,'base64'));
  }finally{
    recording=false;
    await page.evaluate(`(()=>{const s=window.__controllerAcceptance;if(s){s.stopFirst?.();s.stopSecond?.();s.stopThird?.();s.stopReopened?.();}delete window.__controllerAcceptance;})()`).catch(()=>{});
    unlisten();page.close();
  }
});
