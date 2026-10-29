// Verify the worker itself: page-target DevTools can leave its script row pending.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {get} from 'node:https';
import {verifiedServerKey} from './native/browser-trust.mjs';

const {chromium}=createRequire(import.meta.url)(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');

function asset(url,ca){
  return new Promise((resolve,reject)=>{
    const request=get(url,{ca,signal:AbortSignal.timeout(5000)},response=>{
      const chunks=[];let length=0;
      response.on('error',reject);
      response.on('data',chunk=>{
        length+=chunk.length;
        if(length>2*1024*1024){request.destroy(Error('Browser asset exceeds test budget'));return;}
        chunks.push(chunk);
      });
      response.on('end',()=>{
        try{
          assert.equal(response.statusCode,200);
          assert.equal(length,Number(response.headers['content-length']),'The complete HTTP body must arrive.');
          resolve({text:Buffer.concat(chunks).toString('utf8'),mime:response.headers['content-type'],length});
        }catch(error){reject(error);}
      });
    });
    request.on('error',reject);
  });
}

test('the live companion worker downloads completely and executes before the startup deadline',{timeout:30000},async()=>{
  const native=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  let browser;
  try{
    const cockpit=native.contexts().flatMap(context=>context.pages()).find(page=>page.url().startsWith('http://127.0.0.1:1420/'));
    assert.ok(cockpit,'Start the actual native development Cockpit with CDP enabled.');
    const send=async command=>{
      const reply=await cockpit.evaluate(command=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command,clientId:'browser-worker-download-native'}}),command);
      assert.equal(reply.ok,true,reply.error);return reply.state.companions;
    };
    const initial=await send('companion_status');
    assert.equal(initial.listener.enabled,true);
    const ca=(await send('companion_browser_certificate')).certificatePem;
    const html=await asset(initial.browserUrl,ca);
    const script=html.text.match(/<script\b[^>]*\bsrc="([^"]+)"/);
    assert.ok(script,'The served HTML must reference the embedded browser bundle.');
    const main=await asset(new URL(script[1],initial.browserUrl),ca);
    const workerPath=main.text.match(/\/assets\/connection-worker-[\w-]+\.js/)?.[0];
    assert.ok(workerPath,'Discover the current worker hash from the served bundle.');
    const worker=await asset(new URL(workerPath,initial.browserUrl),ca);
    assert.match(worker.mime,/^(?:text|application)\/javascript\b/);
    assert.ok(worker.length>0);
    const spki=await verifiedServerKey(initial.browserUrl,ca);
    browser=await chromium.launch({headless:true,
      ...(process.env.OSG_BROWSER_EXECUTABLE?{executablePath:process.env.OSG_BROWSER_EXECUTABLE}:{channel:process.env.OSG_BROWSER_CHANNEL??'msedge'}),
      args:['--ignore-certificate-errors-spki-list='+spki]});
    const context=await browser.newContext(),page=await context.newPage();
    await page.goto(initial.browserUrl);
    // A message from the real worker proves download, evaluation and WSS startup.
    // Waiting for page.waitForResponse(workerPath) misdiagnoses Chromium's
    // missing shared-worker completion events as a stalled download.
    const elapsed=await page.evaluate(workerPath=>new Promise((resolve,reject)=>{
      const started=performance.now(),worker=new SharedWorker(workerPath,{type:'module'});
      const cleanup=()=>{clearTimeout(timer);worker.port.postMessage({type:'close'});worker.port.close();};
      const timer=setTimeout(()=>{cleanup();reject(Error('Shared worker did not connect within 4 seconds'));},4000);
      worker.onerror=()=>{cleanup();reject(Error('Shared worker script failed'));};
      worker.port.onmessage=({data})=>{
        if(data.type==='connected'){const elapsed=performance.now()-started;cleanup();resolve(elapsed);}
        else if(data.type==='closed'){cleanup();reject(Error('Shared worker closed before connecting'));}
      };
      worker.port.start();worker.port.postMessage({type:'connect'});
    }),workerPath);
    assert.ok(elapsed<4000);
    console.log(JSON.stringify({workerPath,bytes:worker.length,connectedMs:Math.round(elapsed)}));
  }finally{await browser?.close();await native.close();}
});
