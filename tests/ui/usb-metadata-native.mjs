import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('running Cockpit metadata inspection stays passive and rejects an unobserved endpoint',{timeout:60000},async()=>{
  const page=await nativePage();const requests=[];let recording=false;
  const off=page.listen(event=>{
    if(!recording||event.method!=='Network.requestWillBeSent')return;
    const request=event.params.request;if(!/ipc\.localhost|ipc:\/\//.test(request.url))return;
    let command,topic;try{const body=JSON.parse(request.postData);command=body.request?.command;topic=body.topic;}catch{}
    requests.push({url:request.url,command,topic});
  });
  try{
    await until(()=>page.evaluate(`document.title==='OpenSimGear Cockpit'&&!!window.__TAURI_INTERNALS__`));
    await page.call('Network.enable',{maxPostDataSize:65536});recording=true;
    await page.evaluate(`(async()=>{
      const {UsbMetadataObservations}=await import('/src/lib/usb-metadata-observations.ts');
      const owned=window.__usbMetadataAcceptance={client:new UsbMetadataObservations(),first:[],second:[]};
      const key='usb-metadata-acceptance-unobserved-endpoint';
      owned.firstOff=owned.client.observe(key,value=>owned.first.push(value));
      owned.secondOff=owned.client.observe(key,value=>owned.second.push(value));
    })()`);
    const rejected=await until(()=>page.evaluate(`({first:window.__usbMetadataAcceptance.first.at(-1),second:window.__usbMetadataAcceptance.second.at(-1)})`),
      value=>['lost','error'].includes(value.first?.status)&&['lost','error'].includes(value.second?.status));
    assert.match(rejected.first.error,/Unknown USB metadata endpoint/);
    assert.equal(rejected.first.value,undefined);assert.equal(rejected.second.value,undefined);
    await page.evaluate(`(async()=>{
      const url=performance.getEntriesByType('resource').map(entry=>entry.name)
        .find(url=>new URL(url).origin===location.origin&&url.includes('/node_modules/.vite/deps/svelte.js'));
      if(!url)throw Error('The running Svelte runtime is unavailable.');
      const {mount,unmount}=await import(url);const {default:Details}=await import('/src/lib/SystemUsbDetails.svelte');
      const owned=window.__usbMetadataAcceptance;owned.unmount=unmount;
      owned.target=document.createElement('div');owned.target.id='usb-metadata-acceptance';document.body.append(owned.target);
      owned.view=mount(Details,{target:owned.target,props:{endpointKey:'usb-metadata-acceptance-unobserved-endpoint'}});
    })()`);
    await until(()=>page.evaluate(`document.querySelector('#usb-metadata-acceptance')?.textContent`),
      text=>typeof text==='string'&&/Unknown|no longer|not connected|not present|not found|unavailable/i.test(text));
    await new Promise(resolve=>setTimeout(resolve,2300));recording=false;
    assert.deepEqual(requests.filter(row=>/module_usb_(events_(state|read)|claim|config_|actions_|connect|disconnect)/.test(row.command??'')),[]);
    assert.ok(requests.filter(row=>row.command==='module_usb_status').length<=1,'mount has at most one cached initial status request');
    await mkdir('.research/reactive',{recursive:true});
    await writeFile('.research/reactive/usb-native-metadata.json',JSON.stringify({at:new Date().toISOString(),
      executableSha256:createHash('sha256').update(await readFile('src-tauri/target/debug/osg-cockpit.exe')).digest('hex'),
      rejected,requests,physicalQualification:false,knownHardwareSourceAvailable:false},null,2)+'\n');
  }finally{
    recording=false;off();
    await page.evaluate(`(async()=>{const owned=window.__usbMetadataAcceptance;if(!owned)return;
      owned.firstOff?.();owned.secondOff?.();if(owned.view)await owned.unmount(owned.view);
      owned.target?.remove();delete window.__usbMetadataAcceptance;})()`).catch(()=>{});
    page.close();
  }
});
