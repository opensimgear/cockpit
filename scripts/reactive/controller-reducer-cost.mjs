// Synthetic reducer-only diagnostic. Excludes transport, DOM and native capture.
import {execFileSync} from 'node:child_process';
import {mkdir,writeFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
import {resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {ControllerInputObservations as Current} from '../../src/lib/controller-observations.ts';
await mkdir('.research/reactive',{recursive:true});
const baseline=execFileSync('git',['show','a0bf5a3:src/lib/controller-observations.ts'],{encoding:'utf8'})
  .replaceAll("'./", "'../../src/lib/");
const path=resolve('.research/reactive/controller-reducer-baseline.ts');
await writeFile(path,baseline);
const {ControllerInputObservations:Before}=await import(pathToFileURL(path).href);
const id='11111111-1111-4111-8111-111111111111';
const controls=Array.from({length:137},(_,index)=>{
  const kind=index<128?'button':index<134?'axis':index===134?'hat':'diagnostic';
  return {capability:{id:'control-'+index,kind,usagePage:kind==='button'?9:1,usage:index+1,
    reportId:1,collection:0,dataIndex:index,bits:kind==='button'?1:kind==='hat'?4:12,
    min:0,max:kind==='button'?1:kind==='hat'?7:4095,hasNull:kind==='hat'},
    value:{native:0,normalized:kind==='diagnostic'?null:0}};
});
const source={id,name:'Synthetic',role:'controller',origin:'host-controller',backend:'diagnostic',vendorId:1,productId:2,
  osgHid:false,online:true,generation:'1',sequence:'1',controlCount:137,baselineReady:true,identityConfidence:'local-path'};
const frame=(payload,n,kind='change')=>({kind,topic:'controllers.input.'+id,source:'controller-input',session:'wire',
  generation:'2',revision:String(n),capturedUs:String(n*1000),clockId:'controllers:capture',
  continuity:kind==='snapshot'?'baseline':'continuous',payload});
const results=[];
for(const [version,Store]of[['before',Before],['after',Current]])for(const changed of[false,true]){
  let receive,last;
  const store=new Store({subscribe(_topic,listener){receive=listener;return()=>{};}});
  const stop=store.observe(id,state=>{last=state;});
  const stopOther=store.observe(id,()=>{});
  receive({status:'ready',frame:frame({session:'capture',source,identity:{path:'synthetic',collection:'benchmark',serial:null,fingerprint:'a'.repeat(64)},
    diagnostics:[],controls,offset:0,total:137,limit:256,cursor:'1',capturedUs:'1000'},1,'snapshot')});
  if(last.status!=='ready')throw Error(last.error);
  const times=[];
  for(let n=2;n<4502;n++){
    const values=Object.fromEntries(controls.map(({capability:c})=>{
      const native=changed?n%(c.max+1):0;
      const normalized=c.kind==='diagnostic'?null:c.kind==='axis'?Math.trunc(native*32767/4095):native;
      return[c.id,{native,normalized}];
    }));
    const packet=frame({session:'capture',source:{...source,sequence:String(n)},reason:'change',values,cursor:String(n),capturedUs:String(n*1000)},n);
    const started=performance.now();receive({status:'ready',frame:packet});const elapsed=performance.now()-started;
    if(last.status!=='ready'||last.detail.controls[0].value.native!==(changed?n%2:0))throw Error('Invalid benchmark frame');
    if(n>=502)times.push(elapsed);
  }
  stop();stopOther();times.sort((a,b)=>a-b);
  results.push({version,allValuesChanged:changed,samples:times.length,p50Ms:times[times.length/2],p95Ms:times[Math.floor(times.length*.95)]});
}
await writeFile('.research/reactive/controller-reducer-cost.json',JSON.stringify(results,null,2));
console.log(JSON.stringify(results,null,2));
