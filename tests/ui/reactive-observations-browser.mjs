import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdir,writeFile} from 'node:fs/promises';
import {SharedObservations} from '../../src/lib/streams/shared.ts';
import {createBrowserObservationTransport} from '../../src/lib/streams/browser.ts';
import {until} from '../../scripts/reactive/cdp.mjs';

const origin=process.env.OSG_BROWSER_COCKPIT_URL??'http://127.0.0.1:1420';
const fetchLocal=(url,args={})=>fetch(url,{...args,headers:{...args.headers,Origin:origin}});
async function gateway(request){
  const result=await fetchLocal(`${origin}/api/gateway/`,{method:'POST',headers:{'Content-Type':'application/json','x-osg-client':'1'},body:JSON.stringify({...request,clientId:'reactive-browser-acceptance'})});
  const response=await result.json();assert.equal(response.ok,true,response.error);return response.state;
}

test('browser-dev real service publishes committed values across two consumers and explicit reopen',{timeout:120000},async()=>{
  const original=(await gateway({command:'ui_preferences_get',key:'theme'})).uiPreferences;
  const calls=[];
  const transport=createBrowserObservationTransport({endpoint:`${origin}/api/observe/`,fetch:(url,args)=>{
    calls.push({url:String(url),body:JSON.parse(args.body)});return fetchLocal(url,args);
  }});
  const first=new SharedObservations(transport),second=new SharedObservations(transport);
  const a=[],b=[];
  first.subscribe('preferences.theme',value=>a.push(value));
  second.subscribe('preferences.theme',value=>b.push(value));
  let reopened;
  try{
    await until(()=>Promise.resolve(a.at(-1)?.status==='ready'&&b.at(-1)?.status==='ready'));
    assert.equal(calls.filter(call=>call.url===`${origin}/api/observe/`).length,1);
    await mkdir('.research/reactive',{recursive:true});
    await writeFile('.research/reactive/browser-observers-ready.json',JSON.stringify({original,manual:process.env.OSG_REACTIVE_MANUAL==='1'})+'\n');
    console.log('Two live browser-dev consumers are observing preferences.theme.');
    if(process.env.OSG_REACTIVE_MANUAL!=='1')await gateway({command:'ui_preferences_save',key:'theme',expectedRevision:original.revision,value:!original.value});
    const changed=await until(()=>Promise.resolve({a:a.at(-1),b:b.at(-1)}),value=>value.a?.frame?.payload?.value===!original.value&&value.b?.frame?.payload?.value===!original.value,90000);
    assert.equal(changed.a.frame.payload.revision,changed.b.frame.payload.revision);
    assert.equal(changed.a.frame.session,changed.b.frame.session);
    const frozen=a.length;first.close();
    const restored=(await gateway({command:'ui_preferences_save',key:'theme',expectedRevision:Number(changed.b.frame.payload.revision),value:original.value})).uiPreferences;
    await until(()=>Promise.resolve(b.at(-1)?.frame?.payload?.revision===String(restored.revision)));
    assert.equal(a.length,frozen);
    reopened=new SharedObservations(transport);
    const fresh=[];reopened.subscribe('preferences.theme',value=>fresh.push(value));
    await until(()=>Promise.resolve(fresh.at(-1)?.status==='ready'));
    assert.equal(fresh.at(-1).frame.kind,'snapshot');
    assert.equal(fresh.at(-1).frame.payload.value,original.value);
    assert.equal(fresh.at(-1).frame.payload.revision,String(restored.revision));
    await writeFile('.research/reactive/browser-transport.json',JSON.stringify({at:new Date().toISOString(),original,changed,restored,calls,snapshots:[a[1],b[1]],fresh:fresh.at(-1)},null,2)+'\n');
  }finally{
    first.close();second.close();reopened?.close();
    const current=(await gateway({command:'ui_preferences_get',key:'theme'})).uiPreferences;
    if(current.value!==original.value)await gateway({command:'ui_preferences_save',key:'theme',expectedRevision:current.revision,value:original.value});
  }
});
