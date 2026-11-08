// Run against a disposable Cockpit database on Windows with one free compatible vJoy slot.
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {createInterface} from 'node:readline';
import {fileURLToPath} from 'node:url';
import test from 'node:test';
import {browserPage,nativePage,until} from '../../scripts/reactive/cdp.mjs';

const browserUrl=process.env.OSG_SLOT_EXTERNAL_OWNER_BROWSER_URL;
const enabled=!!browserUrl||process.env.OSG_SLOT_EXTERNAL_OWNER_NATIVE==='1';
const transport=browserUrl?'browser-dev':'native';
const feederPath=fileURLToPath(new URL('./external-vjoy-owner.ps1',import.meta.url));
const topic='system.inventory.vjoy';
const outputId='external-owner-slot-fixture';

test(`${transport} visible Profiles slot selector follows external vJoy ownership`,{
  skip:process.platform!=='win32'||!enabled,timeout:90_000,
},async()=>{
  const page=browserUrl?await browserPage(browserUrl):await nativePage();
  const writer=browserUrl?await page.openPeer():page;
  const request=async value=>{
    const payload={...value,clientId:'profile-vjoy-owner-fixture'};
    if(browserUrl){
      const url=new URL('/src/lib/gateway.ts',browserUrl).href;
      return writer.evaluate(`import(${JSON.stringify(url)}).then(({gatewayRequest})=>gatewayRequest(${JSON.stringify(payload)}))`);
    }
    const reply=await writer.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(payload)}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);
    return reply.state;
  };
  const openPath=browserUrl?'/api/observe/subscribe':'/observation_subscribe';
  const closePath=browserUrl?'/api/observe/unsubscribe':'/observation_unsubscribe';
  const gatewayPaths=new Set(['/gateway_request','/api/gateway/']);
  const pending=new Map(),live=new Set(),retired=new Set(),inventoryReads=[];
  let opens=0,closes=0,traceError,feeder;
  const requestKey=value=>JSON.stringify([value.connectionId,value.requestId]);
  await page.call('Network.enable',{maxPostDataSize:65536});
  const stopTrace=page.listen(({method,params})=>{
    const id=params?.requestId;
    if(method==='Network.requestWillBeSent'&&params.request.postData){
      const path=new URL(params.request.url).pathname;
      if(!gatewayPaths.has(path)&&path!==openPath&&path!==closePath)return;
      const body=JSON.parse(params.request.postData);
      const value=body.request??body;
      if(gatewayPaths.has(path)){
        if(value.command==='system_inventory'&&value.clientId!=='profile-vjoy-owner-fixture')inventoryReads.push(value.refresh===true);
        return;
      }
      if(path===openPath&&body.topic===topic)pending.set(id,{opening:true,body});
      else if(path===closePath)pending.set(id,{closing:browserUrl?requestKey(body):body.subscriptionId});
    }
    const entry=pending.get(id);
    if(!entry)return;
    if(method==='Network.responseReceived')entry.status=params.response.status;
    if(method!=='Network.loadingFinished'&&method!=='Network.loadingFailed')return;
    pending.delete(id);
    if(entry.opening){
      void page.call('Network.getResponseBody',{requestId:id}).then(({body})=>{
        assert.equal(entry.status,200);
        const token=browserUrl?JSON.parse(body).subscriptionId:JSON.parse(body);
        assert.equal(typeof token,'string');
        const key=browserUrl?requestKey(entry.body):token;
        opens++;
        if(!retired.has(key))live.add(key);
      }).catch(error=>{traceError=error;});
    }else if(entry.status===200){
      retired.add(entry.closing);
      if(live.delete(entry.closing))closes++;
    }
  });
  const waitDemand=(expected,opened,closed)=>until(()=>{
    if(traceError)throw traceError;
    return live.size===expected&&opens===opened&&closes===closed;
  });
  const navigate=label=>until(()=>page.evaluate(`(()=>{
    const button=[...document.querySelectorAll('nav[aria-label="Main navigation"] button')]
      .find(item=>(item.title||item.textContent.trim())===${JSON.stringify(label)});
    if(!button)return false;button.click();return true;
  })()`));
  const emulateHidden=hidden=>page.evaluate(`(()=>{
    if(${hidden})Object.defineProperty(document,'hidden',{configurable:true,value:true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const picker=()=>page.evaluate(`(()=>{
    const select=document.querySelector('.virtual-slot select');
    if(!select||select.closest('[hidden]')||select.disabled)return null;
    return {selected:select.value,options:[...select.options].map(option=>[option.value,option.textContent.trim()])};
  })()`);
  const slotStatus=async status=>until(picker,value=>value?.options.some(([id,label])=>
    id===String(slot.slot)&&label.includes(' · '+status+' · ')),15_000).catch(async cause=>{
      const current=await picker();
      const label=current?.options.find(([id])=>id===String(slot.slot))?.[1]??'missing or disabled picker';
      throw Error(`Expected slot ${slot.slot} to become ${status}; observed ${label}.`,{cause});
    });
  const created=[];
  const children=[];
  let profile,slot;
  async function startFeeder(){
    feeder=spawn('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',feederPath,'-Slot',String(slot.slot)],
      {windowsHide:true,stdio:['pipe','pipe','pipe']});
    children.push(feeder);
    const lines=createInterface({input:feeder.stdout});
    let stderr='';feeder.stderr.setEncoding('utf8').on('data',value=>{stderr+=value;});
    return new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>reject(Error('External vJoy feeder did not acquire the free slot.')),12_000);
      feeder.once('error',error=>{clearTimeout(timer);reject(error);});
      feeder.once('exit',code=>{if(code!==0){clearTimeout(timer);reject(Error(`Feeder exited early (${code}): ${stderr}`));}});
      lines.once('line',line=>{clearTimeout(timer);try{resolve(JSON.parse(line));}catch(error){reject(error);}});
    });
  }
  const release=async()=>{
    if(!feeder||feeder.exitCode!==null)return;
    const stopped=once(feeder,'exit');feeder.stdin.end('release\n');
    const [code]=await stopped;assert.equal(code,0);
  };
  try{
    await until(()=>page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    await navigate('Overview');
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    slot=await until(async()=>{
      const {virtualDevices}=(await request({command:'system_inventory'})).systemInventory;
      return virtualDevices.find(row=>row.backend==='vjoy'&&row.status==='free'&&row.buttons>0&&Array.isArray(row.axes)
        &&!row.forceFeedback&&!row.discreteHats&&row.buttons+row.axes.length+row.continuousHats<=128);
    },Boolean,12_000);
    assert.ok(slot,'Use a free, already-configured compatible vJoy slot');
    const name='External owner '+randomUUID();
    profile=(await request({command:'profile_create',name})).profiles.document;created.push(profile);
    const draft=await page.evaluate(`import('/src/lib/profile-graph.ts').then(({graphSetVirtual})=>graphSetVirtual(
      {document:${JSON.stringify(profile)},assignments:[]},{id:${JSON.stringify(outputId)},name:'External owner output',required:true,
      layout:${JSON.stringify({buttons:Array.from({length:slot.buttons},(_,index)=>'button-'+(index+1)),
        axes:slot.axes.map((_,index)=>({portId:'axis-'+(index+1),range:'bipolar',fallback:0})),
        hats:Array.from({length:slot.continuousHats},(_,index)=>'hat-'+(index+1))})}}).document)`);
    profile=(await request({command:'profile_editor_save',document:draft,assignments:[],expectedRevision:profile.revision})).profiles.document;
    created[0]=profile;
    await request({command:'profile_virtual_slot_set',id:profile.id,expectedRevision:profile.revision,nodeId:outputId,slot:slot.slot});
    await navigate('Profiles');
    await until(()=>page.evaluate(`(()=>{
      const button=[...document.querySelectorAll('.profiles-workspace button')].find(item=>item.getAttribute('aria-label')===${JSON.stringify('Edit '+name)});
      if(!button||button.disabled)return false;button.click();return true;
    })()`));
    await until(()=>page.evaluate(`(()=>{const block=document.querySelector('[data-node-id="${outputId}"] .block-heading');if(!block)return false;block.click();return true;})()`));
    await until(picker);
    assert.equal((await picker()).selected,String(slot.slot));
    const initialReads=inventoryReads.length;
    const ready=await startFeeder();
    assert.equal(ready.state,'ready');assert.equal(ready.slot,slot.slot);assert.ok(Number.isInteger(ready.pid));
    await slotStatus('busy');
    await waitDemand(1,1,0);
    assert.equal(opens,1,'The visible selector uses one shared sampler subscription');
    await until(()=>inventoryReads.length>=initialReads+1&&inventoryReads.length<=initialReads+2);
    await new Promise(resolve=>setTimeout(resolve,400));
    const busyReads=inventoryReads.length;
    assert.ok(busyReads-initialReads<=2,'Acquisition has bounded cached inventory reads');
    assert.ok(inventoryReads.slice(initialReads).every(refresh=>!refresh),'Ownership updates must not request a full refresh');
    await release();
    await slotStatus('free');
    assert.equal(opens,1,'The visible selector keeps its original subscription through release');
    await until(()=>inventoryReads.length>=busyReads+1&&inventoryReads.length<=busyReads+2);
    await new Promise(resolve=>setTimeout(resolve,400));
    const releasedReads=inventoryReads.length;
    assert.ok(releasedReads-busyReads<=2,'Release has bounded cached inventory reads');
    assert.ok(inventoryReads.slice(initialReads).every(refresh=>!refresh),'Ownership updates must not request a full refresh');

    const second=await startFeeder();
    assert.equal(second.state,'ready');
    await slotStatus('busy');
    await new Promise(resolve=>setTimeout(resolve,400));
    await emulateHidden(true);
    await waitDemand(0,1,1);
    const hiddenReads=inventoryReads.length;
    await release();
    await new Promise(resolve=>setTimeout(resolve,500));
    assert.equal(inventoryReads.length,hiddenReads,'The hidden editor makes no inventory reads after release');
    await emulateHidden(false);
    await slotStatus('free');
    await waitDemand(1,2,1);
    await until(()=>inventoryReads.length>=hiddenReads+1&&inventoryReads.length<=hiddenReads+3);
    assert.ok(inventoryReads.length-hiddenReads<=3,'Reopening has one baseline and bounded cached reads for the change');
    assert.ok(!inventoryReads.slice(initialReads).some(Boolean),'The selector never requests a full inventory refresh');
    await navigate('Overview');
    await waitDemand(0,2,2);
    console.log(JSON.stringify({transport,slot:slot.slot,opens,closes,visibleBusyFree:true,hiddenReads:0,reopenCurrentFree:true}));
  }finally{
    stopTrace();
    for(const child of children){
      if(!child.killed&&child.exitCode===null){child.stdin.end('release\n');await new Promise(resolve=>{
        const timer=setTimeout(()=>{child.kill();resolve();},3000);child.once('exit',()=>{clearTimeout(timer);resolve();});
      });}
    }
    try{
      if(profile){
        const {owner}=(await request({command:'profile_status'})).profiles.profileSession;
        if(created.some(document=>document.id===owner?.profileId))await request({command:'profile_stop'});
        await navigate('Overview');
        for(const document of created)await request({command:'profile_delete',id:document.id,expectedRevision:document.revision});
      }
    }finally{await page.close();}
  }
});
