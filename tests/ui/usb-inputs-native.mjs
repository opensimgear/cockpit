import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('running Cockpit USB preview rejects unobserved sources without input queries or authority',{timeout:60000},async()=>{
  const page=await nativePage();const requests=[];let recording=false;
  const unlisten=page.listen(event=>{
    if(!recording||event.method!=='Network.requestWillBeSent')return;
    const request=event.params.request;
    if(!/ipc\.localhost|ipc:\/\//.test(request.url))return;
    let command;try{command=JSON.parse(request.postData).request?.command;}catch{}
    requests.push({url:request.url,command});
  });
  try{
    await until(()=>page.evaluate(`document.title==='OpenSimGear Cockpit'&&!!window.__TAURI_INTERNALS__`));
    await page.call('Network.enable',{maxPostDataSize:65536});recording=true;
    await page.evaluate(`(async()=>{
      const {UsbInputObservations}=await import('/src/lib/usb-input-observations.ts');
      const owned=window.__usbInputAcceptance={client:new UsbInputObservations(),first:[],second:[]};
      const key='usb-input-acceptance-unobserved-endpoint';
      owned.offFirst=owned.client.observe(key,value=>owned.first.push(value));
      owned.offSecond=owned.client.observe(key,value=>owned.second.push(value));
    })()`);
    const rejected=await until(()=>page.evaluate(`({first:window.__usbInputAcceptance.first.at(-1),second:window.__usbInputAcceptance.second.at(-1)})`),
      value=>['lost','error'].includes(value.first?.status)&&['lost','error'].includes(value.second?.status));
    assert.match(rejected.first.error,/Unknown USB input endpoint/);
    assert.equal(rejected.first.value,undefined);assert.equal(rejected.second.value,undefined);
    await page.evaluate(`(async()=>{
      const runtimeUrl=performance.getEntriesByType('resource').map(entry=>entry.name)
        .find(url=>new URL(url).origin===location.origin&&url.includes('/node_modules/.vite/deps/svelte.js'));
      if(!runtimeUrl)throw Error('The running Cockpit Svelte runtime was not loaded.');
      const {mount,unmount}=await import(runtimeUrl);
      const {default:Details}=await import('/src/lib/SystemUsbDetails.svelte');
      const owned=window.__usbInputAcceptance;owned.unmount=unmount;
      owned.target=document.createElement('div');owned.target.id='usb-input-acceptance';document.body.append(owned.target);
      owned.view=mount(Details,{target:owned.target,props:{endpointKey:'usb-input-acceptance-unobserved-endpoint'}});
    })()`);
    await until(()=>page.evaluate(`document.querySelector('#usb-input-acceptance')?.textContent`),
      text=>typeof text==='string'&&/Unknown|no longer|not connected|not present|not found|unavailable/i.test(text));
    // Long enough to expose the removed 750ms refresh, while exercising the
    // actual mounted component and IPC boundary rather than a source scan.
    await new Promise(resolve=>setTimeout(resolve,1700));recording=false;
    const prohibited=requests.filter(row=>/module_usb_events_(state|read)|module_usb_(claim|config_apply|actions_invoke)/.test(row.command??''));
    assert.deepEqual(prohibited,[]);
    const text=await page.evaluate(`document.querySelector('#usb-input-acceptance').textContent`);
    assert.doesNotMatch(text,/Live inputs:/);
    await mkdir('.research/reactive',{recursive:true});
    await writeFile('.research/reactive/usb-native-preview.json',JSON.stringify({at:new Date().toISOString(),
      executableSha256:createHash('sha256').update(await readFile('src-tauri/target/debug/osg-cockpit.exe')).digest('hex'),
      rejected,requests,physicalQualification:false},null,2)+'\n');
  }finally{
    recording=false;unlisten();
    await page.evaluate(`(async()=>{const owned=window.__usbInputAcceptance;if(!owned)return;
      owned.offFirst?.();owned.offSecond?.();if(owned.view)await owned.unmount(owned.view);
      owned.target?.remove();delete window.__usbInputAcceptance;})()`).catch(()=>{});
    page.close();
  }
});
