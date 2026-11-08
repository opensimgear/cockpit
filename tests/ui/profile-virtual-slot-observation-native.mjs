// Run against a disposable Cockpit database with one free compatible vJoy slot.
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import test from 'node:test';
import {browserPage,nativePage,until} from '../../scripts/reactive/cdp.mjs';

const browserUrl=process.env.OSG_SLOT_OBSERVATION_BROWSER_URL;
const enabled=!!browserUrl||process.env.OSG_SLOT_OBSERVATION_NATIVE==='1';
const transport=browserUrl?'browser-dev':'native';
const nodeId='slot-observation-fixture';

test(`${transport} virtual slot picker follows committed pin and ownership changes`,{
  skip:!enabled,timeout:90_000,
},async()=>{
  const page=browserUrl?await browserPage(browserUrl):await nativePage();
  const writer=browserUrl?await page.openPeer():page;
  const request=async value=>{
    const payload={...value,clientId:'virtual-slot-observation-writer'};
    if(browserUrl){
      const url=new URL('/src/lib/gateway.ts',browserUrl).href;
      return writer.evaluate(`import(${JSON.stringify(url)}).then(({gatewayRequest})=>gatewayRequest(${JSON.stringify(payload)}))`);
    }
    const reply=await writer.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify(payload)}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);
    return reply.state;
  };
  const navigate=label=>until(()=>page.evaluate(`(()=>{
    const button=document.querySelector('nav[aria-label="Main navigation"] button[title="'+${JSON.stringify(label)}+'"]');
    if(!button)return false;button.click();return true;
  })()`));
  const readPicker=()=>page.evaluate(`(()=>{
    const select=document.querySelector('.virtual-slot select');
    if(!select||select.closest('[hidden]')||select.disabled)return null;
    return {selected:select.value,options:[...select.options].map(option=>[option.value,option.textContent.trim()])};
  })()`);
  const calls=[];
  const stopTrace=page.listen(({method,params})=>{
    if(method!=='Network.requestWillBeSent'||!params.request.postData)return;
    if(!['/gateway_request','/api/gateway/'].includes(new URL(params.request.url).pathname))return;
    const value=JSON.parse(params.request.postData);
    const request=value.request??value;
    if(request.clientId!=='virtual-slot-observation-writer')calls.push(request.command);
  });
  const count=command=>calls.filter(call=>call===command).length;
  const created=[];
  async function profile(name,layout){
    const {document}=(await request({command:'profile_create',name})).profiles;
    created.push(document);
    const draft=await page.evaluate(`import('/src/lib/profile-graph.ts').then(({graphSetVirtual})=>{
      const document=${JSON.stringify(document)};
      return graphSetVirtual({document,assignments:[]},${JSON.stringify({
        id:nodeId,name:'Slot observation fixture',required:true,layout,
      })}).document;
    })`);
    const saved=(await request({command:'profile_editor_save',document:draft,assignments:[],
      expectedRevision:document.revision})).profiles.document;
    created[created.length-1]=saved;
    return saved;
  }
  const pin=(document,slot)=>request({command:'profile_virtual_slot_set',id:document.id,
    expectedRevision:document.revision,nodeId,slot});
  async function open(document){
    await navigate('Profiles');
    await until(()=>page.evaluate(`(()=>{
      if(document.querySelector('.virtual-slot select')&&!document.querySelector('.virtual-slot select').closest('[hidden]'))return true;
      const button=[...document.querySelectorAll('.profiles-workspace button')].find(
        button=>button.getAttribute('aria-label')===${JSON.stringify('Edit '+document.name)});
      if(!button||button.disabled)return !!document.querySelector('[data-node-id="${nodeId}"]');
      button.click();return true;
    })()`));
    if(!await readPicker()){
      await until(()=>page.evaluate(`(()=>{
        if(document.querySelector('.virtual-slot select'))return true;
        const button=document.querySelector('[data-node-id="${nodeId}"] .block-heading');
        if(!button)return false;button.click();return true;
      })()`));
    }
    return until(readPicker);
  }
  try{
    await until(()=>page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    await navigate('Overview');
    const initial=(await request({command:'profile_status'})).profiles.profileSession;
    assert.equal(initial.owner,null);
    const slot=await until(async()=>{
      const {virtualDevices}=(await request({command:'system_inventory'})).systemInventory;
      return virtualDevices.find(row=>row.backend==='vjoy'&&row.status==='free'&&row.buttons>0&&Array.isArray(row.axes)
        &&!row.forceFeedback&&!row.discreteHats&&row.buttons+row.axes.length+row.continuousHats<=128);
    },Boolean,12_000).catch(cause=>{throw new Error('Use an unused compatible vJoy slot',{cause});});
    const marker=randomUUID();
    const reader=await profile('Visible slot '+marker,{buttons:['button-1'],axes:[],hats:[]});
    const lease=await profile('Independent lease '+marker,{
      buttons:Array.from({length:slot.buttons},(_,index)=>'button-'+(index+1)),
      axes:slot.axes.map((_,index)=>({portId:'axis-'+(index+1),range:'bipolar',fallback:0})),
      hats:Array.from({length:slot.continuousHats},(_,index)=>'hat-'+(index+1)),
    });
    await pin(lease,slot.slot);
    await page.call('Network.enable',{maxPostDataSize:65536});
    assert.equal((await open(reader)).selected,'');
    const pins=count('profile_virtual_slots_get'),inventory=count('system_inventory');
    await pin(reader,slot.slot);
    await until(readPicker,value=>value?.selected===String(slot.slot));
    assert.equal(count('profile_virtual_slots_get'),pins+1);
    assert.equal(count('system_inventory'),inventory);

    const applied=(await request({command:'profile_apply',id:lease.id,
      expectedRevision:lease.revision,expectedActiveOwner:null})).profiles.profileSession;
    assert.equal(applied.owner?.profileId,lease.id);
    const slotStatus=status=>until(readPicker,value=>value?.options.some(([id,label])=>
      id===String(slot.slot)&&label.includes(' · '+status+' · ')));
    await slotStatus('busy');
    await request({command:'profile_stop'});
    await slotStatus('free');
    assert.equal(count('profile_virtual_slots_get'),pins+1,'A different owner must not reload this pin');

    await navigate('Overview');
    await until(()=>page.evaluate(`document.querySelector('[aria-label="Breadcrumb"] [aria-current="page"]')?.textContent.trim()==='Overview'`));
    const hiddenReads=count('profile_virtual_slots_get');
    await pin(reader,null);
    await delay(250);
    assert.equal(count('profile_virtual_slots_get'),hiddenReads);
    assert.equal((await open(reader)).selected,'');
    assert.equal(count('profile_virtual_slots_get'),hiddenReads+1);
    console.log(JSON.stringify({transport,visiblePinReads:1,hiddenPinReads:0,reopenPinReads:1,leaseReleases:true}));
  }finally{
    stopTrace();
    try{
      const {owner}=(await request({command:'profile_status'})).profiles.profileSession;
      if(created.some(document=>document.id===owner?.profileId))await request({command:'profile_stop'});
      await navigate('Overview');
      for(const document of created)await request({command:'profile_delete',id:document.id,expectedRevision:document.revision});
    }finally{await page.close();}
  }
});
