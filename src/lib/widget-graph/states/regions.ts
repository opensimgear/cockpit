import type {WidgetSource} from '../../../../packages/widget-compiler/src/types.ts';

function checkId(value:string,ids:string[],previous:string){
  if(!/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(value))throw new Error('Use an ID starting with a letter, followed by up to 63 letters, numbers, underscores or hyphens.');
  if(value!==previous&&ids.includes(value))throw new Error(`The ID “${value}” is already in use.`);
}
function regionIn(source:WidgetSource,id:string){
  const region=source.behavior.regions.find(r=>r.id===id);
  if(!region)throw new Error(`State machine “${id}” does not exist.`);
  return region;
}
function expressions(value:unknown,visit:(expression:{state:string})=>void):void{
  if(!value||typeof value!=='object')return;
  if(!Array.isArray(value)&&Object.keys(value).length===1&&'state' in value&&typeof value.state==='string')visit(value as {state:string});
  else for(const child of Object.values(value))expressions(child,visit);
}
function renameKey<T>(record:Record<string,T>|undefined,previous:string,next:string){
  if(record&&Object.hasOwn(record,previous)&&previous!==next){record[next]=record[previous];delete record[previous];}
}
function renameStateComparisons(value:unknown,regionId:string,previous:string,name:string):void{
  if(!value||typeof value!=='object')return;
  const item=value as Record<string,unknown>;
  const readsState=(expression:any):boolean=>!!expression&&typeof expression==='object'&&(expression.state===regionId||expression.op==='if'&&(readsState(expression.then)||readsState(expression.else)));
  function stateValue(expression:any):void{
    if(!expression||typeof expression!=='object')return;
    if(expression.literal===previous)expression.literal=name;
    else if(expression.op==='if'){stateValue(expression.then);stateValue(expression.else);}
  }
  if(item.op==='eq'||item.op==='ne'){
    if(readsState(item.left)){stateValue(item.left);stateValue(item.right);}
    if(readsState(item.right)){stateValue(item.left);stateValue(item.right);}
  }
  for(const child of Object.values(value))renameStateComparisons(child,regionId,previous,name);
}

/** Rename the authoring identity and every reference without changing behavior. */
export function renameRegion(source:WidgetSource,previous:string,name:string):WidgetSource{
  const next=structuredClone(source),region=regionIn(next,previous);
  checkId(name,next.behavior.regions.map(r=>r.id),previous);region.id=name;
  for(const owner of next.design?.owners??[])if(owner.region===previous)owner.region=name;
  for(const appearance of next.design?.appearances??[])renameKey(appearance.regions,previous,name);
  for(const effect of next.design?.effects??[])if(effect.region===previous)effect.region=name;
  for(const motion of next.motions??[]){renameKey(motion.fromRegions,previous,name);renameKey(motion.toRegions,previous,name);}
  expressions(next,expression=>{if(expression.state===previous)expression.state=name;});
  return next;
}

/** Remove owned metadata, but leave callers responsible for external expressions. */
export function removeRegion(source:WidgetSource,id:string):WidgetSource{
  const next=structuredClone(source);regionIn(next,id);
  next.behavior.regions=next.behavior.regions.filter(r=>r.id!==id);
  if(next.design){
    next.design.owners=next.design.owners.filter(o=>o.region!==id);
    next.design.appearances=next.design.appearances.filter(a=>!Object.hasOwn(a.regions,id));
    next.design.effects=next.design.effects.filter(e=>e.region!==id);
  }
  // Motion selectors describe endpoint pairs. Removing either endpoint invalidates
  // the baked motion, so remove that complete motion rather than broaden it.
  if(next.motions)next.motions=next.motions.filter(m=>!Object.hasOwn(m.fromRegions??{},id)&&!Object.hasOwn(m.toRegions??{},id));
  expressions(next,expression=>{if(expression.state===id)throw new Error(`State machine “${id}” is still used by an expression. Remove or replace its external state references first.`);});
  return next;
}

export function renameState(source:WidgetSource,regionId:string,previous:string,name:string):WidgetSource{
  const next=structuredClone(source),region=regionIn(next,regionId);
  if(!region.states.includes(previous))throw new Error(`State “${previous}” does not exist in “${regionId}”.`);
  checkId(name,region.states,previous);
  // A state-valued graph node carries its own enum and can feed comparisons
  // through arbitrary ref chains. Changing only direct literals would leave
  // those declarations and consumers stale, so require an explicit repair.
  const nodes=new Map((next.design?.nodes??[]).map(node=>[node.id,node]));
  function returnsState(value:unknown,seen=new Set<string>()):boolean{
    if(!value||typeof value!=='object')return false;
    const expression=value as Record<string,unknown>;
    if(expression.state===regionId)return true;
    if(expression.op==='if')return returnsState(expression.then,seen)||returnsState(expression.else,seen);
    if(typeof expression.ref==='string'&&!seen.has(expression.ref)){
      const visited=new Set(seen);visited.add(expression.ref);
      return returnsState(nodes.get(expression.ref)?.expression,visited);
    }
    return false;
  }
  if(previous!==name){
    const aliases=[...nodes.values()].filter(node=>node.domain?.type==='enum'&&returnsState(node.expression));
    if(aliases.length)throw new Error(`Update or remove the state value alias${aliases.length===1?'':'es'} ${aliases.map(node=>`“${node.id}”`).join(', ')} and its references before renaming this state.`);
  }
  region.states=region.states.map(state=>state===previous?name:state);
  if(region.initial===previous)region.initial=name;
  for(const transition of region.transitions){if(transition.from===previous)transition.from=name;if(transition.to===previous)transition.to=name;}
  for(const owner of next.design?.owners??[])if(owner.region===regionId)renameKey(owner.values,previous,name);
  for(const appearance of next.design?.appearances??[])if(appearance.regions[regionId]===previous)appearance.regions[regionId]=name;
  for(const motion of next.motions??[])for(const selector of [motion.fromRegions,motion.toRegions])if(selector?.[regionId]===previous)selector[regionId]=name;
  renameStateComparisons(next,regionId,previous,name);
  return next;
}
