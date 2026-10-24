import {designLayers,expressionRefs} from '../../../packages/widget-compiler/src/index.ts';
import type {WidgetSource} from '../../../packages/widget-compiler/src/types.ts';
import type {Expression} from '../../../packages/widget-runtime/src/behavior.ts';
export interface GraphCard {id:string;label:string;kind:string;x:number;y:number;expression?:Expression;state?:boolean;event?:boolean}
export interface GraphWire {id:string;from:string;to:string;label:string;transition:string;pin?:string;fromPin?:string;event?:boolean}
export function graphPositions(cards:GraphCard[],saved:Record<string,{x:number;y:number}>,sizes:Record<string,{width:number;height:number}>={}):Record<string,{x:number;y:number}>{
  const positions:Record<string,{x:number;y:number}>={},boxes:{x:number;y:number;width:number;height:number}[]=[];
  function dimensions(card:GraphCard){return sizes[card.id]??{width:210,height:card.state?225:card.id.startsWith('hit:')?170:110};}
  // Reserve every explicit placement before considering new cards, even when a
  // saved card occurs later in the list. Saved user layouts remain untouched.
  for(const card of cards)if(Object.hasOwn(saved,card.id)){
    positions[card.id]={...saved[card.id]};boxes.push({...positions[card.id],...dimensions(card)});
  }
  for(const card of cards)if(!Object.hasOwn(positions,card.id)){
    const size=dimensions(card),position={x:card.x,y:card.y};
    for(;;){
      const collisions=boxes.filter(box=>position.x<box.x+box.width+20&&position.x+size.width+20>box.x&&position.y<box.y+box.height+20&&position.y+size.height+20>box.y);
      if(!collisions.length)break;
      position.y=Math.max(...collisions.map(box=>box.y+box.height+20));
    }
    positions[card.id]=position;boxes.push({...position,...size});
  }
  return positions;
}
export const inputPins=(expression:Expression):string[]=>'op'in expression?'values'in expression?expression.values.map((_,i)=>'values.'+i):Object.keys(expression).filter(k=>k!=='op'):['value'];
export function pinExpression(expression:Expression,pin:string):Expression {
  if(!('op'in expression))return expression;
  if(pin.startsWith('values.')&&'values'in expression)return expression.values[Number(pin.split('.')[1])];
  return (expression as unknown as Record<string,Expression>)[pin];
}
export function graphCards(source:WidgetSource,mode:'States'|'Signals',regionId:string):GraphCard[]{
  const events=source.behavior.events.map((e,i)=>({id:'event:'+e.id,label:e.id,kind:'Event',x:40,y:240+i*130,event:true}));
  if(mode==='States')return [
    ...source.behavior.regions.find(r=>r.id===regionId)?.states.map((id,i)=>({id,label:id,kind:source.behavior.regions.find(r=>r.id===regionId)?.initial===id?'Initial state':'State',x:650+i*280,y:90,state:true}))??[],
    {id:'any:'+regionId,label:'Any state',kind:'Global transition source',x:650,y:370,event:true},
    ...source.hitRegions.map((h,i)=>({id:'hit:'+h.id,label:h.id,kind:'Touch area',x:40,y:40+i*210,event:true})),
    ...events.map((e,i)=>({...e,x:340,y:40+i*130})),
    ...(source.behavior.timers??[]).map((t,i)=>({id:'timer:'+t.id,label:t.id,kind:`Timer · ${t.delayMs} ms${t.repeat?' · repeat':''}`,x:40,y:220+source.hitRegions.length*210+i*130,event:true})),
  ];
  return [
    ...source.behavior.signals.map((s,i)=>({id:s.id,label:s.id,kind:(s.source==='feedback'?'Feedback':s.source==='input'?'External input':'Local state')+' · '+s.type,x:40,y:40+i*125})),
    ...(source.design?.nodes??[]).map((n,i)=>({id:n.id,label:n.label,kind:'Transform · '+n.domain.type,x:350,y:60+i*150,expression:n.expression})),
    ...source.inputs.map((p,i)=>({id:'output:'+p.id,label:p.label,kind:'To profile · '+p.type,x:680,y:50+i*140,expression:{ref:p.signal} as Expression})),
    ...(source.behavior.outputs??[]).filter(o=>!source.inputs.some(p=>p.id===o.id)).map((o,i)=>({id:'output:'+o.id,label:o.id,kind:'Output binding · '+o.type,x:680,y:50+(source.inputs.length+i)*140})),
    ...source.outputs.map((p,i)=>({id:'feedback:'+p.id,label:p.label,kind:'From profile · '+p.type,x:40,y:60+(source.behavior.signals.length+i)*130})),
    ...source.visualDomains.map((d,i)=>({id:'domain:'+d.signal,label:d.signal+' rendering',kind:'Rendering samples',x:1000,y:40+i*130})),
    ...source.behavior.regions.map((r,i)=>({id:'state:'+r.id,label:r.id,kind:'State owner reference',x:680,y:250+(source.inputs.length+i)*140})),
    ...designLayers(source.scene).map((n,i)=>({id:'layer:'+n.id,label:n.label??n.id,kind:'Presentation',x:1300,y:40+i*115})),
  ];
}
export function graphWires(source:WidgetSource,mode:'States'|'Signals',regionId:string,cards:GraphCard[]):GraphWire[]{
  if(mode==='States')return [
    ...(source.behavior.regions.find(r=>r.id===regionId)?.transitions??[]).map(t=>({id:'transition:'+t.id,from:t.from==='*'?'any:'+regionId:t.from,to:t.to??(t.from==='*'?'any:'+regionId:t.from),label:`${t.event}${t.guard?' · condition':''}${t.to?'':' · Stay'}`,transition:t.id,event:true})),
    ...source.hitRegions.flatMap(h=>Object.entries(h.events).map(([phase,event])=>({id:`hit:${h.id}:${phase}`,from:'hit:'+h.id,fromPin:phase,to:'event:'+event,label:phase,transition:'',event:true}))),
    ...(source.behavior.timers??[]).map(t=>({id:'timer:'+t.id,from:'timer:'+t.id,to:'event:'+t.event,label:'elapsed',transition:'',event:true})),
    ...(source.behavior.regions.find(r=>r.id===regionId)?.transitions??[]).map(t=>({id:'trigger:'+t.id,from:'event:'+t.event,to:t.from==='*'?'any:'+regionId:t.from,label:'triggers',transition:t.id,event:true})),
  ];
  const references=new Set([...source.behavior.signals.map(s=>s.id),...(source.design?.nodes.map(n=>n.id)??[])]);
  return [
    ...cards.flatMap(card=>card.expression?inputPins(card.expression).flatMap(pin=>{const refs=expressionRefs(pinExpression(card.expression!,pin));return [...refs.refs,...[...refs.regions].map(id=>'state:'+id)].map(ref=>({id:ref+card.id+pin,from:ref,to:card.id,pin,label:'value',transition:''}));}):[]),
    ...source.outputs.map(p=>({id:'feedback:'+p.id,from:'feedback:'+p.id,to:p.signal,label:'feedback',transition:''})),
    ...source.visualDomains.map(d=>({id:'domain:'+d.signal,from:d.signal,to:'domain:'+d.signal,label:'samples',transition:''})),
    ...(source.design?.owners??[]).map(o=>({id:o.region+o.signal,from:'state:'+o.region,to:o.signal,label:'owns',transition:''})),
    ...designLayers(source.scene).flatMap(n=>[...expressionRefs(n).refs].filter(id=>references.has(id)).map(id=>({id:id+n.id,from:id,to:'layer:'+n.id,label:'appearance',transition:''}))),
    ...(source.design?.appearances??[]).flatMap(a=>Object.keys(a.layers).flatMap(layer=>Object.keys(a.regions).map(r=>({id:a.id+layer+r,from:'state:'+r,to:'layer:'+layer,label:a.id,transition:''})))),
  ];
}
