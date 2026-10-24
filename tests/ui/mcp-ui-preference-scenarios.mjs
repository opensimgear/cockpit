import assert from 'node:assert/strict';
export async function uiPreferenceScenarios({page,reader,writer,ok,call,mutation,evidence}){
 const original=await ok(reader,'ui_preferences_get',{key:'theme'});const wanted=!original.value;
 const listener=await page.evaluate(async()=>{window.__sqliteUiEvents=[];const api=window.__TAURI_INTERNALS__,handler=api.transformCallback(event=>window.__sqliteUiEvents.push(event.payload));const eventId=await api.invoke('plugin:event|listen',{event:'ui-preferences-changed',target:{kind:'Any'},handler});return{handler,eventId};});
 try{
  assert.ok(!(await reader.listTools()).tools.some(tool=>tool.name==='ui_preferences_save'));
  const saved=await ok(writer,'ui_preferences_save',mutation({key:'theme',expectedRevision:original.revision,value:wanted}));assert.equal(saved.revision,original.revision+1);assert.equal(saved.value,wanted);
  assert.equal((await ok(reader,'ui_preferences_get',{key:'theme'})).value,wanted);
  const deadline=Date.now()+10000;let reflected=false;
  while(Date.now()<deadline){if(await page.evaluate(wanted=>document.querySelector('.cockpit')?.classList.contains('light')===wanted,wanted)){reflected=true;break;}await new Promise(resolve=>setTimeout(resolve,50));}
  assert.equal(reflected,true,'MCP UI changes must reach the mounted portal through native change events');
  const native=await page.invoke('persistence_ui_read');assert.equal(native.entries.theme.value,wanted);assert.equal(native.entries.theme.revision,saved.revision);
  assert.equal(await page.evaluate(revision=>window.__sqliteUiEvents.some(event=>event.key==='theme'&&event.revision===revision),saved.revision),true,'Native commit notifications must carry the exact changed revision');
  const stale=await call(writer,'ui_preferences_save',mutation({key:'theme',expectedRevision:original.revision,value:original.value}));assert.equal(stale.error.code,'revision_conflict');assert.equal((await ok(reader,'ui_preferences_get',{key:'theme'})).value,wanted);
  evidence.uiPreferences={sameNativeRows:true,exactRevisions:true,readerScope:true,crossViewChangeEvents:true,staleWriteRejected:true};
 }finally{
  await page.evaluate(async listener=>{window.__TAURI_EVENT_PLUGIN_INTERNALS__?.unregisterListener('ui-preferences-changed',listener.eventId);await window.__TAURI_INTERNALS__.invoke('plugin:event|unlisten',{event:'ui-preferences-changed',eventId:listener.eventId});window.__TAURI_INTERNALS__.unregisterCallback?.(listener.handler);},listener);
  const current=await ok(reader,'ui_preferences_get',{key:'theme'});if(current.value!==original.value)await ok(writer,'ui_preferences_save',mutation({key:'theme',expectedRevision:current.revision,value:original.value}));
 }
}
