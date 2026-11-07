// Exercise passive vJoy owner updates against a disposable app/database on Windows.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {createInterface} from 'node:readline';
import {fileURLToPath} from 'node:url';
import test from 'node:test';
import {browserPage,nativePage,until} from '../../scripts/reactive/cdp.mjs';

const browserUrl=process.env.OSG_VJOY_OWNERSHIP_BROWSER_URL;
const enabled=!!browserUrl||process.env.OSG_VJOY_OWNERSHIP_NATIVE==='1';
const transport=browserUrl?'browser-dev':'native';
const fixture=fileURLToPath(new URL('./external-vjoy-owner.ps1',import.meta.url));
const demandTopic='system.inventory.vjoy';
const inventoryPaths=new Set(['/gateway_request','/api/gateway/']);
const openPath=browserUrl?'/api/observe/subscribe':'/observation_subscribe';
const closePath=browserUrl?'/api/observe/unsubscribe':'/observation_unsubscribe';
const requestKey=value=>JSON.stringify([value.connectionId,value.requestId]);
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));

test(`${transport} Devices status follows external vJoy acquire and release while visible`,{
  skip:process.platform!=='win32'||!enabled,timeout:60_000,
},async()=>{
  const page=browserUrl?await browserPage(browserUrl):await nativePage();
  const writer=browserUrl?await page.openPeer():page;
  const request=async value=>{
    const payload={...value,clientId:'external-vjoy-owner-fixture'};
    if(browserUrl){
      const url=new URL('/src/lib/gateway.ts',browserUrl).href;
      return writer.evaluate(`import(${JSON.stringify(url)}).then(({gatewayRequest})=>gatewayRequest(${JSON.stringify(payload)}))`);
    }
    const reply=await writer.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(payload)}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);
    return reply.state;
  };
  const failures=[];
  const calls=[];
  const pending=new Map(), live=new Set(), retired=new Set();
  let opens=0, closes=0, traceError;
  await page.call('Network.enable',{maxPostDataSize:65536});
  const stopTrace=page.listen(({method,params})=>{
    const id=params?.requestId;
    if(method==='Network.requestWillBeSent'&&params.request.postData){
      const path=new URL(params.request.url).pathname;
      if(!inventoryPaths.has(path)&&path!==openPath&&path!==closePath)return;
      const body=JSON.parse(params.request.postData);
      const request=body.request??body;
      if(inventoryPaths.has(path)&&request.command==='system_inventory')calls.push(request.refresh===true);
      else if(path===openPath&&body.topic===demandTopic)pending.set(id,{open:true,body});
      else if(path===closePath)pending.set(id,{key:browserUrl?requestKey(body):body.subscriptionId});
    }
    const item=pending.get(id);
    if(!item)return;
    if(method==='Network.responseReceived')item.status=params.response.status;
    if(method!=='Network.loadingFinished'&&method!=='Network.loadingFailed')return;
    pending.delete(id);
    if(item.open){
      void page.call('Network.getResponseBody',{requestId:id}).then(({body})=>{
        assert.equal(item.status,200);
        const response=JSON.parse(body);
        const token=browserUrl?response.subscriptionId:response;
        assert.equal(typeof token,'string');
        const key=browserUrl?requestKey(item.body):token;
        opens++;
        if(!retired.has(key))live.add(key);
      }).catch(error=>{traceError=error;});
    }else if(item.status===200){
      retired.add(item.key);
      if(live.delete(item.key))closes++;
    }
  });
  const waitDemand=(expected,opened,closed)=>until(async()=>{
    if(traceError)throw traceError;
    return live.size===expected&&opens===opened&&closes===closed;
  });
  const navigate=label=>until(()=>page.evaluate(`(()=>{
    const button=[...document.querySelectorAll('nav[aria-label="Main navigation"] button')]
      .find(item=>(item.title||item.textContent.trim())===${JSON.stringify(label)});
    if(!button)return false;button.click();return true;
  })()`));
  const deviceLabel=slot=>page.evaluate(`(()=>{
    const card=[...document.querySelectorAll('[data-component-id]')].find(item=>item.dataset.componentId===${JSON.stringify('virtual:'+slot.id)});
    return card?.getAttribute('aria-label');
  })()`);
  const waitDevice=async(slot,status)=>until(()=>deviceLabel(slot),value=>value?.includes(' · '+status+' · '),12_000)
    .catch(async cause=>{throw new Error('Expected '+slot.id+' to show '+status+'. Label was '+await deviceLabel(slot),{cause});});
  const openDetail=slot=>until(()=>page.evaluate(`(()=>{
    const details=document.querySelector('[aria-label="Component details"]');
    if(details)return details.querySelector('h1')?.textContent.trim()===${JSON.stringify(slot.name)};
    const card=[...document.querySelectorAll('[data-component-id]')].find(item=>item.dataset.componentId===${JSON.stringify('virtual:'+slot.id)});
    card?.click();return false;
  })()`));
  const detailOwner=()=>page.evaluate(`(()=>{
    const details=document.querySelector('[aria-label="Component details"]');
    const owner=details&&[...details.querySelectorAll('dt')].find(item=>item.textContent.trim()==='Owner');
    return owner?.parentElement.querySelector('dd')?.textContent.trim();
  })()`);
  const emulateHidden=hidden=>page.evaluate(`(()=>{
    if(${hidden})Object.defineProperty(document,'hidden',{configurable:true,value:true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const feeders=[];
  let feeder;
  try{
    await until(()=>page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    await navigate('Devices');
    await until(()=>page.evaluate(`!!document.querySelector('[aria-label="System devices"]')`));
    await waitDemand(1,1,0);
    const slot=await until(async()=>{
      const {systemInventory}=await request({command:'system_inventory'});
      return systemInventory.virtualDevices.find(value=>value.backend==='vjoy'&&value.status==='free'&&value.slot);
    },Boolean,12_000).catch(error=>{
      throw Error('A free existing vJoy slot is required. The fixture never provisions slots.',{cause:error});
    });
    const startFeeder=async()=>{
      feeder=spawn('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',fixture,'-Slot',String(slot.slot)],{windowsHide:true,stdio:['pipe','pipe','pipe']});
      feeders.push(feeder);
      const lines=createInterface({input:feeder.stdout});
      feeder.stderr.setEncoding('utf8').on('data',value=>failures.push(value));
      return new Promise((resolve,reject)=>{
        const timeout=setTimeout(()=>reject(Error('External vJoy feeder did not acquire the free slot.')),12_000);
        feeder.once('error',error=>{clearTimeout(timeout);reject(error);});
        feeder.once('exit',code=>{if(code!==0){clearTimeout(timeout);reject(Error(`Feeder exited early (${code}): ${failures.join('')}`));}});
        lines.once('line',line=>{clearTimeout(timeout);try{resolve(JSON.parse(line));}catch(error){reject(error);}});
      });
    };
    const ready=await startFeeder();
    assert.equal(ready.state,'ready');assert.equal(ready.slot,slot.slot);assert.ok(Number.isInteger(ready.pid));
    await waitDevice(slot,'busy');
    await openDetail(slot);
    await until(detailOwner,value=>value===`Process ${ready.pid}`,12_000);
    await waitDemand(1,1,0);
    const release=async()=>{
      const stopped=once(feeder,'exit');
      feeder.stdin.end('release\n');
      const [code]=await stopped;
      assert.equal(code,0,failures.join(''));
    };
    await release();
    await until(detailOwner,value=>value==='None observed',12_000);
    await page.evaluate(`document.querySelector('[aria-label="Component details"] .header-back').click()`);
    await waitDevice(slot,'free');

    await startFeeder();
    await waitDevice(slot,'busy');
    await navigate('Overview');
    await waitDemand(0,1,1);
    const hiddenReads=calls.length;
    await release();
    await delay(800);
    assert.equal(calls.length,hiddenReads,'Overview must not read an external release.');
    await navigate('Devices');
    await waitDemand(1,2,1);
    await waitDevice(slot,'free');

    // A document visibility transition releases demand too, then reopens from current status.
    await startFeeder();
    await waitDevice(slot,'busy');
    await emulateHidden(true);
    await waitDemand(0,2,2);
    const documentReads=calls.length;
    await release();
    await delay(800);
    assert.equal(calls.length,documentReads,'The hidden document must not read an external release.');
    await emulateHidden(false);
    await waitDemand(1,3,2);
    await waitDevice(slot,'free');
    await navigate('Overview');
    await waitDemand(0,3,3);
    await until(()=>page.evaluate(`(()=>{
      const settings=[...document.querySelectorAll('.sidebar-bottom button')].find(button=>(button.title||button.textContent.trim())==='Settings');
      if(!settings)return false;
      settings.click();return true;
    })()`));
    await until(()=>page.evaluate(`(()=>{
      const tab=document.querySelector('#settings-tab-outputs');
      if(!tab)return false;
      tab.click();return true;
    })()`));
    await waitDemand(1,4,3);
    const settingsStatus=()=>page.evaluate(`(()=>{
      const row=[...document.querySelectorAll('.virtual-slots li')].find(item=>item.querySelector('strong')?.textContent===${JSON.stringify(slot.name)});
      return row?.querySelector('span')?.textContent;
    })()`);
    const settingsReady=await startFeeder();
    await until(settingsStatus,value=>value==='busy · Process '+settingsReady.pid,12_000);
    await release();
    await until(settingsStatus,value=>value==='free',12_000);
    await navigate('Overview');
    await waitDemand(0,4,4);
    assert.ok(!calls.some(Boolean),'Passive updates must not request a full inventory refresh.');
    console.log(JSON.stringify({transport,slot:slot.slot,acquiredOwnerPid:ready.pid,opens,closes,ownerReleased:true,detailUpdated:true,settingsUpdated:true,hiddenReads:0}));
  }finally{
    await emulateHidden(false).catch(()=>{});
    stopTrace();
    for(const child of feeders){
      if(!child.killed&&child.exitCode===null){child.stdin.end('release\n');await new Promise(resolve=>{const timer=setTimeout(()=>{child.kill();resolve();},3000);child.once('exit',()=>{clearTimeout(timer);resolve();});});}
    }
    await page.close();
  }
});
