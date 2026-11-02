import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('real native committed preferences fan out without refresh queries and survive disposal/reload',{timeout:120000},async()=>{
  await until(async()=>{try{return(await fetch('http://127.0.0.1:1420/')).ok;}catch{return false;}});
  const page=await nativePage();
  const calls=[];let gate=false;
  const unlisten=page.listen(event=>{
    if(gate&&event.method==='Network.requestWillBeSent'){
      const request=event.params.request;
      if(/ipc\.localhost|ipc:\/\//.test(request.url)){
        let command;try{command=JSON.parse(request.postData).request?.command;}catch{}
        calls.push({url:request.url,command});
      }
    }
  });
  const state=()=>page.evaluate(`({first:window.__reactiveAcceptance?.firstEvents.at(-1),second:window.__reactiveAcceptance?.secondEvents.at(-1)})`);
  let original;
  try{
    await until(()=>page.evaluate(`document.title==='OpenSimGear Cockpit'&&!!window.__TAURI_INTERNALS__`));
    await page.call('Page.reload');
    await until(()=>page.evaluate(`document.title==='OpenSimGear Cockpit'&&!!window.__TAURI_INTERNALS__`));
    original=await page.evaluate(`(async()=>{const response=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'ui_preferences_get',key:'theme',clientId:'reactive-native-acceptance'}});if(!response.ok)throw Error(response.error);return response.state.uiPreferences;})()`);
    await page.evaluate(`(async()=>{
      const {SharedObservations}=await import('/src/lib/streams/index.ts');
      const {uiPreferences}=await import('/src/lib/persistence/ui-preferences.ts');
      const first=new SharedObservations(),second=new SharedObservations();
      const owned=window.__reactiveAcceptance={first,second,firstEvents:[],secondEvents:[],uiPreferences};
      owned.stopFirst=first.subscribe('preferences.theme',value=>owned.firstEvents.push(value));
      owned.stopSecond=second.subscribe('preferences.theme',value=>owned.secondEvents.push(value));
    })()`);
    await until(state,value=>value.first?.status==='ready'&&value.second?.status==='ready');
    await page.call('Network.enable',{maxPostDataSize:65536});
    gate=true;
    const committed=await page.evaluate(`(async()=>{const response=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'ui_preferences_save',key:'theme',expectedRevision:${original.revision},value:${!original.value},clientId:'reactive-native-acceptance'}});if(!response.ok)throw Error(response.error);return response.state.uiPreferences;})()`);
    const changed=await until(state,value=>value.first?.frame?.payload?.revision===String(committed.revision)&&value.second?.frame?.payload?.revision===String(committed.revision));
    gate=false;
    assert.equal(changed.first.frame.payload.value,!original.value);
    assert.equal(changed.second.frame.payload.value,!original.value);
    assert.equal(changed.first.frame.session,changed.second.frame.session);
    const visible=await until(()=>page.evaluate(`window.__reactiveAcceptance.uiPreferences.get('theme')`),value=>value.ready&&value.revision===committed.revision);
    assert.equal(visible.ready,true);assert.equal(visible.value,!original.value);
    assert.equal(calls.some(call=>/persistence_ui_read/.test(call.url)||call.command==='ui_preferences_get'),false,JSON.stringify(calls));
    const frozen=await page.evaluate(`(()=>{const owned=window.__reactiveAcceptance;owned.stopFirst();owned.first.close();return owned.firstEvents.length;})()`);
    const restored=await page.evaluate(`(async()=>{const response=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'ui_preferences_save',key:'theme',expectedRevision:${committed.revision},value:${original.value},clientId:'reactive-native-acceptance'}});if(!response.ok)throw Error(response.error);return response.state.uiPreferences;})()`);
    await until(state,value=>value.second?.frame?.payload?.revision===String(restored.revision));
    assert.equal(await page.evaluate(`window.__reactiveAcceptance.firstEvents.length`),frozen);
    await page.evaluate(`window.__reactiveAcceptance.second.close()`);
    // Six bootstrap subscriptions on each reload would exceed the 16/client cap
    // if the owned window failed to dispose its prior document's subscriptions.
    for(let index=0;index<3;index++){
      await page.call('Page.reload');
      await until(()=>page.evaluate(`(async()=>{try{const {uiPreferences}=await import('/src/lib/persistence/ui-preferences.ts');return uiPreferences.get('theme').ready&&uiPreferences.get('theme').revision===${restored.revision};}catch{return false;}})()`));
    }
    await mkdir('.research/reactive',{recursive:true});
    const executableSha256=createHash('sha256').update(await readFile('src-tauri/target/debug/osg-cockpit.exe')).digest('hex');
    await writeFile('.research/reactive/native-transport.json',JSON.stringify({at:new Date().toISOString(),sourceTree:process.env.OSG_REACTIVE_SOURCE_TREE??'working-tree',executableSha256,original,committed,restored,changed,calls,reloads:3},null,2)+'\n');
    const screenshot=await page.call('Page.captureScreenshot',{format:'png'});
    await writeFile('.research/reactive/native-transport.png',Buffer.from(screenshot.data,'base64'));
  }finally{
    gate=false;unlisten();
    if(original)await page.evaluate(`(async()=>{const invoke=window.__TAURI_INTERNALS__.invoke;const current=await invoke('gateway_request',{request:{command:'ui_preferences_get',key:'theme',clientId:'reactive-native-cleanup'}});const entry=current.state?.uiPreferences;if(entry&&entry.value!==${original.value})await invoke('gateway_request',{request:{command:'ui_preferences_save',key:'theme',expectedRevision:entry.revision,value:${original.value},clientId:'reactive-native-cleanup'}});})()`).catch(()=>{});
    await page.evaluate(`window.__reactiveAcceptance?.first.close();window.__reactiveAcceptance?.second.close();delete window.__reactiveAcceptance;`).catch(()=>{});
    page.close();
  }
});
