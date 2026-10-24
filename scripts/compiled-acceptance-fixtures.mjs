import {readFile,writeFile} from 'node:fs/promises';
import assert from 'node:assert/strict';
import {compileWidget} from '../packages/widget-compiler/src/index.ts';
import {canonicalWidget,widgetDigest,BROWSER_WIDGET_CAPABILITIES} from '../packages/widget-runtime/src/artifact.ts';
import {createCompiledPanel,compiledPanelCapabilityProjection,verifyCompiledPanelPublication} from '../packages/widget-runtime/src/panel.ts';

// These are protocol probes, not shipped controls or a slider implementation.
function source(id,{controls=true,feedback=true,bulk=false}={}){
  const inputPorts=controls?[...['button','shared','encoder','macro'].map(id=>({id,label:id,type:'boolean',signal:id+'Value'})),{id:'axis',label:'Axis',type:'number',signal:'axisValue',min:0,max:1}]:[];
  const signals=[...inputPorts.map(p=>({id:p.signal,source:'local',type:p.type,initial:p.type==='boolean'?false:0,...(p.type==='number'?{min:0,max:1}:{})})),...(feedback?[{id:'lamp',source:'feedback',type:'boolean',initial:false},
    {id:'caption',source:'feedback',type:'enum',initial:bulk?'L'.repeat(64):'local',values:bulk?['L'.repeat(64),'blank']:['local','blank']},
    {id:'level',source:'feedback',type:'number',initial:0,min:0,max:100}]:[])];
  return {id,title:'Protocol probe',size:{width:32,height:32},parameters:[],
    behavior:{version:1,signals,events:[...(controls?[{id:'activate'}]:[]),...(feedback?[{id:'feedback',updates:['lamp','caption','level']}]:[])],
      outputs:inputPorts.map(p=>({id:p.id,type:p.type,neutral:p.type==='boolean'?false:0,...(p.type==='number'?{min:0,max:1}:{})})),
      regions:controls?[{id:'interaction',initial:'ready',states:['ready'],transitions:[{id:'press',from:'ready',event:'activate',actions:[{type:'emit',port:'button',value:{literal:true}},{type:'emit',port:'axis',value:{literal:.75}}]}]}]:[]},
    visualDomains:[],inputs:inputPorts,
    outputs:feedback?(bulk?[{id:'caption',label:'Text',type:'enum',signal:'caption',values:['L'.repeat(64),'blank']}]:[
      {id:'lamp',label:'Indicator',type:'boolean',signal:'lamp'},
      {id:'caption',label:'Text',type:'enum',signal:'caption',values:['local','blank']},
      {id:'level',label:'Gauge',type:'number',signal:'level',min:0,max:100}]):[],
    hitRegions:controls?[{id:'press',x:0,y:0,width:32,height:32,events:{activate:'activate'}}]:[],
    scene:[{id:'pixels',kind:'rectangle',x:0,y:0,width:32,height:32,fill:'#71e5c3'}],motions:[],sounds:[],assets:[]};
}
const options={size:{width:32,height:32},motion:false,sound:false};
const normal=await compileWidget(source('acceptance-probe'),options);
const inputOnly=await compileWidget(source('acceptance-input',{feedback:false}),options);
const textOnly=await compileWidget(source('acceptance-text',{controls:false,bulk:true}),options);
async function publication(name,bulk=false){
  const placement=(id,bundle,index=0)=>({id,pageId:'page',label:id,bounds:{x:(index%10)/10,y:Math.floor(index/10)/10,width:32/800,height:32/480},bundle});
  const panel=await createCompiledPanel({id:name+'-panel',title:name,viewports:[{pageId:'page',title:'Page',width:800,height:480}]},
    bulk?[placement('probe',inputOnly),...Array.from({length:64},(_,i)=>placement('bulk-'+i,textOnly,i))]:[placement('probe',normal)]);
  const d=panel.document,ids=new Map();
  const inputOrder={button:0,shared:1,axis:2,encoder:5,macro:6};
  d.ui.controls.forEach(c=>{const binding=d.presentation.widgets.flatMap(w=>w.inputBindings).find(b=>b.controlId===c.id);const id=name+'-input-'+inputOrder[binding.portId];ids.set(c.id,id);c.id=id;});
  const order={lamp:0,caption:1,level:2};
  for(const w of d.presentation.widgets){
    for(const b of w.inputBindings)b.controlId=ids.get(b.controlId);
    for(const b of w.outputBindings){const f=d.ui.feedbackWidgets.find(f=>f.id===b.feedbackId);
      const index=bulk?Number(w.id.slice(5)):order[b.portId];
      f.id=bulk?'max-output-'+index+'-'+ 'output'.repeat(6):name+'-output-'+index;
      f.channelId=bulk?'max-channel-'+index+'-'+ 'channel'.repeat(6):name+'-channel-'+index;b.feedbackId=f.id;}
  }
  d.ui.feedbackWidgets.sort((a,b)=>a.channelId.localeCompare(b.channelId,undefined,{numeric:true}));
  for(const b of d.bindings)b.controlId=ids.get(b.controlId);
  panel.publicationHash='sha256:'+await widgetDigest(new TextEncoder().encode(canonicalWidget(d)));
  panel.capabilityFingerprint='sha256:'+await widgetDigest(new TextEncoder().encode(canonicalWidget(compiledPanelCapabilityProjection(d))));
  await verifyCompiledPanelPublication(panel);
  return {canonicalDocument:canonicalWidget(d),publicationHash:panel.publicationHash,capabilityFingerprint:panel.capabilityFingerprint,assets:panel.assets};
}
for(const [name,bulk] of [['flight',false],['racing',false],['bounded-feedback',true]]){
  const path=new URL('./fixtures/compiled-'+name+'-publication.json',import.meta.url),text=JSON.stringify(await publication(name,bulk),null,2)+'\n';
  if(process.argv.includes('--check'))assert.equal((await readFile(path,'utf8')).replaceAll('\r\n','\n'),text,'Compiled acceptance fixture is stale: '+name);
  else await writeFile(path,text);
}
console.log('Compiled flight/racing and 64-channel feedback publications verified.');
const capabilityPath=new URL('./fixtures/compiled-widget-capabilities.json',import.meta.url),capabilityText=JSON.stringify(BROWSER_WIDGET_CAPABILITIES,null,2)+'\n';
if(process.argv.includes('--check'))assert.equal((await readFile(capabilityPath,'utf8')).replaceAll('\r\n','\n'),capabilityText);
else await writeFile(capabilityPath,capabilityText);
