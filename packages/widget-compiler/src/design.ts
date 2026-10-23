import { evaluateExpression, validateBehaviorDefinition, type Expression, type Scalar } from '../../widget-runtime/src/behavior.ts';
import { canonicalWidget } from '../../widget-runtime/src/artifact.ts';
import { assertSource, exactSource, sourceId, sourceNumber } from './source.ts';
import type { WidgetSource, VisualNode, VisualValue, SignalNode, StateAppearance, WidgetDesign } from './types.ts';

const properties = /^(visible|opacity|fill|stroke|strokeWidth|x|y|x2|y2|width|height|radius|text|d|fontSize|transform\.(translateX|translateY|rotate|scaleX|scaleY|originX|originY))$/;
const numeric = /^(x|y|x2|y2|width|height|radius|opacity|strokeWidth|fontSize|transform\.(translateX|translateY|rotate|scaleX|scaleY|originX|originY))$/;
const expr = (value:VisualValue):Expression => typeof value==='object'?value:{literal:value};
function list<T>(value:T[],limit:number):T[]{assertSource(Array.isArray(value)&&value.length<=limit,'Design list exceeds bounds');return value;}
export function designLayers(scene:VisualNode[]):VisualNode[]{return scene.flatMap(node=>[node,...(node.kind==='group'?designLayers(node.children):[])]);}
export function layerProperty(node:VisualNode,property:string):VisualValue {
  const [key,part]=property.split('.'),value=(node as unknown as Record<string,any>)[key];
  return (part?value?.[part]:value)??(property==='visible'?true:property==='opacity'||part?.startsWith('scale')?1:property==='fill'||property==='stroke'?'none':0);
}
export function setLayerProperty(node:VisualNode,property:string,value:VisualValue):void {
  const [key,part]=property.split('.'),record=node as unknown as Record<string,any>;
  if(part){record[key]??={};record[key][part]=value;}else record[key]=value;
}
function applicable(node:VisualNode,property:string):boolean{return properties.test(property)&&(property.startsWith('transform.')||['visible','opacity','fill','stroke','strokeWidth'].includes(property)||property in node||property==='radius'&&node.kind==='rectangle');}
function condition(regions:Record<string,string>):Expression {
  const values=Object.entries(regions).sort(([a],[b])=>a.localeCompare(b)).map(([state,value])=>({op:'eq',left:{state},right:{literal:value}} as Expression));
  return values.length===1?values[0]:{op:'and',values};
}
function includes(a:Record<string,string>,b:Record<string,string>):boolean{return Object.entries(b).every(([key,value])=>a[key]===value);}
/** Canonical source keeps authoring nodes; this pass is the only graph compiler. */
export function lowerWidgetDesign(source:WidgetSource,parameters:Record<string,Scalar>={}):WidgetSource {
  if(!source.design)return structuredClone(source);
  const design=exactSource(source.design,['nodes','owners','appearances','effects']) as WidgetDesign;
  const result=structuredClone(source);delete result.design;
  const nodes=new Map<string,SignalNode>(),expanded=new Map<string,Expression>(),visiting=new Set<string>();
  const reserved=new Set([...source.parameters.map(p=>p.id),...source.behavior.signals.map(s=>s.id)]);
  for(const node of list(design.nodes,64)){
    exactSource(node,['id','label','domain','expression']);sourceId(node.id);assertSource(typeof node.label==='string'&&node.label.length<=120,'Invalid graph label');
    assertSource(!nodes.has(node.id)&&!reserved.has(node.id),'Duplicate graph value ID: '+node.id);nodes.set(node.id,node);
    exactSource(node.domain,['type'],node.domain.type==='number'?['min','max']:node.domain.type==='enum'?['values']:[]);
  }
  let expansionWork=0;
  function expand(value:any,depth=0):any {
    assertSource(depth<32&&++expansionWork<=32768,'Graph expansion exceeds work limit');
    if(!value||typeof value!=='object')return value;
    if(Array.isArray(value))return value.map(item=>expand(item,depth+1));
    if(Object.keys(value).length===1&&typeof value.ref==='string'&&nodes.has(value.ref))return resolve(value.ref);
    return Object.fromEntries(Object.entries(value).map(([key,item])=>[key,expand(item,depth+1)]));
  }
  function resolve(id:string):Expression {
    if(expanded.has(id))return expanded.get(id)!;
    assertSource(!visiting.has(id),'Signal graph contains a cycle at '+id);visiting.add(id);
    const node=nodes.get(id)!,expression=expand(node.expression) as Expression;
    // Reuse the bounded runtime type/range checker even for unused graph nodes.
    const probe=structuredClone(source.behavior);probe.regions=[];probe.events=[{id:'probe'}];probe.outputs=[{id:'probe',...node.domain,neutral:node.domain.type==='boolean'?false:node.domain.type==='number'?node.domain.min:node.domain.values[0]}];
    probe.regions=source.behavior.regions.map(r=>({...r,transitions:[]}));if(!probe.regions.length)probe.regions.push({id:'graph-probe',initial:'ready',states:['ready'],transitions:[]});probe.regions[0].transitions=[{id:'probe',from:probe.regions[0].initial,event:'probe',actions:[{type:'emit',port:'probe',value:expression}]}];delete probe.timers;
    const checked=validateBehaviorDefinition(probe);assertSource(checked.ok,'Graph '+id+': '+(checked.ok?'':checked.error));
    visiting.delete(id);expanded.set(id,expression);return expression;
  }
  for(const id of nodes.keys())resolve(id);
  result.behavior=expand(result.behavior);result.scene=expand(result.scene);
  const ownerSignals=new Set<string>(),ownerRegions=new Set<string>(),ownerPorts=new Set<string>();
  for(const owner of list(design.owners,8)){
    exactSource(owner,['region','signal','values'],['port']);
    const region=result.behavior.regions.find(r=>r.id===owner.region),signal=result.behavior.signals.find(s=>s.id===owner.signal);
    assertSource(region&&signal?.source==='local'&&!ownerSignals.has(owner.signal)&&!ownerRegions.has(owner.region),'Invalid or duplicate state owner');
    ownerSignals.add(owner.signal);ownerRegions.add(owner.region);exactSource(owner.values,region.states);
    for(const value of Object.values(owner.values))assertSource(signal.type==='boolean'?typeof value==='boolean':signal.type==='number'?typeof value==='number'&&value>=signal.min&&value<=signal.max:typeof value==='string'&&signal.values.includes(value),'Owner value outside domain');
    assertSource(canonicalWidget(owner.values[region.initial])===canonicalWidget(signal.initial),'Owned signal initial must match initial state');
    for(const other of result.behavior.regions)for(const transition of other.transitions)for(const action of transition.actions??[])assertSource(!(action.type==='set'&&action.signal===owner.signal)&&!(owner.port&&action.type==='emit'&&action.port===owner.port),'State owner has a second writer');
    if(owner.port){assertSource(!ownerPorts.has(owner.port)&&result.inputs.some(p=>p.id===owner.port&&p.signal===owner.signal),'State owner output must match published port');ownerPorts.add(owner.port);}
    for(const transition of region.transitions)if(transition.to){
      const value={literal:owner.values[transition.to]};transition.actions??=[];transition.actions.push({type:'set',signal:owner.signal,value});
      if(owner.port)transition.actions.push({type:'emit',port:owner.port,value});
    }
    // Visuals observe the region directly, avoiding impossible mirror/state combinations.
    const ownerExpression=region.states.slice(0,-1).reduceRight<Expression>((otherwise,state)=>({op:'if',condition:condition({[region.id]:state}),then:{literal:owner.values[state]},else:otherwise}),{literal:owner.values[region.states.at(-1)!]});
    function replace(value:any):any{if(!value||typeof value!=='object')return value;if(Array.isArray(value))return value.map(replace);if(value.ref===owner.signal&&Object.keys(value).length===1)return ownerExpression;return Object.fromEntries(Object.entries(value).map(([k,v])=>[k,replace(v)]));}
    result.scene=replace(result.scene);
  }
  const layers=new Map(designLayers(result.scene).map(n=>[n.id,n])),appearanceIds=new Set<string>();
  const appearances=[...list(design.appearances,64)].sort((a,b)=>Object.keys(a.regions).length-Object.keys(b.regions).length||a.id.localeCompare(b.id));
  for(const appearance of appearances){
    exactSource(appearance,['id','regions','layers']);sourceId(appearance.id);assertSource(!appearanceIds.has(appearance.id),'Duplicate appearance ID');appearanceIds.add(appearance.id);
    assertSource(Object.keys(appearance.regions).length>0,'Appearance needs a state selector');
    for(const [id,state]of Object.entries(appearance.regions))assertSource(source.behavior.regions.some(r=>r.id===id&&r.states.includes(state)),'Unknown appearance state');
    assertSource(Object.keys(appearance.layers).length<=128,'Too many appearance layers');
    for(const [id,patch]of Object.entries(appearance.layers)){
      const node=layers.get(id);assertSource(node,'Appearance refers to missing layer: '+id);
      for(const [property,value]of Object.entries(patch)){
        assertSource(applicable(node,property),'Unsupported appearance property: '+property);
        for(const other of appearances){if(other===appearance||other.layers[id]?.[property]===undefined)continue;
          const overlap=Object.entries(appearance.regions).every(([k,v])=>other.regions[k]===undefined||other.regions[k]===v);
          if(overlap&&!includes(appearance.regions,other.regions)&&!includes(other.regions,appearance.regions))assertSource(appearances.some(combo=>includes(combo.regions,{...appearance.regions,...other.regions})&&combo.layers[id]?.[property]!==undefined),'Conflicting appearances require a combination override: '+id+'.'+property);
          assertSource(!overlap||canonicalWidget(appearance.regions)!==canonicalWidget(other.regions),'Duplicate appearance selector writes '+id+'.'+property);
        }
        setLayerProperty(node,property,{op:'if',condition:condition(appearance.regions),then:expand(expr(value)),else:expr(layerProperty(node,property))});
      }
    }
  }
  const effectIds=new Set<string>();
  for(const effect of list(design.effects,32)){
    exactSource(effect,['region','transition','durationMs','fps','properties'],['easing','interruption','cue']);
    const region=result.behavior.regions.find(r=>r.id===effect.region),transition=region?.transitions.find(t=>t.id===effect.transition),id=effect.region+'-'+effect.transition;
    assertSource(region&&transition?.to&&transition.from!=='*'&&!effectIds.has(id),'Effect needs a unique explicit transition');effectIds.add(id);
    sourceNumber(effect.durationMs,1,60000,true);sourceNumber(effect.fps,1,60,true);
    const values={...Object.fromEntries(result.parameters.map(p=>[p.id,p.default])),...parameters,...Object.fromEntries(result.behavior.signals.map(s=>[s.id,s.initial]))};
    const regions=Object.fromEntries(result.behavior.regions.map(r=>[r.id,r.initial]));
    assertSource([undefined,'linear','ease-in','ease-out','ease-in-out'].includes(effect.easing)&&[undefined,'reverse','restart','snap'].includes(effect.interruption),'Invalid transition effect playback rule');
    const trackIds=new Set<string>();
    const tracks=list(effect.properties,64).map(track=>{
      exactSource(track,['node','property']);const node=layers.get(track.node);assertSource(node&&numeric.test(track.property)&&applicable(node,track.property),'Invalid automatic motion property');
      const trackId=track.node+'.'+track.property;assertSource(!trackIds.has(trackId),'Duplicate automatic motion property');trackIds.add(trackId);
      assertSource(!(result.motions??[]).some(m=>m.tracks.some(t=>t.node===track.node&&t.property===track.property)&&(m.fromRegions?.[region.id]===undefined||m.fromRegions[region.id]===transition.from)&&(m.toRegions?.[region.id]===undefined||m.toRegions[region.id]===transition.to)),'Automatic and explicit motion tracks conflict: '+trackId);
      const value=expr(layerProperty(node,track.property));
      function checkDependency(v:any):void{if(v&&typeof v==='object'){if(v.ref)assertSource(result.parameters.some(p=>p.id===v.ref),'Automatic motion endpoints depend on a live signal; use explicit tracks');if(v.state)assertSource(v.state===effect.region,'Automatic motion endpoints depend on another region; use explicit tracks');for(const child of Object.values(v))checkDependency(child);}}checkDependency(value);
      const from=evaluateExpression(value,{values,regions:{...regions,[region.id]:transition.from}}),to=evaluateExpression(value,{values,regions:{...regions,[region.id]:transition.to!}});
      assertSource(typeof from==='number'&&typeof to==='number','Automatic motion requires numeric endpoints');
      return {...track,keyframes:[{at:0,value:from},{at:1,value:to}],...(effect.easing?{easing:effect.easing}:{})};
    });
    if(tracks.length){result.motions??=[];assertSource(!result.motions.some(m=>m.id===id),'Duplicate motion effect');result.motions.push({id,from:{},to:{},fromRegions:{[region.id]:transition.from},toRegions:{[region.id]:transition.to},durationMs:effect.durationMs,fps:effect.fps,tracks,...(effect.interruption?{interruption:effect.interruption}:{})});}
    if(effect.cue){sourceId(effect.cue);transition.actions??=[];transition.actions.push({type:'cue',cue:effect.cue});}
  }
  return result;
}
/** Apply one editing context without flattening or evaluating advanced bindings. */
export function appearanceScene(source:WidgetSource,appearanceId:string):VisualNode[]{
  const scene=structuredClone(source.scene),appearance=source.design?.appearances.find(a=>a.id===appearanceId);
  if(appearance){
    const inherited=(source.design?.appearances??[]).filter(a=>includes(appearance.regions,a.regions)).sort((a,b)=>Object.keys(a.regions).length-Object.keys(b.regions).length||a.id.localeCompare(b.id));
    for(const context of inherited)for(const node of designLayers(scene))for(const [property,value]of Object.entries(context.layers[node.id]??{}))setLayerProperty(node,property,structuredClone(value));
  }
  return scene;
}
export function patchAppearance(source:WidgetSource,appearanceId:string,nodeId:string,patch:Record<string,unknown>):void {
  const appearance=source.design?.appearances.find(a=>a.id===appearanceId),node=designLayers(source.scene).find(n=>n.id===nodeId);assertSource(node,'Missing layer');
  for(const [key,value]of Object.entries(patch)){
    if(key==='transform'&&value&&typeof value==='object'){for(const [part,item]of Object.entries(value))patchAppearance(source,appearanceId,nodeId,{['transform.'+part]:item});continue;}
    if(!appearance||!properties.test(key)){(node as unknown as Record<string,unknown>)[key]=value;continue;}
    assertSource(applicable(node,key),'Unsupported appearance property');appearance.layers[nodeId]??={};appearance.layers[nodeId][key]=value as VisualValue;
  }
}
export function emptyDesign():WidgetDesign{return {nodes:[],owners:[],appearances:[],effects:[]};}
/** Authoring probes only; the player continues to execute compiled behavior. */
export function designValues(source:WidgetSource,context:{values:Record<string,Scalar>;regions:Record<string,string>}):Record<string,Scalar>{
  const values={...context.values},pending=[...(source.design?.nodes??[])];
  for(let pass=0;pending.length&&pass<64;pass++)for(let i=pending.length-1;i>=0;i--){try{values[pending[i].id]=evaluateExpression(pending[i].expression,{values,regions:context.regions});pending.splice(i,1);}catch{/* An upstream value may be resolved in the next pass. */}}
  return values;
}
/** Topology is global. Remove references in every appearance and motion together. */
export function removeDesignLayer(source:WidgetSource,id:string):void{
  const found=designLayers(source.scene).find(n=>n.id===id);if(!found)return;
  const removed=new Set(designLayers([found]).map(n=>n.id));
  function remove(nodes:VisualNode[]):void{const index=nodes.findIndex(n=>n.id===id);if(index>=0)nodes.splice(index,1);else for(const node of nodes)if(node.kind==='group')remove(node.children);}remove(source.scene);
  for(const appearance of source.design?.appearances??[])for(const key of removed)delete appearance.layers[key];
  for(const effect of source.design?.effects??[])effect.properties=effect.properties.filter(p=>!removed.has(p.node));
  for(const motion of source.motions??[])motion.tracks=motion.tracks.filter(t=>!removed.has(t.node));
}
export function duplicateDesignLayer(source:WidgetSource,id:string,allocate:()=>string):string|undefined{
  const original=designLayers(source.scene).find(n=>n.id===id);if(!original)return;
  const copy=structuredClone(original),ids=new Map<string,string>();
  for(const node of designLayers([copy])){const old=node.id;node.id=allocate();ids.set(old,node.id);}copy.label=(copy.label??copy.kind)+' copy';
  function insert(nodes:VisualNode[]):boolean{const index=nodes.findIndex(n=>n.id===id);if(index>=0){nodes.splice(index+1,0,copy);return true;}return nodes.some(n=>n.kind==='group'&&insert(n.children));}insert(source.scene);
  for(const appearance of source.design?.appearances??[])for(const [old,id]of ids)if(appearance.layers[old])appearance.layers[id]=structuredClone(appearance.layers[old]);
  for(const effect of source.design?.effects??[])effect.properties.push(...effect.properties.filter(p=>ids.has(p.node)).map(p=>({...p,node:ids.get(p.node)!})));
  for(const motion of source.motions??[])motion.tracks.push(...motion.tracks.filter(t=>ids.has(t.node)).map(t=>({...structuredClone(t),node:ids.get(t.node)!})));
  return copy.id;
}
