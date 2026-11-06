// Opt-in actual Cockpit metadata stream; no input injection or output activation.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {browserPage,nativePage,until} from '../../scripts/reactive/cdp.mjs';

const browserUrl=process.env.OSG_CONTROLLER_INVENTORY_BROWSER_URL;
const transport=browserUrl?'browser-dev':'native';

test(transport+' controller catalogue shares current metadata and committed renames',{
  skip:!browserUrl&&process.env.OSG_CONTROLLER_INVENTORY_NATIVE!=='1',timeout:60000,
},async()=>{
  const directory=resolve(`.research/reactive/controller-inventory-${transport}/${Date.now()}`);
  await mkdir(directory,{recursive:true});
  const page=browserUrl?await browserPage(browserUrl):await nativePage();let original;
  const request=async value=>{
    const body=JSON.stringify({...value,clientId:'controller-inventory-acceptance'});
    const reply=await page.evaluate(browserUrl
      ?`fetch('/api/gateway/',{method:'POST',headers:{'Content-Type':'application/json','x-osg-client':'1'},body:${JSON.stringify(body)}}).then(response=>response.json())`
      :`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${body}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;
  };
  const read=()=>page.evaluate(`(()=>{const p=window.__controllerInventoryProbe;return {a:p.a,b:p.b,input:p.input};})()`);
  const row=(model,id)=>model?.value?.sources?.find(source=>source.id===id);
  try{
    if(!browserUrl)await until(async()=>{try{return await page.evaluate("document.readyState==='complete'&&document.title==='OpenSimGear Cockpit'&&!!window.__TAURI_INTERNALS__");}catch{return false;}});
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    await page.evaluate(`(async()=>{
      const {observeControllerInventory}=await import('/src/lib/controller-read-models/shared.ts');
      const p=window.__controllerInventoryProbe={stops:[]};
      p.openFirst=()=>observeControllerInventory(state=>p.a=structuredClone(state));
      p.stops.push(p.openFirst());
      p.stops.push(observeControllerInventory(state=>p.b=structuredClone(state)));
    })()`);
    const baseline=await until(read,p=>p.a?.status==='ready'&&p.b?.status==='ready');
    assert.deepEqual(baseline.a.value,baseline.b.value);
    assert.equal(baseline.a.value.total,baseline.a.value.sources.length);
    assert.ok(baseline.a.value.selfAccess);
    original=baseline.a.value.sources.find(source=>source.id===process.env.OSG_CONTROLLER_SOURCE_ID)
      ??baseline.a.value.sources.find(source=>source.online);
    assert.ok(original,'A real observed controller is needed for the reversible metadata check.');
    await writeFile(resolve(directory,'baseline.json'),JSON.stringify(original,null,2)+'\n');
    assert.equal(original.sequence,'0','Metadata does not acquire report cadence.');
    await page.evaluate(`(async()=>{
      const {observeControllerInputs}=await import('/src/lib/controller-observations.ts');
      const p=window.__controllerInventoryProbe;
      p.stops.push(observeControllerInputs(${JSON.stringify(original.id)},state=>p.input=structuredClone(state)));
    })()`);
    await until(read,p=>p.input?.status==='ready'&&p.input.detail?.source?.id===original.id);
    const name=`Stream check ${Date.now()}`;
    await request({command:'controller_update',sourceId:original.id,name,role:original.role});
    const renamed=await until(read,p=>p.a?.status==='ready'&&p.b?.status==='ready'
      &&row(p.a,original.id)?.name===name&&row(p.b,original.id)?.name===name&&p.input?.detail?.source?.name===name);
    assert.equal(row(renamed.a,original.id).generation,original.generation);
    await page.evaluate('window.__controllerInventoryProbe.stops[0]()');
    const restoredState=await request({command:'controller_update',sourceId:original.id,name:original.name,role:original.role});
    const restoredName=restoredState.controllers.source.name;
    assert.equal(restoredName,original.name.trim(),'The existing metadata command canonicalizes names.');
    const updated=await until(read,p=>row(p.b,original.id)?.name===restoredName);
    assert.equal(row(updated.a,original.id)?.name,name,'The closed reader must not receive the restore');
    await page.evaluate(`(() => {
      const probe=window.__controllerInventoryProbe;
      probe.stops[0]=probe.openFirst();
    })()`);
    await until(read,p=>row(p.a,original.id)?.name===restoredName);
    const restored=original;original=undefined;
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify({at:new Date().toISOString(),
      transport,actualNativeGateway:true,sourceId:restored.id,twoConsumers:true,reopenedAfterRestore:true,selectedInputContinues:true,
      renamedAndRestored:true,originalName:restored.name,restoredName,noOutputsActivated:true,physicalLatencyQualified:false},null,2)+'\n');
  }catch(error){
    await writeFile(resolve(directory,'failure.json'),JSON.stringify({error:String(error),probe:await read()},null,2)+'\n');
    throw error;
  }finally{
    try{
      if(original)await request({command:'controller_update',sourceId:original.id,name:original.name,role:original.role});
      await page.evaluate('window.__controllerInventoryProbe?.stops.forEach(stop=>stop());delete window.__controllerInventoryProbe;');
    }finally{await page.close();}
  }
});
