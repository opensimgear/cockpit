import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {createServer} from 'node:net';
import {setTimeout as delay} from 'node:timers/promises';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl=process.env.OSG_COMPANION_CONNECTIONS_BROWSER_URL??process.env.OSG_NAVIGATION_BROWSER_URL;
const transport=browserUrl?'browser development':'native';
const enabled=process.env.OSG_COMPANION_CONNECTIONS_VISIBILITY==='1'
  && process.env.OSG_NAVIGATION_PRIVATE_LISTENER==='1'
  && process.env.OSG_NAVIGATION_ISOLATED_DATABASE==='1';

async function privateLoopbackPort(){
  const server=createServer();
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});
  const {port}=server.address();
  await new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve()));
  return port;
}

test(`${transport} production companion roster suspends passive status reads while hidden`,{skip:!enabled,timeout:120000},async()=>{
  const page=browserUrl?await browserPage(browserUrl):await nativePage();
  const request=value=>page.evaluate(`(async()=>{
    const request=${JSON.stringify(value)};
    if(window.__TAURI_INTERNALS__?.invoke){
      const result=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'companion-connections-visibility-fixture'}});
      if(!result.ok)throw Error(result.error);return result.state;
    }
    const response=await fetch(new URL('/api/gateway/',location.href),{method:'POST',headers:{'Content-Type':'application/json','X-OSG-Client':'1'},body:JSON.stringify({...request,clientId:'companion-connections-visibility-fixture'})});
    const result=await response.json();if(!response.ok||!result.ok)throw Error(result.error);return result.state;
  })()`);
  let initial,enabledByFixture=false,listenerRevision;
  const shortOfferMs=2500;
  let statusReads=0,oldResponse,offerRequests=0,pauseReveal=false,revealResponse,traceError;
  let stopTrace=()=>{};
  const hidden=value=>page.evaluate(`(()=>{if(${value})Object.defineProperty(document,'hidden',{configurable:true,value:true});else delete document.hidden;document.dispatchEvent(new Event('visibilitychange'));})()`);
  const nav=async label=>{
    await until(()=>page.evaluate(`(()=>{const button=[...document.querySelectorAll('nav[aria-label="Main navigation"] button')].find(item=>(item.title||item.textContent.trim())===${JSON.stringify(label)});if(!button)return false;button.click();return true;})()`));
    await until(()=>page.evaluate(`document.querySelector('[aria-label="Breadcrumb"] [aria-current="page"]')?.textContent.trim()===${JSON.stringify(label)}`));
  };
  try{
    await until(()=>page.evaluate('document.readyState === "complete"'));
    initial=(await request({command:'companion_status'})).companions.listener;
    assert.equal(initial.configuredEnabled,false,'Run against the disposable isolated database with a fresh private listener');
    const port=await privateLoopbackPort();
    await page.call('Network.enable',{maxPostDataSize:65536});
    await page.call('Fetch.enable',{patterns:[{urlPattern:'*gateway_request',requestStage:'Response'},{urlPattern:'*/api/gateway/',requestStage:'Response'}]});
    const shortenFirstOffer=async params=>{
      const response=await page.call('Fetch.getResponseBody',{requestId:params.requestId});
      const payload=JSON.parse(response.base64Encoded?Buffer.from(response.body,'base64').toString('utf8'):response.body);
      assert.equal(payload.ok,true);
      assert.ok(payload.state.companions.pairing.browserUrl);
      // Change only the portal's presentation deadline. The native host still owns the real offer.
      payload.state.companions.pairing.ttlMs=shortOfferMs;
      await page.call('Fetch.fulfillRequest',{
        requestId:params.requestId,responseCode:params.responseStatusCode,
        responseHeaders:params.responseHeaders.filter(header=>header.name.toLowerCase()!=='content-length'),
        body:Buffer.from(JSON.stringify(payload)).toString('base64'),
      });
    };
    stopTrace=page.listen(({method,params})=>{
      if(method!=='Fetch.requestPaused')return;
      const path=new URL(params.request.url).pathname;
      let command;
      try{command=(JSON.parse(params.request.postData??'{}').request??JSON.parse(params.request.postData??'{}')).command;}catch{}
      if(path.endsWith('/gateway_request')||path.endsWith('/api/gateway/')){
        if(command==='companion_status'){
          statusReads++;
          if(!oldResponse){oldResponse=params;return;}
          if(pauseReveal&&!revealResponse){revealResponse=params;return;}
        }
        if(command==='companion_pair_offer'&&++offerRequests===1){
          void shortenFirstOffer(params).catch(error=>{traceError=error;});
          return;
        }
      }
      void page.call('Fetch.continueResponse',{requestId:params.requestId}).catch(()=>{});
    });
    await nav('Companions');
    await until(()=>page.evaluate(`!!document.querySelector('.companions[aria-label="Companions"]')`));
    const paused=await until(()=>oldResponse,Boolean,15000).catch(cause=>{
      throw new Error('The production Companions roster did not issue its initial status read.',{cause});
    });
    const beforeHide=statusReads;
    await hidden(true);
    const committed=await request({command:'companion_enable',companion:{
      expectedSettingsRevision:initial.settingsRevision,address:'127.0.0.1',advertisedHost:'127.0.0.1',port,discovery:false,
    }});
    assert.equal(committed.companions.listener.enabled,true);
    enabledByFixture=true;listenerRevision=committed.companions.listener.settingsRevision;
    await delay(350);
    assert.equal(statusReads,beforeHide,'A listener commit while hidden must not demand a roster status read');

    // A marker makes any use of the retired response observable in the mounted roster.
    const marker='Retired roster reply '+randomUUID();
    const response=await page.call('Fetch.getResponseBody',{requestId:paused.requestId});
    const payload=JSON.parse(response.base64Encoded?Buffer.from(response.body,'base64').toString('utf8'):response.body);
    assert.equal(payload.ok,true);
    payload.state.companions.host.clients.push({companionId:randomUUID(),name:marker,kind:'browser',connected:false});
    await page.call('Fetch.fulfillRequest',{
      requestId:paused.requestId,responseCode:paused.responseStatusCode,
      responseHeaders:paused.responseHeaders.filter(header=>header.name.toLowerCase()!=='content-length'),
      body:Buffer.from(JSON.stringify(payload)).toString('base64'),
    });
    await delay(250);
    assert.equal(await page.evaluate(`document.querySelector('.companions')?.textContent.includes(${JSON.stringify(marker)})`),false,
      'The retired reply must not modify the mounted roster');
    assert.equal(statusReads,beforeHide,'An old passive response must not trigger another hidden read');

    await hidden(false);
    await until(()=>statusReads>beforeHide,undefined,15000);
    await until(()=>page.evaluate(`!document.querySelector('.companions')?.innerText.includes('Enable the companion listener in Settings')`));
    await delay(350);
    const revealReads=statusReads-beforeHide;
    assert.ok(revealReads>=1&&revealReads<=2,`Showing the roster should fetch a fresh baseline with no reader loop (saw ${revealReads})`);
    assert.equal(await page.evaluate(`document.querySelector('.companions')?.innerText.includes('No companions paired')`),true,
      'The visible production roster must continue to show its current paired-client state');

    await page.evaluate(`document.querySelector('.companions button.primary-button')?.click()`);
    await until(()=>page.evaluate(`!!document.querySelector('[aria-label="Pair companion"]')`));
    await page.evaluate(`([...document.querySelectorAll('[aria-label="Pair companion"] label')].find(label=>label.textContent.includes('Browser'))?.querySelector('input')?.click())`);
    const pairingLink=()=>page.evaluate(`document.querySelector('[aria-label="Browser pairing link"]')?.value`);
    const firstLink=await until(()=>{if(traceError)throw traceError;return pairingLink();});
    await hidden(true);
    const readsBeforeExpiry=statusReads;
    await delay(shortOfferMs+750);
    assert.equal(offerRequests,1,'An expired offer must not cause a replacement while hidden');
    assert.equal(statusReads,readsBeforeExpiry,'Hidden pairing must not read the roster');

    pauseReveal=true;
    await hidden(false);
    await until(()=>revealResponse);
    await until(pairingLink,value=>!value);
    await delay(300);
    assert.equal(offerRequests,1,'A replacement must wait for the current roster');
    pauseReveal=false;
    await page.call('Fetch.continueResponse',{requestId:revealResponse.requestId});
    revealResponse=undefined;
    await until(pairingLink,value=>Boolean(value)&&value!==firstLink);
    assert.equal(offerRequests,2,'Reopen must create one fresh browser link');
    console.log(JSON.stringify({transport,companionConnectionsHiddenStatusReads:0,retiredReplyIgnored:true,revealReads,hiddenPairingOffers:0,freshPairingOffersAfterRoster:1}));
  }finally{
    try{
      pauseReveal=false;
      if(revealResponse)await page.call('Fetch.continueResponse',{requestId:revealResponse.requestId}).catch(()=>{});
      await hidden(false);
      if(await page.evaluate(`!!document.querySelector('[aria-label="Pair companion"]')`)){
        await page.evaluate(`([...document.querySelectorAll('[aria-label="Pair companion"] button')].find(button=>button.textContent.includes('Cancel')))?.click()`);
        await until(()=>page.evaluate(`!document.querySelector('[aria-label="Pair companion"]')`));
      }
      await page.evaluate(`(()=>{const button=[...document.querySelectorAll('nav[aria-label="Main navigation"] button')].find(item=>(item.title||item.textContent.trim())==='Overview');button?.click();})()`);
      if(enabledByFixture){
        const current=(await request({command:'companion_status'})).companions.listener;
        if(current.configuredEnabled)await request({command:'companion_disable',companion:{expectedSettingsRevision:current.settingsRevision??listenerRevision}});
        assert.equal((await request({command:'companion_status'})).companions.listener.enabled,false,'Restore the disposable listener state');
      }
    }finally{
      stopTrace();
      await page.call('Fetch.disable').catch(()=>{});
      await page.close();
    }
  }
});
