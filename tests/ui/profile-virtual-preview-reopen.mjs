import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import test from 'node:test';
import {setTimeout as delay} from 'node:timers/promises';
import {browserPage,nativePage,until} from '../../scripts/reactive/cdp.mjs';
import {alignClock,summarize} from './dcs-panel-presentation-timing.mjs';
import {startPreviewVjoyProbe} from './virtual-preview-vjoy-timing.mjs';

const browserUrl=process.env.OSG_VIRTUAL_PREVIEW_BROWSER_URL;
const preview='[aria-label="Virtual controller inputs"]';
const outputId='reopen-vjoy';
const rawProbeExecutable=process.env.OSG_VIRTUAL_PREVIEW_RAW_PROBE;

async function measureVisiblePreview(page,owner,streamId){
  const buttonSelector=preview+' [data-control-id="button-1"]';
  await page.evaluate(`(async()=>{
    const {gatewayRequest}=await import('/src/lib/gateway.ts');
    const button=document.querySelector(${JSON.stringify(buttonSelector)});
    button.scrollIntoView({block:'center',inline:'nearest'});
    let current=null;
    const observer=new MutationObserver(()=>{
      const rect=button.getBoundingClientRect();
      if(!current||current.dom!==undefined||document.visibilityState!=='visible'||!button.isConnected
        ||rect.width===0||rect.height===0||rect.top<0||rect.bottom>innerHeight
        ||rect.left<0||rect.right>innerWidth||button.classList.contains('pressed')!==current.value)return;
      const sample=current;
      sample.dom=performance.now();
      requestAnimationFrame(()=>{sample.frame=performance.now();});
    });
    observer.observe(button,{attributes:true,attributeFilter:['class']});
    window.__virtualPreviewTiming={
      get current(){return current;},
      async send(sequence,value){
        const request={command:'profile_debug_controller_input',owner:${JSON.stringify(owner)},
          nodeId:'debug',streamId:${JSON.stringify(streamId)},sequence:String(sequence),
          changes:[{portId:'button-1',value}]};
        current={sequence,value,submitted:performance.now()};
        const {debugController}=await gatewayRequest(request);
        if(debugController?.sequence!==String(sequence)||debugController.replayed
          ||!Number.isInteger(debugController.pendingFrames)||debugController.pendingFrames<1
          ||debugController.pendingFrames>256)throw Error('Expected one admitted debug input sequence');
        current.pendingFrames=debugController.pendingFrames;
      },
      close(){observer.disconnect();},
    };
  })()`);
  try{
    const samples=[];
    for(let index=0;index<64;index++){
      if(index)await delay(50);
      const sequence=index+3,value=index%2===0;
      await page.evaluate(`window.__virtualPreviewTiming.send(${sequence},${value})`);
      const sample=await until(()=>page.evaluate('window.__virtualPreviewTiming.current'),
        row=>Number.isFinite(row.frame),1000);
      assert.equal(sample.sequence,sequence);
      assert.equal(sample.value,value);
      assert.ok(sample.submitted<=sample.dom&&sample.dom<=sample.frame);
      samples.push(sample);
    }
    return {
      submitToDomMs:summarize(samples.map(row=>row.dom-row.submitted)),
      submitToFrameMs:summarize(samples.map(row=>row.frame-row.submitted)),
      submissionIntervalMs:summarize(samples.slice(1).map((row,index)=>row.submitted-samples[index].submitted)),
      pendingFramesPeak:Math.max(...samples.map(row=>row.pendingFrames)),
      samples,
    };
  }finally{
    await page.evaluate('window.__virtualPreviewTiming.close(); delete window.__virtualPreviewTiming;');
  }
}

