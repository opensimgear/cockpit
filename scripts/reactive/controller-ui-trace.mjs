// Opt-in measurement: open the running native controller inspector first.
import {nativePage} from './cdp.mjs';
import {writeFile} from 'node:fs/promises';
const page=await nativePage(),calls=[];
try{
  const read=()=>page.evaluate(`({text:document.body.innerText,inputs:document.querySelector('.host-detail')?.innerText})`);
  const before=await read();
  if(!before.inputs?.includes('Live inputs'))throw Error('Open the actual controller inspector before measuring.');
  const stop=page.listen(event=>{
    if(event.method==='Network.requestWillBeSent'){
      let command;try{command=JSON.parse(event.params.request.postData).request?.command;}catch{}
      if(command)calls.push(command);
    }
  });
  await page.call('Network.enable');
  await new Promise(resolve=>setTimeout(resolve,5000));
  stop();
  const after=await read();
  if(calls.includes('controller_inspect'))throw Error('Inspector still queries controller pages.');
  await writeFile('.research/reactive/controller-ui-trace.json',JSON.stringify({at:new Date().toISOString(),durationMs:5000,before,after,calls},null,2));
  console.log(JSON.stringify({durationMs:5000,inspectRequests:0,commands:calls.length,reportBefore:before.inputs.match(/report \d+/)?.[0],reportAfter:after.inputs.match(/report \d+/)?.[0]}));
}finally{page.close()}
