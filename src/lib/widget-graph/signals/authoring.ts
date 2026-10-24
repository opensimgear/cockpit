import type { WidgetSource, VisualDomain, SignalDomain } from '../../../../packages/widget-compiler/src/types.ts';
import type { BehaviorSignal, BehaviorOutput } from '../../../../packages/widget-runtime/src/behavior.ts';

export type Selection = { source: WidgetSource; selected: string };
const copy = (source:WidgetSource) => structuredClone(source);
function unique(source:WidgetSource,prefix:string) {
  const used=new Set([...source.behavior.signals,...source.parameters,...source.design?.nodes??[],...source.inputs,...source.outputs,...source.behavior.outputs??[]].map(item=>item.id));
  let suffix=1;while(used.has(`${prefix}-${suffix}`))suffix++;return `${prefix}-${suffix}`;
}
export function domain(signal:BehaviorSignal|BehaviorOutput):SignalDomain {
  return signal.type==='number'?{type:'number',min:signal.min,max:signal.max}:signal.type==='enum'?{type:'enum',values:[...signal.values]}:{type:'boolean'};
}
export function sampling(signal:BehaviorSignal):VisualDomain {
  return signal.type==='number'?{signal:signal.id,min:signal.min,max:signal.max,samples:11,maxErrorPixels:1,travelPixels:100}:{signal:signal.id,values:signal.type==='boolean'?[false,true]:[...signal.values]};
}
export function addSignal(source:WidgetSource,kind:'local'|'input'|'feedback'):Selection {
  const next=copy(source),id=unique(next,kind);next.behavior.signals.push({id,source:kind,type:'boolean',initial:false,...kind==='feedback'?{freshnessMs:1000}:{}});return {source:next,selected:id};
}
export function addPort(source:WidgetSource,direction:'inputs'|'outputs'):Selection {
  let next=copy(source);const kind=direction==='inputs'?'local':'feedback';
  if(!next.behavior.signals.some(s=>s.source===kind))next=addSignal(next,kind).source;
  const signal=next.behavior.signals.find(s=>s.source===kind)!,id=unique(next,direction==='inputs'?'to-profile':'from-profile');
  next[direction].push({id,label:direction==='inputs'?'To profile':'From profile',signal:signal.id,...domain(signal)});
  if(direction==='inputs')(next.behavior.outputs??=[]).push({id,...domain(signal),neutral:signal.initial});
  return {source:next,selected:`${direction==='inputs'?'output':'feedback'}:${id}`};
}
export function addOutputBinding(source:WidgetSource):Selection {
  const next=copy(source),id=unique(next,'binding');(next.behavior.outputs??=[]).push({id,type:'boolean',neutral:false});return {source:next,selected:`output:${id}`};
}
/** Profile declarations share their signal domain and reset value by definition. */
export function syncSignal(source:WidgetSource,id:string):void {
  const signal=source.behavior.signals.find(s=>s.id===id);if(!signal)return;
  for(const direction of ['inputs','outputs'] as const)for(const port of source[direction])if(port.signal===id){
    delete port.min;delete port.max;delete port.values;Object.assign(port,domain(signal));
    if(direction==='inputs'){
      const outputs=source.behavior.outputs??=[],index=outputs.findIndex(o=>o.id===port.id),output={id:port.id,...domain(signal),neutral:signal.initial};
      if(index<0)outputs.push(output);else outputs[index]=output;
    }
  }
  for(const owner of source.design?.owners??[])if(owner.signal===id){const region=source.behavior.regions.find(r=>r.id===owner.region);if(region)owner.values[region.initial]=signal.initial;}
}
export function addVisualDomain(source:WidgetSource):Selection {
  let next=copy(source);if(!next.behavior.signals.some(s=>!next.visualDomains.some(d=>d.signal===s.id)))next=addSignal(next,'local').source;
  const signal=next.behavior.signals.find(s=>!next.visualDomains.some(d=>d.signal===s.id))!;next.visualDomains.push(sampling(signal));return {source:next,selected:`domain:${signal.id}`};
}
export function recognizesSelection(source:WidgetSource,selected:string):boolean {
  return source.behavior.signals.some(s=>s.id===selected)||source.inputs.some(p=>`output:${p.id}`===selected)||(source.behavior.outputs??[]).some(p=>`output:${p.id}`===selected)||source.outputs.some(p=>`feedback:${p.id}`===selected)||source.visualDomains.some(d=>`domain:${d.signal}`===selected);
}
function validId(value:string,ids:string[],previous:string) {
  if(!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(value)||value!==previous&&ids.includes(value))throw new Error('Use a unique ID with letters, numbers, underscores or hyphens.');
}
function walk(value:unknown,visit:(item:Record<string,unknown>)=>void) {
  if(!value||typeof value!=='object')return;
  if(Array.isArray(value)){value.forEach(item=>walk(item,visit));return;}
  const record=value as Record<string,unknown>;visit(record);Object.values(record).forEach(item=>walk(item,visit));
}
export function renameSignal(source:WidgetSource,id:string,value:string):Selection {
  validId(value,[...source.behavior.signals,...source.parameters,...source.design?.nodes??[]].map(s=>s.id),id);
  const next=copy(source),signal=next.behavior.signals.find(s=>s.id===id);if(!signal)throw new Error('Signal no longer exists.');signal.id=value;
  // An invalid duplicate can be repaired without stealing the remaining declaration's references.
  if(next.behavior.signals.some(s=>s.id===id))return {source:next,selected:value};
  walk(next,item=>{for(const key of ['ref','signal','qualitySignal'])if(item[key]===id)item[key]=value;});
  for(const event of next.behavior.events)if(event.updates)event.updates=event.updates.map(update=>update===id?value:update);
  for(const motion of next.motions??[])for(const selector of [motion.from,motion.to])if(Object.hasOwn(selector,id)){selector[value]=selector[id];delete selector[id];}
  return {source:next,selected:value};
}
export function removeSignal(source:WidgetSource,id:string):Selection {
  let used=false;walk(source,item=>{if(['ref','signal','qualitySignal'].some(key=>item[key]===id))used=true;});
  if(used||source.behavior.events.some(e=>e.updates?.includes(id))||source.motions?.some(m=>Object.hasOwn(m.from,id)||Object.hasOwn(m.to,id)))throw new Error('Disconnect references, ports, sampling and state ownership before deleting this signal.');
  const next=copy(source),index=next.behavior.signals.findIndex(s=>s.id===id);if(index>=0)next.behavior.signals.splice(index,1);return {source:next,selected:''};
}
export function renamePort(source:WidgetSource,direction:'inputs'|'outputs',id:string,value:string):Selection {
  validId(value,[...source[direction],...direction==='inputs'?source.behavior.outputs??[]:[]].map(p=>p.id),id);
  const next=copy(source),port=next[direction].find(p=>p.id===id);if(port)port.id=value;
  if(direction==='inputs'){
    const output=next.behavior.outputs?.find(p=>p.id===id);if(output)output.id=value;
    walk(next.behavior,item=>{if(item.type==='emit'&&item.port===id)item.port=value;});
    for(const owner of next.design?.owners??[])if(owner.port===id)owner.port=value;
  }
  return {source:next,selected:`${direction==='inputs'?'output':'feedback'}:${value}`};
}
export function removePort(source:WidgetSource,direction:'inputs'|'outputs',id:string):Selection {
  if(direction==='inputs'){
    let used=false;walk(source.behavior,item=>{if(item.type==='emit'&&item.port===id)used=true;});
    if(used||source.design?.owners.some(o=>o.port===id))throw new Error('Disconnect emit actions and state ownership before deleting this endpoint.');
  }
  const next=copy(source);next[direction]=next[direction].filter(p=>p.id!==id);if(direction==='inputs'&&next.behavior.outputs)next.behavior.outputs=next.behavior.outputs.filter(p=>p.id!==id);
  return {source:next,selected:''};
}
export function associatePort(source:WidgetSource,direction:'inputs'|'outputs',id:string,signalId:string):WidgetSource {
  const next=copy(source),port=next[direction].find(p=>p.id===id),signal=next.behavior.signals.find(s=>s.id===signalId);
  if(!port||!signal)throw new Error('Choose an existing signal.');
  if(direction==='outputs'&&signal.source!=='feedback')throw new Error('From profile requires a feedback signal.');
  if(direction==='inputs'&&signal.source!=='local')throw new Error('To profile requires a local signal.');
  if(direction==='inputs'&&next.design?.owners.some(o=>o.port===id&&o.signal!==signalId))throw new Error('Change the state owner before changing its endpoint signal.');
  next[direction][next[direction].indexOf(port)]={id:port.id,label:port.label,signal:signalId,...domain(signal)};
  if(direction==='inputs'){
    const outputs=next.behavior.outputs??=[];const index=outputs.findIndex(o=>o.id===id),output={id,...domain(signal),neutral:signal.initial};
    if(index<0)outputs.push(output);else outputs[index]=output;
  }
  return next;
}
export function changeSignalType(source:WidgetSource,id:string,type:BehaviorSignal['type']):WidgetSource {
  if(source.design?.owners.some(o=>o.signal===id))throw new Error('Change state ownership before changing this signal type.');
  const next=copy(source),index=next.behavior.signals.findIndex(s=>s.id===id),old=next.behavior.signals[index];if(!old)return next;
  const shape=type==='number'?{type,min:0,max:100,initial:0}:type==='enum'?{type,values:['off','on'],initial:'off'}:{type,initial:false};
  const feedback=old.source==='feedback'?Object.fromEntries(Object.entries(old).filter(([key,value])=>['freshnessMs','qualitySignal','requireGameConfirmed'].includes(key)&&value!==undefined)):{};
  next.behavior.signals[index]={id,source:old.source,...shape,...feedback} as BehaviorSignal;
  syncSignal(next,id);
  next.visualDomains=next.visualDomains.map(d=>d.signal===id?sampling(next.behavior.signals[index]):d);return next;
}