test('virtual inputs mount hidden, reopen on the current native sample, and retire exactly',{
  skip:!browserUrl&&process.env.OSG_VIRTUAL_PREVIEW_NATIVE!=='1',timeout:90000,
},async()=>{
  const page=browserUrl?await browserPage(browserUrl):await nativePage();
  const writer=browserUrl?await page.openPeer():page;
  const request=value=>writer.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest})=>gatewayRequest(${JSON.stringify(value)}))`);
  const click=selector=>until(()=>page.evaluate(`(()=>{
    const node=document.querySelector(${JSON.stringify(selector)});
    if(!node||node.disabled)return false;node.click();return true;
  })()`));
  const visible=value=>page.evaluate(`(()=>{
    if(${value})delete document.hidden;
    else Object.defineProperty(document,'hidden',{configurable:true,value:true});
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const state=()=>page.evaluate(`(()=>{
    const view=document.querySelector(${JSON.stringify(preview)});
    return view?{mode:view.querySelector('.heading span').textContent,
      pressed:view.querySelector('[data-control-id="button-1"]').classList.contains('pressed')}:null;
  })()`);
  const openPath=browserUrl?'/api/observe/subscribe':'/observation_subscribe';
  const closePath=browserUrl?'/api/observe/unsubscribe':'/observation_unsubscribe';
  const gatewayPath=browserUrl?'/api/gateway/':'/gateway_request';
  const opens=[],closes=[],reads=[],pending=new Map();
  let traceError,profile,owner,slot;
  const pair=body=>JSON.stringify([body.connectionId,body.requestId]);
  const stopTrace=page.listen(({method,params})=>{
    const id=params?.requestId;
    if(method==='Network.requestWillBeSent'&&params.request.postData){
      const path=new URL(params.request.url).pathname;
      if(![openPath,closePath,gatewayPath].includes(path))return;
      const body=JSON.parse(params.request.postData);
      if(path===openPath&&body.topic?.startsWith('profiles.samples.')){
        const open={id,topic:body.topic,token:browserUrl?pair(body):undefined};
        opens.push(open);
        if(!browserUrl)pending.set(id,{open});
      }else if(path===closePath)pending.set(id,{closing:browserUrl?pair(body):body.subscriptionId});
      else if(path===gatewayPath&&(body.request??body).command==='profile_runtime_inspect')reads.push(body);
    }
    const entry=pending.get(id);
    if(!entry)return;
    if(method==='Network.responseReceived')entry.status=params.response.status;
    if(method!=='Network.loadingFinished'&&method!=='Network.loadingFailed')return;
    pending.delete(id);
    if(entry.open&&entry.status===200){
      void page.call('Network.getResponseBody',{requestId:id}).then(({body})=>{
        entry.open.token=JSON.parse(body);
      }).catch(error=>traceError=error);
    }else if(entry.closing&&entry.status===200)closes.push(entry.closing);
  });
  const waitTrace=predicate=>until(()=>{if(traceError)throw traceError;return predicate();});
  const inventory=async()=>(await request({command:'system_inventory'})).systemInventory.virtualDevices;
  const streamId=randomUUID();
  const send=(sequence,value)=>request({command:'profile_debug_controller_input',owner,nodeId:'debug',streamId,
    sequence:String(sequence),changes:[{portId:'button-1',value}]});
  try{
    await until(()=>page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    slot=await until(async()=>(await inventory()).find(row=>row.status==='free'&&row.buttons>0&&Array.isArray(row.axes)
      &&!row.forceFeedback&&!row.discreteHats&&row.buttons+row.axes.length+row.continuousHats<=128));
    const layout={buttons:Array.from({length:slot.buttons},(_,i)=>'button-'+(i+1)),
      axes:slot.axes.map((_,i)=>({portId:'axis-'+(i+1),range:'bipolar',fallback:0})),
      hats:Array.from({length:slot.continuousHats},(_,i)=>'hat-'+(i+1))};
    const name='Hidden virtual preview '+randomUUID();
    profile=(await request({command:'profile_create',name})).profiles.document;
    const document=await page.evaluate(`(async()=>{
      const {graphSetVirtual,graphConnect}=await import('/src/lib/profile-graph.ts');
      const document=${JSON.stringify(profile)};
      const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
      document.resources.push({id:'debug',name:'Authored input',kind:'debug-controller',required:false,
        ports:[{id:'button-1',name:'Button',direction:'output',contract}]});
      const draft=graphSetVirtual({document,assignments:[]},{id:${JSON.stringify(outputId)},
        name:'Preview output',required:true,layout:${JSON.stringify(layout)}});
      return graphConnect(draft,{nodeId:'debug',portId:'button-1'},
        {nodeId:${JSON.stringify(outputId)},portId:'button-1'}).document;
    })()`);
    profile=(await request({command:'profile_editor_save',document,assignments:[],expectedRevision:profile.revision})).profiles.document;
    await request({command:'profile_virtual_slot_set',id:profile.id,expectedRevision:profile.revision,nodeId:outputId,slot:slot.slot});
    owner=(await request({command:'profile_apply',id:profile.id,expectedRevision:profile.revision,expectedActiveOwner:null})).profiles.profileSession.owner;
    assert.equal(owner?.profileId,profile.id);
    await click('nav[aria-label="Main navigation"] button[title="Profiles"]');
    await click(`button[aria-label="Edit ${name}"]`);
    await page.call('Network.enable',{maxPostDataSize:65536});
    await visible(false);
    await click(`[data-node-id="${outputId}"] .block-heading`);
    await until(state,value=>value?.mode==='Preview');
    assert.equal(opens.length,0,'An initially hidden preview must wait for visibility');
    await send(1,true);
    await visible(true);
    await until(state,value=>value?.mode==='Live'&&value.pressed);
    const first=await waitTrace(()=>opens[0]?.token);
    assert.equal(opens.length,1);

    await visible(false);
    await waitTrace(()=>closes.includes(first));
    await send(2,false);
    assert.equal(opens.length,1);
    await visible(true);
    await until(state,value=>value?.mode==='Live'&&!value.pressed);
    const second=await waitTrace(()=>opens[1]?.token);
    assert.notEqual(second,first);
    assert.equal(opens[1].topic,opens[0].topic);
    const rawProbe=rawProbeExecutable?await startPreviewVjoyProbe(rawProbeExecutable):null;
    let timing,vjoy;
    try {
      const before=rawProbe?await alignClock(page):null;
      timing=await measureVisiblePreview(page,owner,streamId);
      if(rawProbe) {
        vjoy=await rawProbe.finish(slot.slot,timing.samples,before,await alignClock(page));
        const {p95,p99}=vjoy.submitToRawUpperMs;
        assert.ok(p95<=20&&p99<=50,`Raw Input budget exceeded with p95 ${p95} ms and p99 ${p99} ms`);
      }
    } finally { await rawProbe?.close(); }
    assert.ok(timing.submitToFrameMs.p95<=100,'Foreground preview presentation exceeded 100 ms p95');
    assert.deepEqual((await request({command:'profile_status'})).profiles.profileSession.owner,owner);
    assert.deepEqual(reads,[],'The applied virtual preview should use samples without inspection queries');

    await click('nav[aria-label="Main navigation"] button[title="Overview"]');
    await waitTrace(()=>closes.includes(second));
    assert.equal(opens.length,2);
    await request({command:'profile_stop'});
    await until(inventory,rows=>rows.find(row=>row.slot===slot.slot)?.status==='free');
    console.log(JSON.stringify({transport:browserUrl?'browser-dev':'native',opens:2,retirements:2,
      hiddenInitialOpen:false,currentOnReopen:true,inspectionReads:0,slotFreed:true,
      profile:{id:owner.profileId,revision:owner.profileRevision},slot:slot.slot,timing,vjoy}));
  }finally{
    stopTrace();
    try{
      await visible(true);
      if(profile){
        const active=(await request({command:'profile_status'})).profiles.profileSession.owner;
        if(active?.profileId===profile.id)await request({command:'profile_stop'});
        await request({command:'profile_delete',id:profile.id,expectedRevision:profile.revision});
      }
    }finally{await page.close();}
  }
});
