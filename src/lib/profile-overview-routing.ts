import {diagramRoutes,type DiagramNode,type DiagramRouteOptions} from './profile-diagram';
import type {DeepReadonly,ProfileBinding} from './profiles';

type OverviewRoutes=ReturnType<typeof diagramRoutes>;

/** Route overview arrows with progressively smaller lead clearance when saved
 * block positions leave too little space for the preferred 36px channels. */
export function routeOverview(nodes:readonly DiagramNode[],bindings:readonly DeepReadonly<ProfileBinding>[],width:number,height:number,options:DiagramRouteOptions={}):OverviewRoutes {
  const unique=[...new Map(bindings.map(binding=>[binding.id,binding])).values()];
  const preferred=Number.isFinite(options.clearance)?Math.max(0,options.clearance!):36;
  const clearances=[...new Set([preferred,36,24,12,0].filter(value=>value<=preferred))];
  const routed=new Map<string,OverviewRoutes[number]>();
  for(const clearance of clearances){
    const pending=unique.some(binding=>!routed.has(binding.id));
    if(!pending)break;
    const retained=[...routed.values()].map(route=>({bindingId:route.binding.id,points:route.points,automatic:true}));
    const wireRoutes=[...(options.wireRoutes??[]),...retained];
    for(const route of diagramRoutes(nodes,unique,width,height,{...options,clearance,wireRoutes})){
      if(!routed.has(route.binding.id))routed.set(route.binding.id,route);
    }
  }
  return unique.flatMap(binding=>{const route=routed.get(binding.id);return route?[route]:[];});
}

/** Cache immutable overview geometry per canvas. Inventory refreshes often
 * rebuild the same node and route arrays without changing any geometry. */
export function createOverviewRouter(maxEntries=4):typeof diagramRoutes {
  const cache=new Map<string,OverviewRoutes>(),limit=Math.max(1,Math.min(16,Math.floor(maxEntries)||1));
  return (nodes,bindings,width,height,options={})=>{
    const unique=[...new Map(bindings.map(binding=>[binding.id,binding])).values()];
    const key=JSON.stringify([
      width,height,options.style??'smooth',options.cornerRadius??24,options.clearance??36,options.wireRoutes,
      nodes.map(node=>[node.id,node.position.x,node.position.y,node.width??width,node.height??height,node.inputAnchors,node.outputAnchors]),
      unique.map(binding=>[binding.id,binding.from.nodeId,binding.from.portId,binding.to.nodeId,binding.to.portId]),
    ]);
    const previous=cache.get(key);
    if(previous){
      const byId=new Map(unique.map(binding=>[binding.id,binding]));
      const routes=previous.some(route=>route.binding!==byId.get(route.binding.id))?
        previous.map(route=>({...route,binding:byId.get(route.binding.id)!})):previous;
      cache.delete(key);cache.set(key,routes);return routes;
    }
    const routes=routeOverview(nodes,unique,width,height,options);
    cache.set(key,routes);if(cache.size>limit)cache.delete(cache.keys().next().value!);
    return routes;
  };
}
