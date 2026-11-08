// Browser-dev acceptance for the app-level activation progress presenter.
// Observation frames are authored at the browser transport boundary; no
// activation, output, hardware, or configuration command is sent.
import assert from 'node:assert/strict';
import test from 'node:test';
import {createRequire} from 'node:module';
import {until} from '../../scripts/reactive/cdp.mjs';

const require=createRequire(import.meta.url);
const origin=process.env.OSG_BROWSER_COCKPIT_URL;
const activationId='00000000-0000-4000-8000-000000000196';
const profileId='00000000-0000-4000-8000-000000000197';
const topic='profiles.activation';

test('app-level activation progress keeps the latest background frame and clears terminal work on reveal',{skip:!origin,timeout:90000},async()=>{
  const {chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'playwright');
  const browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})});
  const context=await browser.newContext();
  const page=await context.newPage();
  const peer=await context.newPage();
  await page.addInitScript(()=>{
    const fixture={connections:[],subscriptions:[],acks:[],token:null,delivery:0};
    window.__profileProgressFixture=fixture;
    const json=value=>new Response(JSON.stringify(value),{headers:{'content-type':'application/json'}});
    fixture.publish=frame=>{
      if(!fixture.writer||!fixture.token)throw new Error('Observation stream is not subscribed.');
      const envelope={subscriptionId:fixture.token,delivery:String(++fixture.delivery),frame};
      fixture.writer.enqueue(new TextEncoder().encode(`${JSON.stringify({requestId:fixture.requestId,envelope})}\n`));
    };
    // Install a controllable NDJSON writer at the browser observation boundary.
    const originalFetch=window.fetch.bind(window);
    window.fetch=async(input,init={})=>{
      const url=new URL(typeof input==='string'?input:input.url,location.href);
      if(url.pathname==='/api/observe/'){
        const body=JSON.parse(init.body),stream=new ReadableStream({start(writer){fixture.writer=writer;writer.enqueue(new TextEncoder().encode(`${JSON.stringify({type:'ready',connectionId:body.connectionId})}\n`));},cancel(){}});
        fixture.connections.push(body.connectionId);return new Response(stream,{headers:{'content-type':'application/x-ndjson'}});
      }
      if(url.pathname==='/api/observe/subscribe'){
        const body=JSON.parse(init.body);
        const token=`profile-progress-${body.requestId}`;
        fixture.subscriptions.push({topic:body.topic,requestId:body.requestId,token});
        if(body.topic==='profiles.activation'){fixture.token=token;fixture.requestId=body.requestId;}
        return json({ok:true,subscriptionId:token});
      }
      if(url.pathname==='/api/observe/ack'){
        const body=JSON.parse(init.body);
        fixture.acks.push({subscriptionId:body.subscriptionId,delivery:body.delivery});
        return new Response(null,{status:200});
      }
      if(url.pathname==='/api/observe/unsubscribe'){
        return json({ok:true});
      }
      return originalFetch(input,init);
    };
  });
  const cdp=await context.newCDPSession(page);
  await cdp.send('Emulation.setFocusEmulationEnabled',{enabled:false});
  let visibilityMode;
  const visibility=async hidden=>{
    if(hidden){await peer.goto('about:blank');await peer.bringToFront();}
    else await page.bringToFront();
    if(visibilityMode===undefined){
      try{
        await until(()=>page.evaluate(()=>document.hidden),value=>value===hidden,1200);
        visibilityMode='real-tab-background';
      }catch{visibilityMode='synthetic-document-visibility';}
    }
    if(visibilityMode==='synthetic-document-visibility')await page.evaluate(hidden=>{
      Object.defineProperty(document,'hidden',{configurable:true,value:hidden});
      Object.defineProperty(document,'visibilityState',{configurable:true,value:hidden?'hidden':'visible'});
      document.dispatchEvent(new Event('visibilitychange'));
    },hidden);
    await until(()=>page.evaluate(()=>document.hidden),value=>value===hidden,1200);
  };
  const frame=(revision,sequence,payload,kind)=>({kind,topic,source:'profile-activation',session:'profile-progress-fixture',generation:'1',revision:String(revision),sequence:String(sequence),capturedUs:String(revision),clockId:'profile-progress-fixture',continuity:kind==='snapshot'?'baseline':'continuous',payload});
  const progress=(phase,message,canCancel)=>({activationId,profileId,phase,origin:'manual',profileName:'Fixture profile',step:phase,message,cancelRequested:false,canCancel,companions:[],outcome:null,error:null});
  try{
    await page.goto(origin);
    await until(()=>page.evaluate(()=>!!document.querySelector('nav[aria-label="Main navigation"]')));
    const observed=await page.evaluate(async()=>{
      const {observeProfileActivation}=await import('/src/lib/profile-activation-observation.ts');
      window.__profileProgressStates=[];
      window.__profileProgressStop=observeProfileActivation((value,error)=>window.__profileProgressStates.push({value,error}));
      return true;
    });
    assert.equal(observed,true);
    await until(()=>page.evaluate(()=>window.__profileProgressFixture.subscriptions.some(item=>item.topic==='profiles.activation')));
    const send=(revision,sequence,value,kind='change')=>page.evaluate(
      value=>window.__profileProgressFixture.publish(value),frame(revision,sequence,value,kind));
    await send(1,1,progress('preparing','Preparing saved profile A.',true),'snapshot');
    const dialog=page.locator('dialog[aria-label="Profile operation progress"]');
    await until(()=>dialog.locator('[role="status"]').innerText(),value=>value==='Preparing saved profile A.');
    assert.equal(await dialog.getByRole('button',{name:'Cancel',exact:true}).isEnabled(),true);

    await visibility(true);
    await send(2,2,progress('deactivating','Cleaning up profile B safely.',false));
    await until(()=>page.evaluate(()=>window.__profileProgressFixture.acks.length),value=>value===2);
    await visibility(false);
    await until(()=>dialog.locator('[role="status"]').innerText(),value=>value==='Cleaning up profile B safely.');
    assert.equal(await dialog.getByRole('button',{name:'Cancel',exact:true}).count(),0,'Cleanup progress is not cancellable.');
    const states=await page.evaluate(()=>window.__profileProgressStates.map(item=>item.value?.message??null));
    assert.ok(states.includes('Cleaning up profile B safely.'));
    assert.equal((await page.evaluate(()=>window.__profileProgressFixture.subscriptions.filter(item=>item.topic==='profiles.activation').length)),1,'App presenter and an independent observer share one topic subscription.');
    assert.equal((await page.evaluate(()=>window.__profileProgressFixture.connections.length)),1,'The shared topic uses one browser transport connection.');

    await visibility(true);
    await send(3,3,progress('succeeded','Profile operation complete.',false));
    await until(()=>page.evaluate(()=>window.__profileProgressFixture.acks.length),value=>value===3);
    await visibility(false);
    await until(()=>dialog.count(),value=>value===0);
    assert.equal(await page.getByText('Preparing saved profile A.',{exact:true}).count(),0,'No stale cancellable progress survives terminal completion.');
    assert.equal(await page.getByText('Cleaning up profile B safely.',{exact:true}).count(),0,'No stale cleanup progress survives terminal completion.');
    await page.evaluate(()=>window.__profileProgressStop?.());
    await page.reload();
    await until(()=>page.evaluate(()=>!!window.__profileProgressFixture?.token));
    await send(3,3,progress('succeeded','Profile operation complete.',false),'snapshot');
    await until(()=>page.evaluate(()=>window.__profileProgressFixture.acks.length),value=>value===1);
    assert.equal(await dialog.count(),0,'Reopening on a retained terminal baseline does not show a progress dialog.');
    console.log(JSON.stringify({mode:'browser-dev-scripted-observation',visibilityMode,topicSubscriptions:1,transportConnections:1,deliveries:3,acks:3,terminalBaselineAfterReload:true}));
  }finally{
    await context.close();await browser.close();
  }
});
