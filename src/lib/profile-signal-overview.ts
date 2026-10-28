import type {DeepReadonly, ProfileBinding, ProfileDocument, ProfileEndpoint} from './profiles';

export type SignalLane = {
  id:string;
  destination:DeepReadonly<ProfileEndpoint>;
  sources:DeepReadonly<ProfileEndpoint>[];
  transforms:string[];
  bindingIds:string[];
};
export type SignalBundle = {id:string; binding:ProfileBinding; lanes:SignalLane[]};

/** A presentation-only projection. A lane is one terminal input, not every
 * possible path through the DAG. Shared stages retain their real identities.
 * Exclusive vehicle alternatives need distinct terminal bindings; authoring
 * instances their stages per row instead of merging alternatives upstream. */
export function signalOverview(document:DeepReadonly<ProfileDocument>):SignalBundle[] {
  const processing=new Set(document.processingNodes.map(node=>node.id));
  const resources=new Set([...document.resources,...document.virtualOutputs,...(document.desktopOutputs??[])].map(node=>node.id));
  const incoming=new Map<string,DeepReadonly<ProfileBinding>[]>();
  for(const binding of document.bindings){const list=incoming.get(binding.to.nodeId)??[];list.push(binding);incoming.set(binding.to.nodeId,list);}
  const bundles=new Map<string,SignalBundle>();
  for(const terminal of document.bindings){
    if(!resources.has(terminal.to.nodeId))continue;
    const sources=new Map<string,DeepReadonly<ProfileEndpoint>>(),transforms:string[]=[],bindingIds=new Set<string>(),visited=new Set<string>();
    function visit(binding:DeepReadonly<ProfileBinding>){
      bindingIds.add(binding.id);
      const id=binding.from.nodeId;
      if(resources.has(id)){sources.set(JSON.stringify(binding.from),binding.from);return;}
      if(!processing.has(id)||visited.has(id))return;
      visited.add(id);
      for(const upstream of incoming.get(id)??[])visit(upstream);
      transforms.push(id);
    }
    visit(terminal);
    const lane:SignalLane={id:terminal.id,destination:terminal.to,sources:[...sources.values()],transforms,bindingIds:[...bindingIds]};
    for(const sourceId of new Set(lane.sources.map(source=>source.nodeId))){
      const id=`overview:${JSON.stringify([sourceId,terminal.to.nodeId])}`;
      let bundle=bundles.get(id);
      if(!bundle){bundle={id,binding:{id,from:{nodeId:sourceId,portId:''},to:{nodeId:terminal.to.nodeId,portId:''}},lanes:[]};bundles.set(id,bundle);}
      bundle.lanes.push(lane);
    }
  }
  return [...bundles.values()];
}

/** Stable identity for the presentation-only overview of one destination. */
export function destinationOverviewId(to:string):string {
  return `destination:${JSON.stringify(to)}`;
}

/** Groups source-pair arrows into destination arrows while keeping their real
 * lane data intact. Lanes shared by multiple source arrows appear only once. */
export function destinationOverview(bundles:readonly SignalBundle[]):SignalBundle[] {
  const destinations=new Map<string,SignalBundle>();
  for(const bundle of bundles){
    const to=bundle.binding.to.nodeId;
    let overview=destinations.get(to);
    if(!overview){
      const id=destinationOverviewId(to);
      overview={id,binding:{id,from:bundle.binding.from,to:bundle.binding.to},lanes:[]};
      destinations.set(to,overview);
    }
    const laneIds=new Set(overview.lanes.map(lane=>lane.id));
    for(const lane of bundle.lanes){
      if(laneIds.has(lane.id))continue;
      overview.lanes.push(lane);
      laneIds.add(lane.id);
    }
  }
  return [...destinations.values()];
}

/** Relative weights remain proportional even on very dense profiles. */
export function signalBundleWidth(count:number,maximum:number):number {
  return count*Math.min(3,14/Math.max(1,maximum));
}
